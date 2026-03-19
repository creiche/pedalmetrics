use anyhow::Result;
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap,
    Stroke, Transform,
};

use crate::template::{Color as TemplateColor, PlotConfig, PlotType, PointStyle};

const MIN_MARKER_RADIUS_PX: f32 = 0.5;

// ---------------------------------------------------------------------------
// Coordinate mapping
// ---------------------------------------------------------------------------

/// Map a data value to a pixel coordinate within [0, size].
fn map_value(val: f64, min: f64, max: f64, size: u32) -> f32 {
    map_value_with_padding(val, min, max, size, 0.0)
}

/// Map a data value to a pixel coordinate within [padding_px, size - padding_px].
fn map_value_with_padding(val: f64, min: f64, max: f64, size: u32, padding_px: f64) -> f32 {
    if (max - min).abs() < 1e-10 {
        return size as f32 / 2.0;
    }
    let max_padding = (size as f64 / 2.0 - 1.0).max(0.0);
    let pad = padding_px.clamp(0.0, max_padding);
    let drawable = (size as f64 - 2.0 * pad).max(1.0);
    (pad + (val - min) / (max - min) * drawable) as f32
}

fn marker_radius_px(point: &PointStyle) -> f64 {
    point.radius.max(MIN_MARKER_RADIUS_PX) as f64
}

fn course_padding_px(config: &PlotConfig) -> f64 {
    if config.value != PlotType::Course {
        return 0.0;
    }
    let marker_pad = if config.points.is_empty() {
        marker_radius_px(&PointStyle::default())
    } else {
        config
            .points
            .iter()
            .map(marker_radius_px)
            .fold(0.0_f64, f64::max)
    };
    let stroke_pad = (config.line.width as f64 / 2.0).max(1.0);
    marker_pad.max(stroke_pad) + 1.0
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

        let x_min = x_data.iter().cloned().fold(f64::INFINITY, f64::min);
        let x_max = x_data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let y_min = y_data.iter().cloned().fold(f64::INFINITY, f64::min);
        let y_max = y_data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let course_pad = course_padding_px(config);

        let mut pixmap = Pixmap::new(w, h)
            .ok_or_else(|| anyhow::anyhow!("Failed to create Pixmap {}x{}", w, h))?;

        // Transparent background
        pixmap.fill(Color::TRANSPARENT);

        let line_color = config
            .line
            .color
            .as_ref()
            .or(config.color.as_ref())
            .unwrap_or(scene_color);
        let line_alpha = config.opacity.unwrap_or(1.0);
        let line_rgba_w_alpha = line_color.to_rgba_with_opacity(line_alpha);

        // Map data points to pixels
        // For course: we flip y (latitude increases upward, pixels increase downward)
        let pixels: Vec<(f32, f32)> = x_data.iter().zip(&y_data).map(|(&x, &y)| {
            let px = map_value_with_padding(x, x_min, x_max, w, course_pad);
            let py = match config.value {
                PlotType::Course => {
                    // Flip y-axis: higher latitude = higher on screen
                    h as f32 - map_value_with_padding(y, y_min, y_max, h, course_pad)
                }
                PlotType::Elevation => {
                    // Flip y-axis: higher elevation = higher on screen
                    h as f32 - map_value(y, y_min, y_max, h)
                }
            };
            (px, py)
        }).collect();

        // Draw fill area (elevation only)
        if let Some(fill) = &config.fill {
            if config.value == PlotType::Elevation {
                let mut pb = PathBuilder::new();
                let baseline_y = h as f32 - map_value(y_min * 0.99, y_min, y_max, h);
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
        let course_pad = course_padding_px(config);

        let px = map_value_with_padding(frame_x, self.x_min, self.x_max, self.config_width, course_pad);
        let py = match config.value {
            PlotType::Course => {
                self.config_height as f32
                    - map_value_with_padding(frame_y, self.y_min, self.y_max, self.config_height, course_pad)
            }
            PlotType::Elevation => {
                self.config_height as f32 - map_value(frame_y, self.y_min, self.y_max, self.config_height)
            }
        };

        // Draw position markers
        let points = if config.points.is_empty() {
            vec![PointStyle::default()]
        } else {
            config.points.clone()
        };

        for point in &points {
            let dot_color = point
                .color
                .as_ref()
                .or(config.color.as_ref())
                .unwrap_or(scene_color);
            let [r, g, b, _] = dot_color.to_rgba();
            let alpha = (point.opacity * 255.0) as u8;
            // Keep markers visible at high resolutions while preserving weight scaling.
            let radius = point.radius.max(MIN_MARKER_RADIUS_PX);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::{LineStyle, PlotConfig};

    fn course_plot_config() -> PlotConfig {
        PlotConfig {
            value: PlotType::Course,
            x: 0,
            y: 0,
            width: 240,
            height: 120,
            color: Some(TemplateColor::new("#ff0000")),
            opacity: Some(1.0),
            dpi: 72,
            line: LineStyle {
                width: 2.0,
                color: None,
            },
            fill: None,
            rotation: 0.0,
            points: vec![],
            point_label: None,
        }
    }

    #[test]
    fn test_plot_color_fallback_applies_to_course_line() {
        let mut red_cfg = course_plot_config();
        red_cfg.color = Some(TemplateColor::new("#ff0000"));
        let mut green_cfg = course_plot_config();
        green_cfg.color = Some(TemplateColor::new("#00ff00"));

        let x_data = vec![-82.10, -82.09, -82.08, -82.07];
        let y_data = vec![29.10, 29.11, 29.115, 29.12];
        let scene_color = TemplateColor::new("#0000ff");

        let red_cache = PlotCache::build(&red_cfg, x_data.clone(), y_data.clone(), &scene_color)
            .expect("red plot cache should build");
        let green_cache = PlotCache::build(&green_cfg, x_data, y_data, &scene_color)
            .expect("green plot cache should build");

        assert!(
            red_cache.background.data() != green_cache.background.data(),
            "expected plot.color to influence line rendering when line.color is unset"
        );
    }

    #[test]
    fn test_plotcache_build_empty_x_data_error() {
        let cfg = course_plot_config();
        let scene_color = TemplateColor::new("#0000ff");
        let result = PlotCache::build(&cfg, vec![], vec![], &scene_color);
        assert!(result.is_err(), "Should error on empty x_data");
    }

    #[test]
    fn test_plotcache_build_mismatched_lengths_error() {
        let cfg = course_plot_config();
        let scene_color = TemplateColor::new("#0000ff");
        let result = PlotCache::build(&cfg, vec![1.0, 2.0], vec![1.0], &scene_color);
        assert!(result.is_err(), "Should error on mismatched x/y lengths");
    }

    #[test]
    fn test_plotcache_build_with_fill_elevation() {
        let mut cfg = course_plot_config();
        cfg.value = PlotType::Elevation;
        cfg.fill = Some(crate::template::FillStyle {
            color: Some(TemplateColor::new("#00ff00")),
            opacity: 0.5,
        });
        let x_data = vec![0.0, 1.0, 2.0, 3.0];
        let y_data = vec![10.0, 12.0, 11.0, 13.0];
        let scene_color = TemplateColor::new("#0000ff");
        let cache = PlotCache::build(&cfg, x_data, y_data, &scene_color)
            .expect("Should build elevation plot with fill");
        // Just check that the background is not empty
        assert!(cache.background.width() > 0 && cache.background.height() > 0);
    }

    #[test]
    fn test_render_frame_with_custom_point_style() {
        let mut cfg = course_plot_config();
        cfg.points = vec![crate::template::PointStyle {
            radius: 5.0,
            color: Some(TemplateColor::new("#123456")),
            edge_color: None,
            opacity: 0.8,
        }];
        let x_data = vec![-82.10, -82.09, -82.08, -82.07];
        let y_data = vec![29.10, 29.11, 29.115, 29.12];
        let scene_color = TemplateColor::new("#0000ff");
        let cache = PlotCache::build(&cfg, x_data, y_data, &scene_color)
            .expect("Should build plot with custom point style");
        let pixmap = cache.render_frame(&cfg, -82.09, 29.11, &scene_color)
            .expect("Should render frame with custom marker");
        assert!(pixmap.width() > 0 && pixmap.height() > 0);
    }

    #[test]
    fn test_build_plot_data_course_and_elevation() {
        // Mock Activity
                use crate::activity::Activity;
                let gpx = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="test">
    <trk><name>test</name><trkseg>
        <trkpt lat="1.0" lon="2.0"><ele>10.0</ele><time>2020-01-01T00:00:00Z</time></trkpt>
        <trkpt lat="3.0" lon="4.0"><ele>20.0</ele><time>2020-01-01T00:00:01Z</time></trkpt>
        <trkpt lat="5.0" lon="6.0"><ele>30.0</ele><time>2020-01-01T00:00:02Z</time></trkpt>
    </trkseg></trk>
</gpx>"#;
                let act = Activity::from_str(gpx).expect("parse minimal gpx");
                let (x, y) = build_plot_data(PlotType::Course, &act);
                assert_eq!(x, vec![2.0, 4.0, 6.0]);
                assert_eq!(y, vec![1.0, 3.0, 5.0]);
                let (x, y) = build_plot_data(PlotType::Elevation, &act);
                assert_eq!(x, vec![0.0, 1.0, 2.0]);
                assert_eq!(y, vec![10.0, 20.0, 30.0]);
    }
}
