use eframe::egui;
use egui::Ui;

use crate::app::PedalmetricsApp;

/// Full-screen overlay shown during video rendering.
pub struct RenderOverlay<'a> {
    app: &'a mut PedalmetricsApp,
}

impl<'a> RenderOverlay<'a> {
    pub fn new(app: &'a mut PedalmetricsApp) -> Self {
        Self { app }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        let Some(vr) = &self.app.video_render else { return; };
        if vr.thread.is_none() { return; } // Finished — don't show overlay

        let current = vr.progress.current();
        let total = vr.progress.total_frames;
        let pct = vr.progress.percent();

        egui::Window::new("Rendering…")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .min_width(360.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.label(format!("Frame {} / {}", current, total));
                ui.add_space(4.0);
                ui.add(egui::ProgressBar::new(pct / 100.0)
                    .desired_width(320.0)
                    .show_percentage()
                    .animate(true));
                ui.add_space(8.0);

                if let Some(err) = &vr.error {
                    ui.colored_label(egui::Color32::RED, format!("Error: {}", err));
                }

                ui.add_space(4.0);
                if ui.button("Cancel").clicked() {
                    vr.progress.cancel();
                }
                ui.add_space(8.0);
            });
    }
}
