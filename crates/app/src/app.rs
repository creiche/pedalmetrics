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
    constant::{downloads_dir, fonts_dir, templates_dir},
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
    pub activity: Activity,
    pub duration_seconds: usize,
}

/// Preview render request sent to the background thread.
struct PreviewRequest {
    frame_idx: usize,
    scale: f32,
}

/// Preview render result returned from the background thread.
struct PreviewResult {
    image: RgbaImage,
    frame_idx: usize,
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
    pub preview_pending: Arc<Mutex<Option<PreviewResult>>>,
    pub preview_request: Arc<Mutex<Option<PreviewRequest>>>,
    pub preview_thread_running: Arc<AtomicBool>,

    // --- Video render ---
    pub video_render: Option<VideoRenderState>,

    // --- Template list ---
    pub available_templates: Vec<(String, PathBuf)>,

    // --- UI state ---
    pub show_template_editor: bool,
    pub status_message: String,
}

impl PedalmetricsApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Load available templates
        let available_templates = Self::scan_templates_pub();

        // Load the first bundled template as the default
        let template = available_templates
            .first()
            .and_then(|(_, path)| {
                std::fs::read_to_string(path).ok()
                    .and_then(|s| Template::from_json(&s).ok())
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

    // -----------------------------------------------------------------------
    // GPX file loading
    // -----------------------------------------------------------------------

    pub fn load_gpx(&mut self, path: PathBuf) {
        self.status_message = format!("Loading {}…", path.display());
        match Activity::from_path(&path) {
            Ok(mut activity) => {
                let duration = activity.duration_seconds();
                // Trim and interpolate with scene settings
                let start = self.template.scene.start as usize;
                let end = (self.template.scene.end as usize).min(duration);
                let fps = self.template.scene.fps;

                if start < end {
                    let _ = activity.trim(start, end);
                }
                activity.interpolate(fps);

                self.selected_second = 0;
                self.loaded_activity = Some(LoadedActivity {
                    path,
                    activity,
                    duration_seconds: duration,
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

        match RenderState::build(
            loaded.activity.clone(),
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
                                frame_idx: req.frame_idx,
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
        self.render_state_dirty = true;
    }

    // -----------------------------------------------------------------------
    // Video render
    // -----------------------------------------------------------------------

    pub fn start_video_render(&mut self) {
        let Some(loaded) = &self.loaded_activity else { return; };
        let Some(state) = &self.render_state else { return; };

        let total = self.template.scene.total_frames();
        let progress = RenderProgress::new(total);
        let state = Arc::clone(state);
        let template = self.template.clone();
        let output_path = downloads_dir()
            .join(&template.scene.overlay_filename);

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
