use eframe::egui;
use egui::Ui;

use crate::app::PedalmetricsApp;

/// Shown in the central panel when no GPX file is loaded.
pub struct OnboardingState<'a> {
    app: &'a mut PedalmetricsApp,
}

impl<'a> OnboardingState<'a> {
    pub fn new(app: &'a mut PedalmetricsApp) -> Self {
        Self { app }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.heading("Pedalmetrics");
                ui.add_space(16.0);
                ui.label("Create telemetry overlay videos from GPX data.");
                ui.add_space(32.0);

                if ui.button(
                    egui::RichText::new("📂  Open GPX File…").size(18.0)
                ).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("GPX Files", &["gpx"])
                        .pick_file()
                    {
                        self.app.load_gpx(path);
                    }
                }

                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new("or drag and drop a .gpx file here")
                        .small()
                        .color(egui::Color32::GRAY)
                );

                ui.add_space(48.0);
                ui.label(
                    egui::RichText::new(
                        "An independent Rust rewrite inspired by Cyclemetry.\nNot affiliated with or maintained by the original authors."
                    )
                    .small()
                    .color(egui::Color32::from_gray(100))
                );
            });
        });

        // Handle drag-and-drop
        if !ui.ctx().input(|i| i.raw.dropped_files.is_empty()) {
            if let Some(file) = ui.ctx().input(|i| i.raw.dropped_files.first().cloned()) {
                if let Some(path) = file.path {
                    if path.extension().map_or(false, |e| e == "gpx") {
                        self.app.load_gpx(path);
                    }
                }
            }
        }
    }
}
