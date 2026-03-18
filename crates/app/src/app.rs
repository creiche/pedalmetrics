use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;
use egui::{ColorImage, TextureHandle, TextureOptions};
use image::RgbaImage;

use pedalmetrics_core::{
    Activity, Template,
    encoder::{RenderProgress, VideoEncoder},
    renderer::{RenderState, Renderer},
    constant::{fonts_dir, templates_dir},
};

use crate::ui::{
    control_panel::ControlPanel,
    preview::PreviewPanel,
    render_overlay::RenderOverlay,
    onboarding::OnboardingState,
};

// ---------------------------------------------------------------------------
// Application State
// ---------------------------------------------------------------------------

/// The loaded GPX + processed activity data.
pub struct LoadedActivity {
    pub path: PathBuf,
    pub source_activity: Activity,
    pub full_duration_seconds: usize,
}

/// Preview render request sent to the background thread.
struct PreviewRequest {
    frame_idx: usize,
    scale: f32,
}

/// Preview render result returned from the background thread.
struct PreviewResult {
    image: RgbaImage,
}

/// Video render state.
pub struct VideoRenderState {
    pub progress: RenderProgress,
    pub thread: Option<std::thread::JoinHandle<anyhow::Result<PathBuf>>>,
    pub output_path: Option<PathBuf>,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Main App
// ---------------------------------------------------------------------------

pub struct PedalmetricsApp {
    // --- Data ---
    pub loaded_activity: Option<LoadedActivity>,
    pub template: Template,

    // --- Render state (rebuilt on GPX load or template change) ---
    pub render_state: Option<Arc<RenderState>>,
    pub render_state_dirty: bool,

    // --- Timeline ---
    pub selected_second: u32,
    pub scrubbing: bool,

    // --- Preview ---
    pub preview_texture: Option<TextureHandle>,
    preview_pending: Arc<Mutex<Option<PreviewResult>>>,
    preview_request: Arc<Mutex<Option<PreviewRequest>>>,
    preview_thread_running: Arc<AtomicBool>,

    // --- Video render ---
    pub video_render: Option<VideoRenderState>,

    // --- Template list ---
    pub available_templates: Vec<(String, PathBuf)>,

    // --- UI state ---
    pub show_template_editor: bool,
    pub status_message: String,
}

impl PedalmetricsApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Load available templates
        let available_templates = Self::scan_templates_pub();

        // Use bundled default first, then user templates, then hardcoded fallback.
        let template = Self::bundled_default_template()
            .or_else(|| {
                available_templates
                    .first()
                    .and_then(|(_, path)| {
                        std::fs::read_to_string(path).ok()
                            .and_then(|s| Template::from_json(&s).ok())
                    })
            })
            .unwrap_or_else(Template::default_4k);

        Self {
            loaded_activity: None,
            template,
            render_state: None,
            render_state_dirty: false,
            selected_second: 0,
            scrubbing: false,
            preview_texture: None,
            preview_pending: Arc::new(Mutex::new(None)),
            preview_request: Arc::new(Mutex::new(None)),
            preview_thread_running: Arc::new(AtomicBool::new(false)),
            video_render: None,
            available_templates,
            show_template_editor: false,
            status_message: String::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Template discovery
    // -----------------------------------------------------------------------

    pub fn scan_templates_pub() -> Vec<(String, PathBuf)> {
        let mut templates = Vec::new();

        // User templates dir
        let user_dir = templates_dir();
        let _ = std::fs::create_dir_all(&user_dir);
        if let Ok(entries) = std::fs::read_dir(&user_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "json") {
                    let name = path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    templates.push((name, path));
                }
            }
        }

        // Built-in templates bundled at compile time are handled separately
        // via BUNDLED_TEMPLATES in the ui/control_panel.rs

        templates
    }

    fn sanitize_template_name(name: &str) -> String {
        let mut out = String::with_capacity(name.len());
        for ch in name.chars() {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                out.push(ch);
            } else if ch == ' ' {
                out.push('_');
            }
        }
        let out = out.trim_matches('_').to_string();
        if out.is_empty() { "template".to_string() } else { out }
    }

    pub fn save_current_template_to_dir(&mut self, dir: &std::path::Path) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;

        let base = self
            .template
            .scene
            .overlay_filename
            .strip_suffix(".mov")
            .unwrap_or("template");
        let base = Self::sanitize_template_name(base);

        let mut path = dir.join(format!("{}.json", base));
        if path.exists() {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            path = dir.join(format!("{}_{}.json", base, ts));
        }

        let json = self.template.to_json_pretty()?;
        std::fs::write(&path, json)?;
        Ok(path)
    }

    pub fn save_current_template(&mut self) -> anyhow::Result<PathBuf> {
        let path = self.save_current_template_to_dir(&templates_dir())?;
        self.available_templates = Self::scan_templates_pub();
        Ok(path)
    }

    pub fn load_template_from_path(&mut self, path: PathBuf) -> anyhow::Result<()> {
        let json = std::fs::read_to_string(&path)?;
        let t = Template::from_json(&json)?;
        self.on_template_change(t);
        self.available_templates = Self::scan_templates_pub();
        Ok(())
    }

    fn bundled_default_template() -> Option<Template> {
        Template::from_json(include_str!("../../../templates/init_template.json")).ok()
    }

    /// Returns an effective `[start, end)` range in absolute seconds, clamped to activity length.
    pub fn effective_scene_range(&self) -> Option<(u32, u32)> {
        let loaded = self.loaded_activity.as_ref()?;
        let max_end = loaded.full_duration_seconds as u32;
        if max_end == 0 {
            return None;
        }

        let start = self.template.scene.start.min(max_end.saturating_sub(1));
        let mut end = self.template.scene.end.min(max_end);
        if end <= start {
            end = (start + 1).min(max_end);
        }
        Some((start, end))
    }

    // -----------------------------------------------------------------------
    // GPX file loading
    // -----------------------------------------------------------------------

    pub fn load_gpx(&mut self, path: PathBuf) {
        self.status_message = format!("Loading {}…", path.display());
        match Activity::from_path(&path) {
            Ok(activity) => {
                let duration = activity.duration_seconds();
                self.selected_second = 0;
                self.loaded_activity = Some(LoadedActivity {
                    path,
                    source_activity: activity,
                    full_duration_seconds: duration,
                });
                self.render_state_dirty = true;
                self.status_message = format!("Loaded activity ({} seconds)", duration);
            }
            Err(e) => {
                self.status_message = format!("Error loading GPX: {}", e);
                log::error!("GPX load error: {:?}", e);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Render state management
    // -----------------------------------------------------------------------

    fn rebuild_render_state(&mut self) {
        let Some(loaded) = &self.loaded_activity else { return; };
        let Some((start, end)) = self.effective_scene_range() else { return; };

        let mut activity = loaded.source_activity.clone();
        // Keep both endpoints [start, end] in samples to produce (end-start)*fps frames.
        let trim_end_exclusive = (end as usize + 1).min(activity.times.len());
        if (start as usize) < trim_end_exclusive {
            if let Err(e) = activity.trim(start as usize, trim_end_exclusive) {
                self.status_message = format!("Render state error: {}", e);
                log::error!("Activity trim error: {:?}", e);
                return;
            }
        }
        activity.interpolate(self.template.scene.fps);

        let clip_duration = end.saturating_sub(start);
        self.selected_second = self.selected_second.min(clip_duration.saturating_sub(1));

        match RenderState::build(
            activity,
            self.template.clone(),
            fonts_dir(),
        ) {
            Ok(state) => {
                self.render_state = Some(Arc::new(state));
                self.render_state_dirty = false;
                self.trigger_preview();
            }
            Err(e) => {
                self.status_message = format!("Render state error: {}", e);
                log::error!("RenderState build error: {:?}", e);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Preview rendering (background thread)
    // -----------------------------------------------------------------------

    pub fn trigger_preview(&mut self) {
        let Some(state) = &self.render_state else { return; };
        let Some((start, end)) = self.effective_scene_range() else { return; };
        let clip_duration = end.saturating_sub(start);
        if clip_duration == 0 {
            return;
        }

        self.selected_second = self.selected_second.min(clip_duration - 1);

        let fps = self.template.scene.fps as usize;
        let frame_idx = self.selected_second as usize * fps;
        // Use half resolution for scrubbing, full resolution otherwise
        let scale = if self.scrubbing { 0.5 } else { 1.0 };

        // Enqueue request (latest wins)
        *self.preview_request.lock().unwrap() = Some(PreviewRequest { frame_idx, scale });

        // Spawn a preview thread if one isn't already running
        if self.preview_thread_running.load(Ordering::Relaxed) {
            return; // existing thread will pick up the latest request
        }

        let state = Arc::clone(state);
        let request_slot = Arc::clone(&self.preview_request);
        let result_slot = Arc::clone(&self.preview_pending);
        let running = Arc::clone(&self.preview_thread_running);

        running.store(true, Ordering::Relaxed);

        std::thread::spawn(move || {
            loop {
                let request = request_slot.lock().unwrap().take();
                match request {
                    None => break,
                    Some(req) => {
                        let result = if req.scale < 1.0 {
                            state.render_frame_scaled(req.frame_idx, req.scale)
                        } else {
                            state.render_frame(req.frame_idx)
                        };
                        if let Ok(img) = result {
                            *result_slot.lock().unwrap() = Some(PreviewResult {
                                image: img,
                            });
                        }
                    }
                }
            }
            running.store(false, Ordering::Relaxed);
        });
    }

    // -----------------------------------------------------------------------
    // Template change handler
    // -----------------------------------------------------------------------

    pub fn on_template_change(&mut self, new_template: Template) {
        self.template = new_template;
        self.selected_second = 0;
        self.render_state_dirty = true;
    }

    // -----------------------------------------------------------------------
    // Video render
    // -----------------------------------------------------------------------

    pub fn start_video_render_to(&mut self, output_path: PathBuf) {
        let Some(_loaded) = &self.loaded_activity else { return; };
        let Some(state) = &self.render_state else { return; };

        let total = self.template.scene.total_frames();
        let progress = RenderProgress::new(total);
        let state = Arc::clone(state);
        let template = self.template.clone();

        let fps = template.scene.fps;
        let width = template.scene.width;
        let height = template.scene.height;
        let progress_clone = progress.clone();
        let output_path_for_thread = output_path.clone();

        let handle = std::thread::spawn(move || {
            let renderer = Renderer::new((*state).clone());
            let encoder = VideoEncoder::new(&output_path_for_thread, width, height, fps);
            encoder.encode(renderer, Some(&progress_clone))
        });

        self.video_render = Some(VideoRenderState {
            progress,
            thread: Some(handle),
            output_path: Some(output_path),
            error: None,
        });
    }
}

// ---------------------------------------------------------------------------
// eframe::App implementation
// ---------------------------------------------------------------------------

impl eframe::App for PedalmetricsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Rebuild render state if dirty (GPX loaded or template changed)
        if self.render_state_dirty && self.loaded_activity.is_some() {
            self.rebuild_render_state();
        }

        // Poll preview result from background thread
        if let Some(result) = self.preview_pending.lock().unwrap().take() {
            let size = [result.image.width() as usize, result.image.height() as usize];
            let pixels: Vec<egui::Color32> = result.image
                .pixels()
                .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
                .collect();
            let color_image = ColorImage { size, pixels };
            self.preview_texture = Some(
                ctx.load_texture("preview", color_image, TextureOptions::LINEAR)
            );
        }

        // Request repaint while preview or render is in progress
        if self.preview_thread_running.load(Ordering::Relaxed) {
            ctx.request_repaint();
        }
        if self.video_render.as_ref().map_or(false, |r| r.thread.is_some()) {
            ctx.request_repaint();
        }

        // Check if video render finished
        if let Some(vr) = &mut self.video_render {
            if vr.thread.as_ref().map_or(false, |t| t.is_finished()) {
                let result = vr.thread.take().unwrap().join()
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("Render thread panicked")));
                match result {
                    Ok(path) => {
                        self.status_message = format!("✓ Saved to: {}", path.display());
                        vr.output_path = Some(path);
                    }
                    Err(e) => {
                        vr.error = Some(e.to_string());
                        self.status_message = format!("Render failed: {}", e);
                    }
                }
            }
        }

        // ---------- Layout ----------
        // Left panel: controls + template editor
        egui::SidePanel::left("control_panel")
            .min_width(300.0)
            .max_width(480.0)
            .show(ctx, |ui| {
                ControlPanel::new(self).show(ui);
            });

        // Central panel: preview or onboarding
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.loaded_activity.is_none() {
                OnboardingState::new(self).show(ui);
            } else {
                PreviewPanel::new(self).show(ui);
            }
        });

        // Render progress overlay (shown on top of everything during encode)
        if self.video_render.is_some() {
            RenderOverlay::new(self).show(ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    fn app_with_template(template: Template) -> PedalmetricsApp {
        PedalmetricsApp {
            loaded_activity: None,
            template,
            render_state: None,
            render_state_dirty: false,
            selected_second: 0,
            scrubbing: false,
            preview_texture: None,
            preview_pending: Arc::new(Mutex::new(None)),
            preview_request: Arc::new(Mutex::new(None)),
            preview_thread_running: Arc::new(AtomicBool::new(false)),
            video_render: None,
            available_templates: Vec::new(),
            show_template_editor: false,
            status_message: String::new(),
        }
    }

    #[test]
    fn save_current_template_to_dir_writes_template_json() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let mut template = Template::default_4k();
        template.scene.overlay_filename = "my custom overlay.mov".to_string();

        let mut app = app_with_template(template.clone());
        let path = app
            .save_current_template_to_dir(temp_dir.path())
            .expect("save template to temp dir");

        assert!(path.exists(), "saved template file should exist");
        let json = std::fs::read_to_string(&path).expect("read saved template");
        let roundtrip = Template::from_json(&json).expect("parse saved template");
        assert_eq!(roundtrip.scene.overlay_filename, template.scene.overlay_filename);
        assert_eq!(roundtrip.scene.width, template.scene.width);
        assert_eq!(roundtrip.scene.height, template.scene.height);
        assert_eq!(roundtrip.scene.fps, template.scene.fps);
    }

    #[test]
    fn load_template_from_path_updates_template_state() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");

        let mut original = Template::default_4k();
        original.scene.overlay_filename = "loader test.mov".to_string();
        original.scene.width = 1280;
        original.scene.height = 720;
        original.scene.fps = 24;

        let mut app = app_with_template(original.clone());
        let path = app
            .save_current_template_to_dir(temp_dir.path())
            .expect("save source template");

        app.template = Template::default_4k();
        app.load_template_from_path(path).expect("load template from path");

        assert_eq!(app.template.scene.overlay_filename, original.scene.overlay_filename);
        assert_eq!(app.template.scene.width, 1280);
        assert_eq!(app.template.scene.height, 720);
        assert_eq!(app.template.scene.fps, 24);
    }
}

