use eframe::egui;
use egui::Ui;

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
        let Some((start, end)) = self.app.effective_scene_range() else { return; };
        let clip_duration = end.saturating_sub(start);
        if clip_duration == 0 { return; }

        let mut selected = self.app.selected_second.min(clip_duration.saturating_sub(1));

        ui.horizontal(|ui| {
            ui.label(format!("{}", fmt_time(start)));

            let resp = ui.add(
                egui::Slider::new(&mut selected, 0..=clip_duration.saturating_sub(1))
                    .show_value(false)
            );

            ui.label(format!("{}", fmt_time(start + selected)));
            ui.label(format!("/ {}", fmt_time(end)));

            if resp.changed() {
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
            let full_duration = self
                .app
                .loaded_activity
                .as_ref()
                .map(|l| l.full_duration_seconds as u32)
                .unwrap_or(0);
            if full_duration == 0 {
                return;
            }

            ui.label("Start:");
            let mut scene_start = self.app.template.scene.start;
            if ui.add(egui::DragValue::new(&mut scene_start)
                .range(0..=full_duration.saturating_sub(1))
                .suffix("s")).changed()
            {
                self.app.template.scene.start = scene_start;
                self.app.render_state_dirty = true;
            }

            ui.label("End:");
            let mut scene_end = self.app.template.scene.end;
            if ui.add(egui::DragValue::new(&mut scene_end)
                .range(scene_start + 1..=full_duration)
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
