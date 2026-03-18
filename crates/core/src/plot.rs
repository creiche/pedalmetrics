use anyhow::Result;
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap,
    Stroke, Transform,
};

use crate::template::{Color as TemplateColor, PlotConfig, PlotType, PointStyle};

// ---------------------------------------------------------------------------
// Coordinate mapping
// ---------------------------------------------------------------------------

/// Map a data value to a pixel coordinate within [margin, size - margin].
fn map_value(val: f64, min: f64, max: f64, size: u32, margin: f64) -> f32 {
    if (max - min).abs() < 1e-10 {
        return size as f32 / 2.0;
    }
    let margin_px = size as f64 * margin;
    let range_px = size as f64 - 2.0 * margin_px;
    (margin_px + (val - min) / (max - min) * range_px) as f32
}

// ---------------------------------------------------------------------------
// PlotCache — pre-rendered static plot background
// ---------------------------------------------------------------------------

/// A pre-rendered plot background. Computed once per activity load or template
/// change. Rendering a frame just pastes this and draws the position dot on top.
#[derive(Clone)]
pub struct PlotCache {
    /// The pre-rendered static polyline + optional fill area
    pub background: Pixmap,
    pub plot_type: PlotType,
    pub config_width: u32,
    pub config_height: u32,
    // Data extents for coordinate mapping
    pub x_min: f64,
    pub x_max: f64,
    pub y_min: f64,
    pub y_max: f64,
    /// The full data series (used to draw position markers)
    pub x_data: Vec<f64>,
    pub y_data: Vec<f64>,
}

impl PlotCache {
    /// Build a PlotCache for an elevation profile or course map.
    ///
    /// For **elevation**: `x_data = frame indices`, `y_data = elevation values (m)`
    /// For **course**: `x_data = longitudes`, `y_data = latitudes`
    pub fn build(
        config: &PlotConfig,
        x_data: Vec<f64>,
        y_data: Vec<f64>,
        scene_color: &TemplateColor,
    ) -> Result<Self> {
        anyhow::ensure!(!x_data.is_empty(), "PlotCache: empty x_data");
        anyhow::ensure!(x_data.len() == y_data.len(), "PlotCache: x_data and y_data length mismatch");

        let w = config.width;
        let h = config.height;
        let margin = config.margin;

        let x_min = x_data.iter().cloned().fold(f64::INFINITY, f64::min);
        let x_max = x_data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let y_min = y_data.iter().cloned().fold(f64::INFINITY, f64::min);
        let y_max = y_data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        let mut pixmap = Pixmap::new(w, h)
            .ok_or_else(|| anyhow::anyhow!("Failed to create Pixmap {}x{}", w, h))?;

        // Transparent background
        pixmap.fill(Color::TRANSPARENT);

        let line_color = config.line.color.as_ref().unwrap_or(scene_color);
        let line_alpha = config.opacity.unwrap_or(1.0);
        let line_rgba_w_alpha = line_color.to_rgba_with_opacity(line_alpha);

        // Map data points to pixels
        // For course: we flip y (latitude increases upward, pixels increase downward)
        let pixels: Vec<(f32, f32)> = x_data.iter().zip(&y_data).map(|(&x, &y)| {
            let px = map_value(x, x_min, x_max, w, margin);
            let py = match config.value {
                PlotType::Course => {
                    // Flip y-axis: higher latitude = higher on screen
                    map_value(y_max - (y - y_min), 0.0, y_max - y_min, h, margin)
                }
                PlotType::Elevation => {
                    // Flip y-axis: higher elevation = higher on screen
                    h as f32 - map_value(y, y_min, y_max, h, margin)
                }
            };
            (px, py)
        }).collect();

        // Draw fill area (elevation only)
        if let Some(fill) = &config.fill {
            if config.value == PlotType::Elevation {
                let mut pb = PathBuilder::new();
                let baseline_y = h as f32 - map_value(y_min * 0.99, y_min, y_max, h, margin);
                if let Some(&(x0, y0)) = pixels.first() {
                    pb.move_to(x0, baseline_y);
                    pb.line_to(x0, y0);
                    for &(px, py) in pixels.iter().skip(1) {
                        pb.line_to(px, py);
                    }
                    if let Some(&(xlast, _)) = pixels.last() {
                        pb.line_to(xlast, baseline_y);
                    }
                    pb.close();
                    if let Some(path) = pb.finish() {
                        let fill_color = fill
                            .color
                            .as_ref()
                            .or(config.color.as_ref())
                            .unwrap_or(scene_color);
                        let [r, g, b, _] = fill_color.to_rgba();
                        let alpha = (fill.opacity * 255.0) as u8;
                        let mut paint = Paint::default();
                        paint.set_color_rgba8(r, g, b, alpha);
                        pixmap.fill_path(
                            &path,
                            &paint,
                            FillRule::Winding,
                            Transform::identity(),
                            None,
                        );
                    }
                }
            }
        }

        // Draw the polyline
        if pixels.len() >= 2 {
            let mut pb = PathBuilder::new();
            let (x0, y0) = pixels[0];
            pb.move_to(x0, y0);
            for &(px, py) in pixels.iter().skip(1) {
                pb.line_to(px, py);
            }
            if let Some(path) = pb.finish() {
                let [r, g, b, a] = line_rgba_w_alpha;
                let mut paint = Paint::default();
                paint.set_color_rgba8(r, g, b, a);
                let mut stroke = Stroke::default();
                stroke.width = config.line.width;
                stroke.line_cap = LineCap::Round;
                stroke.line_join = LineJoin::Round;
                pixmap.stroke_path(
                    &path,
                    &paint,
                    &stroke,
                    Transform::identity(),
                    None,
                );
            }
        }

        Ok(PlotCache {
            background: pixmap,
            plot_type: config.value,
            config_width: w,
            config_height: h,
            x_min, x_max, y_min, y_max,
            x_data,
            y_data,
        })
    }

    /// Render a single frame: return a Pixmap with the static background plus
    /// position markers drawn at the current data index.
    ///
    /// `frame_x`: for elevation: frame_idx as f64; for course: current longitude
    /// `frame_y`: for elevation: current elevation (m); for course: current latitude
    pub fn render_frame(
        &self,
        config: &PlotConfig,
        frame_x: f64,
        frame_y: f64,
        scene_color: &TemplateColor,
    ) -> Result<Pixmap> {
        let mut pixmap = self.background.clone();

        let px = map_value(frame_x, self.x_min, self.x_max, self.config_width, config.margin);
        let py = match config.value {
            PlotType::Course => {
                map_value(self.y_max - (frame_y - self.y_min), 0.0, self.y_max - self.y_min, self.config_height, config.margin)
            }
            PlotType::Elevation => {
                self.config_height as f32 - map_value(frame_y, self.y_min, self.y_max, self.config_height, config.margin)
            }
        };

        // Draw position markers
        let points = if config.points.is_empty() {
            vec![PointStyle::default()]
        } else {
            config.points.clone()
        };

        for point in &points {
            let dot_color = point.color.as_ref().unwrap_or(scene_color);
            let [r, g, b, _] = dot_color.to_rgba();
            let alpha = (point.opacity * 255.0) as u8;
            let radius = (point.weight / 10.0).sqrt().max(1.0);

            let mut pb = PathBuilder::new();
            pb.push_circle(px, py, radius);
            if let Some(path) = pb.finish() {
                let mut paint = Paint::default();
                paint.set_color_rgba8(r, g, b, alpha);
                pixmap.fill_path(
                    &path,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }

        Ok(pixmap)
    }
}

// ---------------------------------------------------------------------------
// Build plot data arrays from Activity
// ---------------------------------------------------------------------------

use crate::activity::Activity;

/// Build (x_data, y_data) arrays for a plot from the activity.
/// For course: x = longitude, y = latitude.
/// For elevation: x = frame index (0..total_frames), y = elevation (m).
pub fn build_plot_data(
    plot_type: PlotType,
    activity: &Activity,
) -> (Vec<f64>, Vec<f64>) {
    match plot_type {
        PlotType::Course => {
            let x: Vec<f64> = activity.course.iter().map(|(_, lon)| *lon).collect();
            let y: Vec<f64> = activity.course.iter().map(|(lat, _)| *lat).collect();
            (x, y)
        }
        PlotType::Elevation => {
            let x: Vec<f64> = (0..activity.elevation.len()).map(|i| i as f64).collect();
            let y = activity.elevation.clone();
            (x, y)
        }
    }
}
