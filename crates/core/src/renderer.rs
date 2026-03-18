use anyhow::{Context, Result};
use image::imageops::FilterType;
use image::{ImageBuffer, Rgba, RgbaImage};
use std::collections::HashMap;
use std::path::Path;
use chrono::{TimeZone, Utc};
use std::sync::{Arc, Mutex};

use crate::activity::Activity;
use crate::constant::{FT_CONVERSION, KMH_CONVERSION, MPH_CONVERSION};
use crate::plot::{build_plot_data, PlotCache};
use crate::template::{
    Color, LabelConfig, PlotConfig, PlotType, SceneConfig, Template, UnitSystem, ValueConfig,
    ValueType,
};

// ---------------------------------------------------------------------------
// Font cache
// ---------------------------------------------------------------------------

use fontdue::{Font, FontSettings};

pub struct FontCache {
    fonts: HashMap<String, Font>,
    font_dir: std::path::PathBuf,
}

impl FontCache {
    pub fn new(font_dir: impl AsRef<Path>) -> Self {
        Self {
            fonts: HashMap::new(),
            font_dir: font_dir.as_ref().to_owned(),
        }
    }

    pub fn get_or_load(&mut self, font_name: &str) -> &Font {
        if !self.fonts.contains_key(font_name) {
            let path = self.font_dir.join(font_name);
            let font = load_font_from_path(&path)
                .unwrap_or_else(|_| load_fallback_font());
            self.fonts.insert(font_name.to_string(), font);
        }
        self.fonts.get(font_name).unwrap()
    }
}

fn load_font_from_path(path: &Path) -> Result<Font> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("Cannot read font: {}", path.display()))?;
    Font::from_bytes(bytes, FontSettings::default())
        .map_err(|e| anyhow::anyhow!("Failed to parse font {}: {}", path.display(), e))
}

fn load_fallback_font() -> Font {
    // Embed a minimal fallback font at compile time
    static FALLBACK: &[u8] = include_bytes!("../../../fonts/Arial.ttf");
    Font::from_bytes(FALLBACK.to_vec(), FontSettings::default())
        .expect("bundled fallback font must be valid")
}

// ---------------------------------------------------------------------------
// Pre-computed render state
// ---------------------------------------------------------------------------

/// All state needed to render frames quickly.
/// Built once after GPX load + template parse; reused for every frame.
#[derive(Clone)]
pub struct RenderState {
    pub template: Template,
    pub activity: Activity,
    /// Pre-rendered static base image (labels drawn once)
    pub base_image: RgbaImage,
    /// Pre-rendered plot backgrounds (one per PlotConfig index)
    pub plot_caches: Vec<PlotCache>,
    pub font_dir: std::path::PathBuf,
    pub font_cache: Arc<Mutex<FontCache>>,
}

impl RenderState {
    /// Build render state from an Activity and Template.
    /// This is the expensive one-time setup — called on GPX load or template change.
    pub fn build(activity: Activity, template: Template, font_dir: impl AsRef<Path>) -> Result<Self> {
        let font_dir = font_dir.as_ref().to_owned();
        let w = template.scene.width;
        let h = template.scene.height;

        // Pre-render static base image (all labels)
        let mut font_cache = FontCache::new(&font_dir);
        let base_image = {
            let mut img: RgbaImage = ImageBuffer::new(w, h);
            for label in &template.labels {
                draw_label(&mut img, label, &template, &mut font_cache);
            }
            img
        };

        // Pre-render plot caches
        let mut plot_caches = Vec::with_capacity(template.plots.len());
        for plot_config in &template.plots {
            let (x_data, y_data) = build_plot_data(plot_config.value, &activity);
            let cache = PlotCache::build(plot_config, x_data, y_data, &template.scene.color)
                .with_context(|| format!("Failed to build PlotCache for {:?}", plot_config.value))?;
            plot_caches.push(cache);
        }

        Ok(RenderState {
            template,
            activity,
            base_image,
            plot_caches,
            font_cache: Arc::new(Mutex::new(FontCache::new(&font_dir))),
            font_dir,
        })
    }

    /// Render a single preview frame (full resolution).
    /// `frame_idx`: absolute frame index in the interpolated activity.
    pub fn render_frame(&self, frame_idx: usize) -> Result<RgbaImage> {
        self.render_frame_at_scale(frame_idx, 1.0)
    }

    /// Render a preview frame at a fractional scale (e.g., 0.5 for 1/2 resolution).
    /// Used for scrub preview.
    pub fn render_frame_scaled(&self, frame_idx: usize, scale: f32) -> Result<RgbaImage> {
        self.render_frame_at_scale(frame_idx, scale)
    }

    fn render_frame_at_scale(&self, frame_idx: usize, scale: f32) -> Result<RgbaImage> {
        let mut img = self.base_image.clone();
        let mut font_cache = self
            .font_cache
            .lock()
            .map_err(|_| anyhow::anyhow!("Font cache mutex poisoned"))?;

        // Draw values (dynamic telemetry text)
        for value_config in &self.template.values {
            let text = format_value(value_config, &self.activity, frame_idx, &self.template.scene);
            draw_text(
                &mut img,
                &text,
                value_config.x,
                value_config.y,
                self.template.value_font(value_config),
                self.template.value_font_size(value_config),
                self.template.value_color(value_config),
                self.template.value_opacity(value_config),
                &mut font_cache,
            );
        }

        // Draw plots with position markers
        for (i, plot_config) in self.template.plots.iter().enumerate() {
            let cache = &self.plot_caches[i];
            let (frame_x, frame_y) = current_plot_position(plot_config, &self.activity, frame_idx);
            let plot_pixmap = cache.render_frame(plot_config, frame_x, frame_y, &self.template.scene.color)?;

            // Rotate if needed
            let plot_img = pixmap_to_rgba_image(&plot_pixmap);
            let plot_img = if plot_config.rotation.abs() > 0.01 {
                rotate_image(&plot_img, plot_config.rotation)
            } else {
                plot_img
            };

            composite_onto(&mut img, &plot_img, plot_config.x, plot_config.y);
        }

        if (scale - 1.0).abs() < f32::EPSILON {
            Ok(img)
        } else {
            let s = scale.clamp(0.1, 1.0);
            let new_w = ((img.width() as f32) * s).round().max(1.0) as u32;
            let new_h = ((img.height() as f32) * s).round().max(1.0) as u32;
            Ok(image::imageops::resize(&img, new_w, new_h, FilterType::Triangle))
        }
    }
}

// ---------------------------------------------------------------------------
// Renderer — thin wrapper for use in the encoder loop
// ---------------------------------------------------------------------------

pub struct Renderer {
    state: RenderState,
}

impl Renderer {
    pub fn new(state: RenderState) -> Self {
        Self { state }
    }

    pub fn render_frame(&mut self, frame_idx: usize) -> Result<RgbaImage> {
        self.state.render_frame(frame_idx)
    }

    pub fn total_frames(&self) -> u32 {
        self.state.template.scene.total_frames()
    }

    pub fn fps(&self) -> u32 {
        self.state.template.scene.fps
    }

    pub fn width(&self) -> u32 {
        self.state.template.scene.width
    }

    pub fn height(&self) -> u32 {
        self.state.template.scene.height
    }

    /// Consume the Renderer and return the inner RenderState.
    /// Used by the encoder to share state across rayon threads via Arc.
    pub fn into_state(self) -> RenderState {
        self.state
    }
}

// ---------------------------------------------------------------------------
// Value formatting
// ---------------------------------------------------------------------------

fn format_value(
    config: &ValueConfig,
    activity: &Activity,
    frame_idx: usize,
    scene: &SceneConfig,
) -> String {
    use ValueType::*;
    let raw = activity.value_at(config.value, frame_idx);
    let decimal = config.decimal_rounding
        .or(scene.decimal_rounding)
        .unwrap_or(0) as usize;

    let converted = match (config.value, config.unit) {
        (Speed, Some(UnitSystem::Imperial)) => raw * MPH_CONVERSION,
        (Speed, Some(UnitSystem::Metric))   => raw * KMH_CONVERSION,
        (Speed, None)                        => raw * MPH_CONVERSION, // default imperial
        (Elevation, Some(UnitSystem::Imperial)) => raw * FT_CONVERSION,
        (Elevation, None)                        => raw * FT_CONVERSION, // default imperial
        (Distance, Some(UnitSystem::Imperial)) => raw * 0.000621371, // m → miles
        (Distance, Some(UnitSystem::Metric))   => raw / 1000.0,      // m → km
        (Time, _) => {
            // Format as time string
            let hours_offset = config.hours_offset.unwrap_or(0.0);
            let fmt = config.time_format.as_deref().unwrap_or("%H:%M:%S");
            let ts = raw as i64;
            let dt = Utc.timestamp_opt(ts, 0).single().unwrap_or(Utc::now());
            let dt = dt + chrono::Duration::seconds((hours_offset * 3600.0) as i64);
            let text = dt.format(fmt).to_string();
            return append_suffix(text, config);
        }
        _ => raw,
    };

    let text = if decimal == 0 {
        format!("{:.0}", converted)
    } else {
        format!("{:.prec$}", converted, prec = decimal)
    };
    append_suffix(text, config)
}

fn append_suffix(text: String, config: &ValueConfig) -> String {
    if let Some(suffix) = &config.suffix {
        format!("{}{}", text, suffix)
    } else {
        text
    }
}

// ---------------------------------------------------------------------------
// Plot position helpers
// ---------------------------------------------------------------------------

fn current_plot_position(
    config: &PlotConfig,
    activity: &Activity,
    frame_idx: usize,
) -> (f64, f64) {
    match config.value {
        PlotType::Course => {
            let (lat, lon) = activity.course.get(frame_idx).copied().unwrap_or((0.0, 0.0));
            (lon, lat) // x=lon, y=lat
        }
        PlotType::Elevation => {
            let elev = activity.elevation.get(frame_idx).copied().unwrap_or(0.0);
            (frame_idx as f64, elev)
        }
    }
}

// ---------------------------------------------------------------------------
// Text rendering (fontdue)
// ---------------------------------------------------------------------------

fn draw_label(
    img: &mut RgbaImage,
    label: &LabelConfig,
    template: &Template,
    font_cache: &mut FontCache,
) {
    draw_text(
        img,
        &label.text,
        label.x,
        label.y,
        template.label_font(label),
        template.label_font_size(label),
        template.label_color(label),
        template.label_opacity(label),
        font_cache,
    );
}

fn draw_text(
    img: &mut RgbaImage,
    text: &str,
    x: i32,
    y: i32,
    font_name: &str,
    font_size: f32,
    color: &Color,
    opacity: f32,
    font_cache: &mut FontCache,
) {
    let font = font_cache.get_or_load(font_name);
    let [r, g, b, a] = color.to_rgba();
    let color_alpha = (a as f32 / 255.0) * opacity.clamp(0.0, 1.0);

    let mut cursor_x = x;
    for ch in text.chars() {
        let (metrics, bitmap) = font.rasterize(ch, font_size);
        let glyph_x = cursor_x + metrics.xmin;
        let glyph_y = y - metrics.height as i32 - metrics.ymin;

        for row in 0..metrics.height {
            for col in 0..metrics.width {
                let alpha = bitmap[row * metrics.width + col];
                if alpha == 0 { continue; }

                let px = glyph_x + col as i32;
                let py = glyph_y + row as i32;

                if px < 0 || py < 0 || px >= img.width() as i32 || py >= img.height() as i32 {
                    continue;
                }

                let pixel = img.get_pixel_mut(px as u32, py as u32);
                let src_a = (alpha as f32 / 255.0) * color_alpha;
                let dst_a = pixel[3] as f32 / 255.0;
                // Alpha compositing: src over dst
                let out_a = src_a + dst_a * (1.0 - src_a);
                if out_a > 0.0 {
                    pixel[0] = ((r as f32 * src_a + pixel[0] as f32 * dst_a * (1.0 - src_a)) / out_a) as u8;
                    pixel[1] = ((g as f32 * src_a + pixel[1] as f32 * dst_a * (1.0 - src_a)) / out_a) as u8;
                    pixel[2] = ((b as f32 * src_a + pixel[2] as f32 * dst_a * (1.0 - src_a)) / out_a) as u8;
                    pixel[3] = (out_a * 255.0) as u8;
                }
            }
        }
        cursor_x += metrics.advance_width as i32;
    }
}

// ---------------------------------------------------------------------------
// Compositing helpers
// ---------------------------------------------------------------------------

/// Alpha-composite `src` onto `dst` at position (x, y).
pub fn composite_onto(dst: &mut RgbaImage, src: &RgbaImage, x: i32, y: i32) {
    let dst_w = dst.width() as i32;
    let dst_h = dst.height() as i32;
    let src_w = src.width() as i32;
    let src_h = src.height() as i32;

    for sy in 0..src_h {
        for sx in 0..src_w {
            let dx = x + sx;
            let dy = y + sy;
            if dx < 0 || dy < 0 || dx >= dst_w || dy >= dst_h { continue; }
            let src_px = src.get_pixel(sx as u32, sy as u32);
            if src_px[3] == 0 { continue; }
            let dst_px = dst.get_pixel_mut(dx as u32, dy as u32);

            let sa = src_px[3] as f32 / 255.0;
            let da = dst_px[3] as f32 / 255.0;
            let out_a = sa + da * (1.0 - sa);
            if out_a > 0.0 {
                dst_px[0] = ((src_px[0] as f32 * sa + dst_px[0] as f32 * da * (1.0 - sa)) / out_a) as u8;
                dst_px[1] = ((src_px[1] as f32 * sa + dst_px[1] as f32 * da * (1.0 - sa)) / out_a) as u8;
                dst_px[2] = ((src_px[2] as f32 * sa + dst_px[2] as f32 * da * (1.0 - sa)) / out_a) as u8;
                dst_px[3] = (out_a * 255.0) as u8;
            }
        }
    }
}

/// Convert a tiny-skia Pixmap to an `image::RgbaImage`.
pub fn pixmap_to_rgba_image(pixmap: &tiny_skia::Pixmap) -> RgbaImage {
    let w = pixmap.width();
    let h = pixmap.height();
    // tiny-skia stores pixels as pre-multiplied RGBA; convert to straight alpha
    let data = pixmap.data();
    let mut out = vec![0u8; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        let r = data[i * 4];
        let g = data[i * 4 + 1];
        let b = data[i * 4 + 2];
        let a = data[i * 4 + 3];
        if a > 0 {
            out[i * 4]     = ((r as u16 * 255) / a as u16).min(255) as u8;
            out[i * 4 + 1] = ((g as u16 * 255) / a as u16).min(255) as u8;
            out[i * 4 + 2] = ((b as u16 * 255) / a as u16).min(255) as u8;
        }
        out[i * 4 + 3] = a;
    }
    ImageBuffer::from_raw(w, h, out).expect("data length must match w*h*4")
}

/// Rotate an RgbaImage by `degrees` clockwise using bilinear interpolation.
pub fn rotate_image(img: &RgbaImage, degrees: f32) -> RgbaImage {
    use imageproc::geometric_transformations::{rotate_about_center, Interpolation};
    let angle = degrees.to_radians();
    rotate_about_center(img, angle, Interpolation::Bilinear, Rgba([0, 0, 0, 0]))
}
