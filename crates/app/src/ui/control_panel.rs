use eframe::egui;
use egui::Ui;
use rfd::FileDialog;

use crate::app::PedalmetricsApp;
use pedalmetrics_core::Template;

/// Bundled template files (compiled into the binary).
const BUNDLED_TEMPLATES: &[(&str, &str)] = &[
    ("Walker Crit A", include_str!("../../../../templates/walker_crit_a.json")),
];

pub struct ControlPanel<'a> {
    app: &'a mut PedalmetricsApp,
}

impl<'a> ControlPanel<'a> {
    pub fn new(app: &'a mut PedalmetricsApp) -> Self {
        Self { app }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        ui.set_min_width(300.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            self.show_file_section(ui);
            ui.separator();
            self.show_template_section(ui);
            ui.separator();
            self.show_scene_section(ui);
            ui.separator();
            self.show_render_section(ui);
            ui.add_space(8.0);

            if !self.app.status_message.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new(&self.app.status_message.clone()).small());
            }
        });
    }

    // -----------------------------------------------------------------------
    // File section
    // -----------------------------------------------------------------------

    fn show_file_section(&mut self, ui: &mut Ui) {
        ui.heading("Activity");
        ui.add_space(4.0);

        if let Some(loaded) = &self.app.loaded_activity {
            let name = loaded.path.file_name()
                .unwrap_or_default()
                .to_string_lossy();
            ui.label(format!("📄 {}", name));
            ui.label(format!("Duration: {}:{:02}",
                loaded.duration_seconds / 60,
                loaded.duration_seconds % 60));
            ui.add_space(4.0);
        }

        if ui.button("📂 Open GPX File…").clicked() {
            if let Some(path) = FileDialog::new()
                .add_filter("GPX Files", &["gpx"])
                .pick_file()
            {
                self.app.load_gpx(path);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Template section
    // -----------------------------------------------------------------------

    fn show_template_section(&mut self, ui: &mut Ui) {
        ui.heading("Template");
        ui.add_space(4.0);

        // Bundled templates
        ui.label("Built-in:");
        for (name, json) in BUNDLED_TEMPLATES {
            if ui.selectable_label(false, *name).clicked() {
                if let Ok(t) = Template::from_json(json) {
                    self.app.on_template_change(t);
                }
            }
        }

        // User templates
        if !self.app.available_templates.is_empty() {
            ui.add_space(4.0);
            ui.label("My Templates:");
            let templates: Vec<(String, std::path::PathBuf)> = self.app.available_templates.clone();
            for (name, path) in &templates {
                if ui.selectable_label(false, name.as_str()).clicked() {
                    if let Ok(json) = std::fs::read_to_string(path) {
                        if let Ok(t) = Template::from_json(&json) {
                            self.app.on_template_change(t);
                        }
                    }
                }
            }
        }

        ui.add_space(4.0);

        // Save template
        if ui.button("💾 Save Template…").clicked() {
            if let Some(path) = FileDialog::new()
                .set_file_name("my_template.json")
                .add_filter("JSON", &["json"])
                .save_file()
            {
                if let Ok(json) = self.app.template.to_json_pretty() {
                    let _ = std::fs::write(&path, json);
                    self.app.status_message = format!("Saved: {}", path.display());
                    self.app.available_templates = PedalmetricsApp::scan_templates_pub();
                }
            }
        }

        ui.add_space(4.0);

        // Toggle template editor panel
        let editor_label = if self.app.show_template_editor {
            "▲ Close Editor"
        } else {
            "✏ Edit Template"
        };
        if ui.button(editor_label).clicked() {
            self.app.show_template_editor = !self.app.show_template_editor;
        }

        // Inline template editor
        if self.app.show_template_editor {
            ui.add_space(4.0);
            let mut te = super::template_editor::TemplateEditor::new(self.app);
            te.show(ui);
        }
    }

    // -----------------------------------------------------------------------
    // Scene settings section
    // -----------------------------------------------------------------------

    fn show_scene_section(&mut self, ui: &mut Ui) {
        ui.heading("Scene");
        ui.add_space(4.0);

        let mut changed = false;
        let scene = &mut self.app.template.scene;

        // Resolution preset
        ui.horizontal(|ui| {
            ui.label("Resolution:");
            if ui.selectable_label(scene.width == 3840, "4K").clicked() {
                scene.width = 3840; scene.height = 2160; changed = true;
            }
            if ui.selectable_label(scene.width == 1920, "1080p").clicked() {
                scene.width = 1920; scene.height = 1080; changed = true;
            }
            if ui.selectable_label(scene.width == 1280, "720p").clicked() {
                scene.width = 1280; scene.height = 720; changed = true;
            }
        });

        // FPS
        ui.horizontal(|ui| {
            ui.label("FPS:");
            for fps in [24u32, 30, 60] {
                if ui.selectable_label(scene.fps == fps, fps.to_string()).clicked() {
                    scene.fps = fps; changed = true;
                }
            }
        });

        // Font size
        ui.horizontal(|ui| {
            ui.label("Font Size:");
            if ui.add(egui::Slider::new(&mut scene.font_size, 8.0..=300.0)).changed() {
                changed = true;
            }
        });

        // Opacity
        let mut opacity_pct = scene.opacity * 100.0;
        ui.horizontal(|ui| {
            ui.label("Opacity:");
            if ui.add(egui::Slider::new(&mut opacity_pct, 0.0..=100.0).suffix("%")).changed() {
                scene.opacity = opacity_pct / 100.0;
                changed = true;
            }
        });

        if changed {
            self.app.render_state_dirty = true;
        }
    }

    // -----------------------------------------------------------------------
    // Render section
    // -----------------------------------------------------------------------

    fn show_render_section(&mut self, ui: &mut Ui) {
        ui.heading("Render");
        ui.add_space(4.0);

        let can_render = self.app.loaded_activity.is_some()
            && self.app.render_state.is_some()
            && self.app.video_render.as_ref().map_or(true, |r| r.thread.is_none());

        if ui.add_enabled(can_render, egui::Button::new("▶ Render Video")).clicked() {
            self.app.start_video_render();
        }

        if let Some(vr) = &self.app.video_render {
            if let Some(path) = &vr.output_path {
                if vr.thread.is_none() {
                    // Render finished
                    if ui.button("📁 Show in Finder").clicked() {
                        let _ = std::process::Command::new("open")
                            .arg("-R")
                            .arg(path)
                            .spawn();
                    }
                }
            }
        }
    }
}
