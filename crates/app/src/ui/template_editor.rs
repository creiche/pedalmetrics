use eframe::egui;
use egui::Ui;

use crate::app::PedalmetricsApp;
use pedalmetrics_core::template::{Color, LabelConfig, UnitSystem, ValueType};

pub struct TemplateEditor<'a> {
    app: &'a mut PedalmetricsApp,
}

impl<'a> TemplateEditor<'a> {
    pub fn new(app: &'a mut PedalmetricsApp) -> Self {
        Self { app }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        let template = &mut self.app.template;
        let mut changed = false;

        ui.separator();
        ui.strong("Scene");
        ui.add_space(4.0);

        // Scene color (used for text + plot lines)
        ui.horizontal(|ui| {
            ui.label("Color:");
            let rgba = template.scene.color.to_rgba();
            let mut c32 = egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            if ui.color_edit_button_srgba(&mut c32).changed() {
                let [r, g, b, _] = c32.to_array();
                template.scene.color = Color(format!("#{:02x}{:02x}{:02x}", r, g, b));
                changed = true;
            }
        });

        ui.separator();

        // Labels
        ui.strong("Labels");
        ui.add_space(4.0);

        let mut remove_label: Option<usize> = None;
        // Iterate by index to avoid borrow issues
        for i in 0..template.labels.len() {
            let header = template.labels[i].text.chars().take(20).collect::<String>();
            egui::CollapsingHeader::new(if header.is_empty() { "label".to_string() } else { header })
                .id_salt(format!("label_{i}"))
                .show(ui, |ui| {
                    let label = &mut template.labels[i];
                    ui.horizontal(|ui| {
                        ui.label("Text:");
                        if ui.text_edit_singleline(&mut label.text).changed() {
                            changed = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("X:");
                        if ui.add(egui::DragValue::new(&mut label.x).range(-500..=7680).speed(1.0)).changed() {
                            changed = true;
                        }
                        ui.label("Y:");
                        if ui.add(egui::DragValue::new(&mut label.y).range(-500..=4320).speed(1.0)).changed() {
                            changed = true;
                        }
                    });
                    let mut fs = label.font_size.unwrap_or(48.0);
                    ui.horizontal(|ui| {
                        ui.label("Font size:");
                        if ui.add(egui::DragValue::new(&mut fs).range(6.0..=400.0).speed(0.5)).changed() {
                            label.font_size = Some(fs);
                            changed = true;
                        }
                    });
                    if ui.small_button("Remove").clicked() {
                        remove_label = Some(i);
                    }
                });
        }
        if let Some(idx) = remove_label {
            template.labels.remove(idx);
            changed = true;
        }
        if ui.small_button("+ Add Label").clicked() {
            template.labels.push(LabelConfig {
                text: "Label".to_string(),
                x: 100,
                y: 100,
                font_size: Some(48.0),
                font: None,
                color: None,
                opacity: None,
            });
            changed = true;
        }

        ui.separator();

        // Values
        ui.strong("Values");
        ui.add_space(4.0);

        for i in 0..template.values.len() {
            let header = format!("{:?}", template.values[i].value);
            egui::CollapsingHeader::new(header)
                .id_salt(format!("value_{i}"))
                .show(ui, |ui| {
                    let value = &mut template.values[i];

                    ui.horizontal(|ui| {
                        ui.label("X:");
                        if ui.add(egui::DragValue::new(&mut value.x).range(-500..=7680).speed(1.0)).changed() {
                            changed = true;
                        }
                        ui.label("Y:");
                        if ui.add(egui::DragValue::new(&mut value.y).range(-500..=4320).speed(1.0)).changed() {
                            changed = true;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Type:");
                        egui::ComboBox::from_id_salt(format!("vtype_{i}"))
                            .selected_text(format!("{:?}", value.value))
                            .show_ui(ui, |ui| {
                                for vt in [
                                    ValueType::Speed, ValueType::Power, ValueType::HeartRate,
                                    ValueType::Cadence, ValueType::Gradient, ValueType::Elevation,
                                    ValueType::Distance, ValueType::Time, ValueType::Temperature,
                                ] {
                                    if ui.selectable_label(value.value == vt, format!("{vt:?}")).clicked() {
                                        value.value = vt;
                                        changed = true;
                                    }
                                }
                            });
                    });

                    ui.horizontal(|ui| {
                        ui.label("Units:");
                        let selected = value.unit.map(|u| format!("{u:?}"))
                            .unwrap_or_else(|| "Inherit".to_string());
                        egui::ComboBox::from_id_salt(format!("units_{i}"))
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                if ui.selectable_label(value.unit.is_none(), "Inherit").clicked() {
                                    value.unit = None;
                                    changed = true;
                                }
                                if ui.selectable_label(value.unit == Some(UnitSystem::Imperial), "Imperial").clicked() {
                                    value.unit = Some(UnitSystem::Imperial);
                                    changed = true;
                                }
                                if ui.selectable_label(value.unit == Some(UnitSystem::Metric), "Metric").clicked() {
                                    value.unit = Some(UnitSystem::Metric);
                                    changed = true;
                                }
                            });
                    });

                    let mut fs = value.font_size.unwrap_or(80.0);
                    ui.horizontal(|ui| {
                        ui.label("Font size:");
                        if ui.add(egui::DragValue::new(&mut fs).range(6.0..=400.0).speed(1.0)).changed() {
                            value.font_size = Some(fs);
                            changed = true;
                        }
                    });
                });
        }

        ui.separator();

        // Plots
        ui.strong("Plots");
        ui.add_space(4.0);

        for i in 0..template.plots.len() {
            let header = format!("{:?} plot", template.plots[i].value);
            egui::CollapsingHeader::new(header)
                .id_salt(format!("plot_{i}"))
                .show(ui, |ui| {
                    let plot = &mut template.plots[i];
                    ui.horizontal(|ui| {
                        ui.label("X:");
                        if ui.add(egui::DragValue::new(&mut plot.x).range(-2000..=7680).speed(1.0)).changed() {
                            changed = true;
                        }
                        ui.label("Y:");
                        if ui.add(egui::DragValue::new(&mut plot.y).range(-2000..=4320).speed(1.0)).changed() {
                            changed = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("W:");
                        if ui.add(egui::DragValue::new(&mut plot.width).range(10..=7680u32).speed(1.0)).changed() {
                            changed = true;
                        }
                        ui.label("H:");
                        if ui.add(egui::DragValue::new(&mut plot.height).range(10..=4320u32).speed(1.0)).changed() {
                            changed = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Rotation:");
                        if ui.add(egui::DragValue::new(&mut plot.rotation).range(-180.0_f32..=180.0).speed(0.5)).changed() {
                            changed = true;
                        }
                    });
                });
        }

        if changed {
            self.app.render_state_dirty = true;
        }
    }
}
