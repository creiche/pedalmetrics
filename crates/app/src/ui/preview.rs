use eframe::egui;
use egui::{Align2, Ui};

use crate::app::PedalmetricsApp;

pub struct PreviewPanel<'a> {
    app: &'a mut PedalmetricsApp,
}

impl<'a> PreviewPanel<'a> {
    pub fn new(app: &'a mut PedalmetricsApp) -> Self {
        Self { app }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        let available = ui.available_size();

        egui::Frame::dark_canvas(ui.style()).show(ui, |ui| {
            ui.set_min_size(available);

            // Preview image
            if let Some(texture) = &self.app.preview_texture {
                let img_size = texture.size_vec2();
                // Scale to fit the available area while maintaining aspect ratio
                let scale = (available.x / img_size.x).min(available.y / img_size.y).min(1.0);
                let display_size = img_size * scale;

                ui.centered_and_justified(|ui| {
                    ui.add(egui::Image::new(texture).fit_to_exact_size(display_size));
                });
            } else {
                // Loading spinner
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                    ui.label("Generating preview…");
                });
            }
        });

        // Timeline scrubber below the preview
        ui.add_space(8.0);
        self.show_timeline(ui);
    }

    fn show_timeline(&mut self, ui: &mut Ui) {
        let Some(loaded) = &self.app.loaded_activity else { return; };
        let duration = loaded.duration_seconds as u32;
        if duration == 0 { return; }

        let start = self.app.template.scene.start;
        let end = self.app.template.scene.end.min(duration);
        let mut selected = self.app.selected_second.clamp(start, end.saturating_sub(1));

        ui.horizontal(|ui| {
            ui.label(format!("{}", fmt_time(start)));

            let resp = ui.add(
                egui::Slider::new(&mut selected, start..=end.saturating_sub(1))
                    .show_value(false)
                    .clamp_to_range(true)
            );

            ui.label(format!("{}", fmt_time(selected)));
            ui.label(format!("/ {}", fmt_time(end)));

            if resp.changed() {
                let was_scrubbing = self.app.scrubbing;
                self.app.scrubbing = resp.dragged();
                self.app.selected_second = selected;

                // Trigger preview on every scrub tick
                self.app.trigger_preview();
            }

            if resp.drag_stopped() {
                self.app.scrubbing = false;
                // Final full-res preview on drag release
                self.app.trigger_preview();
            }
        });

        // Scene start/end time range
        ui.horizontal(|ui| {
            ui.label("Start:");
            let mut scene_start = self.app.template.scene.start;
            if ui.add(egui::DragValue::new(&mut scene_start)
                .range(0..=end.saturating_sub(1))
                .suffix("s")).changed()
            {
                self.app.template.scene.start = scene_start;
                self.app.render_state_dirty = true;
            }

            ui.label("End:");
            let mut scene_end = self.app.template.scene.end;
            if ui.add(egui::DragValue::new(&mut scene_end)
                .range(scene_start + 1..=duration)
                .suffix("s")).changed()
            {
                self.app.template.scene.end = scene_end;
                self.app.render_state_dirty = true;
            }
        });
    }
}

fn fmt_time(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
