use eframe::egui;
use egui::Ui;

use crate::app::PedalmetricsApp;
use pedalmetrics_core::template::{
    Color, FillStyle, LabelConfig, PointStyle, UnitSystem, ValueConfig, ValueLabelPosition,
    ValueType,
};

const ALL_VALUE_TYPES: [ValueType; 10] = [
    ValueType::Speed,
    ValueType::Power,
    ValueType::HeartRate,
    ValueType::Cadence,
    ValueType::Gradient,
    ValueType::Elevation,
    ValueType::Distance,
    ValueType::Time,
    ValueType::Timecode,
    ValueType::Temperature,
];

fn default_value_config(vtype: ValueType) -> ValueConfig {
    ValueConfig {
        value: vtype,
        x: 100,
        y: 100,
        unit: None,
        font: None,
        font_size: Some(80.0),
        color: None,
        opacity: None,
        suffix: None,
        decimal_rounding: None,
        hours_offset: None,
        time_format: None,
        value_label: Some(vtype.display_name().to_string()),
        value_label_position: Some(ValueLabelPosition::Below),
    }
}

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

        // Enable/disable value fields by type.
        ui.label("Show fields:");
        for vtype in ALL_VALUE_TYPES {
            let exists = template.values.iter().any(|v| v.value == vtype);
            let mut enabled = exists;
            if ui.checkbox(&mut enabled, vtype.display_name()).changed() {
                if enabled && !exists {
                    template.values.push(default_value_config(vtype));
                } else if !enabled && exists {
                    template.values.retain(|v| v.value != vtype);
                }
                changed = true;
            }
        }

        ui.add_space(4.0);

        // Per-value settings for enabled fields.
        for i in 0..template.values.len() {
            let header = template.values[i].value.display_name().to_string();
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

                    let mut value_label_text = value.value_label.clone().unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.label("Attached label:");
                        if ui.text_edit_singleline(&mut value_label_text).changed() {
                            let trimmed = value_label_text.trim();
                            value.value_label = if trimmed.is_empty() {
                                None
                            } else {
                                Some(trimmed.to_string())
                            };
                            changed = true;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Label position:");
                        let selected = match value.value_label_position.unwrap_or(ValueLabelPosition::Below) {
                            ValueLabelPosition::Above => "Above value",
                            ValueLabelPosition::Below => "Below value",
                        };
                        egui::ComboBox::from_id_salt(format!("value_label_position_{i}"))
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(
                                        value.value_label_position.unwrap_or(ValueLabelPosition::Below)
                                            == ValueLabelPosition::Above,
                                        "Above value",
                                    )
                                    .clicked()
                                {
                                    value.value_label_position = Some(ValueLabelPosition::Above);
                                    changed = true;
                                }
                                if ui
                                    .selectable_label(
                                        value.value_label_position.unwrap_or(ValueLabelPosition::Below)
                                            == ValueLabelPosition::Below,
                                        "Below value",
                                    )
                                    .clicked()
                                {
                                    value.value_label_position = Some(ValueLabelPosition::Below);
                                    changed = true;
                                }
                            });
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
                        ui.label("Line width:");
                        let mut line_width = plot.line.width;
                        if ui
                            .add(
                                egui::DragValue::new(&mut line_width)
                                    .range(0.1..=64.0)
                                    .speed(0.1),
                            )
                            .changed()
                        {
                            plot.line.width = line_width.max(0.1);
                            changed = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Line color:");
                        let rgba = plot
                            .line
                            .color
                            .as_ref()
                            .map(|c| c.to_rgba())
                            .unwrap_or([255, 255, 255, 255]);
                        let mut c32 = egui::Color32::from_rgba_unmultiplied(
                            rgba[0], rgba[1], rgba[2], rgba[3],
                        );
                        if ui.color_edit_button_srgba(&mut c32).changed() {
                            let [r, g, b, _] = c32.to_array();
                            plot.line.color = Some(Color(format!("#{:02x}{:02x}{:02x}", r, g, b)));
                            changed = true;
                        }
                        if ui.small_button("Inherit").clicked() {
                            plot.line.color = None;
                            changed = true;
                        }
                    });

                    let mut fill_enabled = plot.fill.is_some();
                    if ui.checkbox(&mut fill_enabled, "Fill under line").changed() {
                        if fill_enabled {
                            plot.fill = Some(FillStyle {
                                opacity: 0.65,
                                color: None,
                            });
                        } else {
                            plot.fill = None;
                        }
                        changed = true;
                    }

                    if let Some(fill) = plot.fill.as_mut() {
                        ui.horizontal(|ui| {
                            ui.label("Fill opacity:");
                            let mut fill_opacity_pct = (fill.opacity * 100.0).clamp(0.0, 100.0);
                            if ui
                                .add(egui::Slider::new(&mut fill_opacity_pct, 0.0..=100.0).suffix("%"))
                                .changed()
                            {
                                fill.opacity = (fill_opacity_pct / 100.0).clamp(0.0, 1.0);
                                changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Fill color:");
                            let rgba = fill
                                .color
                                .as_ref()
                                .map(|c| c.to_rgba())
                                .unwrap_or([255, 255, 255, 255]);
                            let mut c32 = egui::Color32::from_rgba_unmultiplied(
                                rgba[0], rgba[1], rgba[2], rgba[3],
                            );
                            if ui.color_edit_button_srgba(&mut c32).changed() {
                                let [r, g, b, _] = c32.to_array();
                                fill.color = Some(Color(format!("#{:02x}{:02x}{:02x}", r, g, b)));
                                changed = true;
                            }
                            if ui.small_button("Inherit").clicked() {
                                fill.color = None;
                                changed = true;
                            }
                        });
                    }

                    ui.horizontal(|ui| {
                        ui.label("Rotation:");
                        if ui.add(egui::DragValue::new(&mut plot.rotation).range(-180.0_f32..=180.0).speed(0.5)).changed() {
                            changed = true;
                        }
                    });

                    ui.add_space(6.0);
                    ui.label("Position markers:");

                    let mut remove_point: Option<usize> = None;
                    for pidx in 0..plot.points.len() {
                        egui::CollapsingHeader::new(format!("Marker {}", pidx + 1))
                            .id_salt(format!("plot_{i}_point_{pidx}"))
                            .show(ui, |ui| {
                                let point = &mut plot.points[pidx];

                                ui.horizontal(|ui| {
                                    ui.label("Radius (px):");
                                    let mut marker_radius = point.radius;
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut marker_radius)
                                                .range(0.5..=200.0)
                                                .speed(0.25),
                                        )
                                        .changed()
                                    {
                                        point.radius = marker_radius.max(0.5);
                                        changed = true;
                                    }
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Opacity:");
                                    let mut opacity_pct = (point.opacity * 100.0).clamp(0.0, 100.0);
                                    if ui
                                        .add(egui::Slider::new(&mut opacity_pct, 0.0..=100.0).suffix("%"))
                                        .changed()
                                    {
                                        point.opacity = (opacity_pct / 100.0).clamp(0.0, 1.0);
                                        changed = true;
                                    }
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Color:");
                                    let rgba = point
                                        .color
                                        .as_ref()
                                        .map(|c| c.to_rgba())
                                        .unwrap_or([255, 255, 255, 255]);
                                    let mut c32 = egui::Color32::from_rgba_unmultiplied(
                                        rgba[0], rgba[1], rgba[2], rgba[3],
                                    );
                                    if ui.color_edit_button_srgba(&mut c32).changed() {
                                        let [r, g, b, _] = c32.to_array();
                                        point.color = Some(Color(format!("#{:02x}{:02x}{:02x}", r, g, b)));
                                        changed = true;
                                    }

                                    if ui.small_button("Use plot color").clicked() {
                                        point.color = None;
                                        changed = true;
                                    }
                                });

                                if ui.small_button("Remove marker").clicked() {
                                    remove_point = Some(pidx);
                                }
                            });
                    }

                    if let Some(pidx) = remove_point {
                        plot.points.remove(pidx);
                        changed = true;
                    }

                    if ui.small_button("+ Add marker").clicked() {
                        plot.points.push(PointStyle::default());
                        changed = true;
                    }
                });
        }

        if changed {
            self.app.render_state_dirty = true;
        }
    }
}
