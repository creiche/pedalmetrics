use eframe::egui;
use egui::Ui;

use crate::app::PedalmetricsApp;

const TIMELINE_HEIGHT: f32 = 64.0;
const PREVIEW_GAP: f32 = 4.0;
const MIN_PREVIEW_HEIGHT: f32 = 80.0;

fn preview_matte_color() -> egui::Color32 {
    // Distinct from typical black video content so frame bounds are always visible.
    egui::Color32::from_rgb(28, 34, 42)
}

fn preview_frame_stroke() -> egui::Stroke {
    egui::Stroke::new(1.0, egui::Color32::from_rgb(148, 160, 174))
}

fn draw_alpha_checkerboard(ui: &mut Ui, rect: egui::Rect) {
    let light = egui::Color32::from_rgb(64, 72, 84);
    let dark = egui::Color32::from_rgb(44, 52, 64);
    let cell = 20.0;

    let cols = ((rect.width() / cell).ceil() as i32).max(1);
    let rows = ((rect.height() / cell).ceil() as i32).max(1);

    for row in 0..rows {
        for col in 0..cols {
            let x = rect.left() + col as f32 * cell;
            let y = rect.top() + row as f32 * cell;
            let r = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell, cell)).intersect(rect);
            let color = if (row + col) % 2 == 0 { light } else { dark };
            ui.painter().rect_filled(r, 0.0, color);
        }
    }
}

fn fitted_size_for_aspect(container: egui::Vec2, aspect: f32) -> egui::Vec2 {
    let mut size = container;
    if aspect <= 0.0 {
        return size;
    }
    if (size.x / size.y) > aspect {
        size.x = size.y * aspect;
    } else {
        size.y = size.x / aspect;
    }
    size
}

#[derive(Debug, Clone, Copy)]
struct PreviewLayout {
    preview_size: egui::Vec2,
    timeline_height: f32,
    gap: f32,
}

fn compute_preview_layout(available: egui::Vec2, scene_aspect: f32) -> PreviewLayout {
    let max_preview_size = egui::vec2(
        available.x,
        (available.y - TIMELINE_HEIGHT - PREVIEW_GAP).max(MIN_PREVIEW_HEIGHT),
    );
    let preview_size = fitted_size_for_aspect(max_preview_size, scene_aspect);

    PreviewLayout {
        preview_size,
        timeline_height: TIMELINE_HEIGHT,
        gap: PREVIEW_GAP,
    }
}

pub struct PreviewPanel<'a> {
    app: &'a mut PedalmetricsApp,
}

impl<'a> PreviewPanel<'a> {
    pub fn new(app: &'a mut PedalmetricsApp) -> Self {
        Self { app }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        let available = ui.available_size();
        let scene_w = self.app.template.scene.width.max(1) as f32;
        let scene_h = self.app.template.scene.height.max(1) as f32;
        let scene_aspect = scene_w / scene_h;
        let layout = compute_preview_layout(available, scene_aspect);
        let preview_size = layout.preview_size;

        // Use full panel width for layout, but only allocate the exact fitted preview height.
        ui.allocate_ui(egui::vec2(available.x, preview_size.y), |ui| {
            ui.set_min_size(egui::vec2(available.x, preview_size.y));

            ui.centered_and_justified(|ui| {
                let (rect, _) = ui.allocate_exact_size(preview_size, egui::Sense::hover());
                ui.painter().rect_filled(rect, 0.0, preview_matte_color());
                draw_alpha_checkerboard(ui, rect.shrink(1.0));
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    preview_frame_stroke(),
                    egui::StrokeKind::Inside,
                );

                if let Some(texture) = &self.app.preview_texture {
                    ui.put(rect, egui::Image::new(texture).fit_to_exact_size(preview_size));
                } else {
                    ui.put(
                        rect,
                        egui::Label::new(egui::RichText::new("Generating preview…").color(egui::Color32::WHITE)),
                    );
                }
            });
        });

        ui.add_space(layout.gap);
        ui.allocate_ui(egui::vec2(available.x, layout.timeline_height), |ui| {
            self.show_timeline(ui);
        });
    }

    fn show_timeline(&mut self, ui: &mut Ui) {
        let Some((start, end)) = self.app.effective_scene_range() else { return; };
        let clip_duration = end.saturating_sub(start);
        if clip_duration == 0 { return; }

        let mut selected = self.app.selected_second.min(clip_duration.saturating_sub(1));
        let playhead_abs = start + selected;

        ui.horizontal(|ui| {
            ui.strong("Playhead:");
            ui.label(format!("{} (abs {})", fmt_time(playhead_abs), playhead_abs));
            ui.separator();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preview_matte_is_not_black() {
        let c = preview_matte_color();
        assert_ne!(c, egui::Color32::BLACK);
        assert!(c.r() > 0 || c.g() > 0 || c.b() > 0);
    }

    #[test]
    fn test_fitted_size_preserves_aspect() {
        let container = egui::vec2(1000.0, 500.0);
        let aspect = 16.0 / 9.0;
        let fitted = fitted_size_for_aspect(container, aspect);
        let ratio = fitted.x / fitted.y;
        assert!((ratio - aspect).abs() < 0.01);
        assert!(fitted.x <= container.x + f32::EPSILON);
        assert!(fitted.y <= container.y + f32::EPSILON);
    }

    #[test]
    fn test_fitted_size_handles_tall_container() {
        let container = egui::vec2(600.0, 1200.0);
        let aspect = 16.0 / 9.0;
        let fitted = fitted_size_for_aspect(container, aspect);
        assert!((fitted.x - 600.0).abs() < 0.01);
        assert!((fitted.y - (600.0 / aspect)).abs() < 0.01);
    }

    #[test]
    fn test_compute_preview_layout_width_limited() {
        let available = egui::vec2(1400.0, 900.0);
        let aspect = 16.0 / 9.0;
        let layout = compute_preview_layout(available, aspect);

        assert!((layout.preview_size.x - 1400.0).abs() < 0.01);
        assert!((layout.preview_size.y - (1400.0 / aspect)).abs() < 0.01);
        assert!(layout.preview_size.y + layout.timeline_height + layout.gap <= available.y + 0.01);
    }

    #[test]
    fn test_compute_preview_layout_height_limited() {
        let available = egui::vec2(2200.0, 700.0);
        let aspect = 16.0 / 9.0;
        let layout = compute_preview_layout(available, aspect);

        let expected_h = available.y - TIMELINE_HEIGHT - PREVIEW_GAP;
        assert!((layout.preview_size.y - expected_h).abs() < 0.01);
        assert!((layout.preview_size.x - (expected_h * aspect)).abs() < 0.01);
    }

    #[test]
    fn test_compute_preview_layout_non_positive_aspect_fallback() {
        let available = egui::vec2(1000.0, 600.0);
        let layout = compute_preview_layout(available, 0.0);
        let expected_h = available.y - TIMELINE_HEIGHT - PREVIEW_GAP;
        assert!((layout.preview_size.x - available.x).abs() < 0.01);
        assert!((layout.preview_size.y - expected_h).abs() < 0.01);
    }
}
