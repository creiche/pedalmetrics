use serde::{Deserialize, Serialize};
use crate::constant::{
    DEFAULT_COLOR, DEFAULT_DPI, DEFAULT_FONT_SIZE, DEFAULT_FPS, DEFAULT_LINE_WIDTH,
    DEFAULT_MARGIN, DEFAULT_OPACITY, DEFAULT_OVERLAY_FILENAME, DEFAULT_POINT_WEIGHT,
};

// ---------------------------------------------------------------------------
// Value type enum — what telemetry channel to display
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Speed,
    Power,
    HeartRate,
    Cadence,
    Gradient,
    Elevation,
    Distance,
    Time,
    Temperature,
}

impl ValueType {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Speed => "Speed",
            Self::Power => "Power",
            Self::HeartRate => "Heart Rate",
            Self::Cadence => "Cadence",
            Self::Gradient => "Gradient",
            Self::Elevation => "Elevation",
            Self::Distance => "Distance",
            Self::Time => "Time",
            Self::Temperature => "Temperature",
        }
    }
}

// ---------------------------------------------------------------------------
// Unit system
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnitSystem {
    Imperial,
    Metric,
}

// ---------------------------------------------------------------------------
// Plot type enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlotType {
    Course,
    Elevation,
}

// ---------------------------------------------------------------------------
// Color — stored as "#rrggbb" or "#rrggbbaa"
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Color(pub String);

impl Default for Color {
    fn default() -> Self {
        Self(DEFAULT_COLOR.to_string())
    }
}

impl Color {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Parse to [r, g, b, a] components in 0..=255.
    pub fn to_rgba(&self) -> [u8; 4] {
        let s = self.0.trim_start_matches('#');
        match s.len() {
            6 => {
                let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(255);
                let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(255);
                let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(255);
                [r, g, b, 255]
            }
            8 => {
                let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(255);
                let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(255);
                let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(255);
                let a = u8::from_str_radix(&s[6..8], 16).unwrap_or(255);
                [r, g, b, a]
            }
            _ => [255, 255, 255, 255],
        }
    }

    /// Apply an opacity multiplier (0.0–1.0) to produce [r, g, b, a].
    pub fn to_rgba_with_opacity(&self, opacity: f32) -> [u8; 4] {
        let mut rgba = self.to_rgba();
        rgba[3] = (rgba[3] as f32 * opacity.clamp(0.0, 1.0)) as u8;
        rgba
    }
}

// ---------------------------------------------------------------------------
// Scene settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneConfig {
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_fps")]
    pub fps: u32,
    pub start: u32,
    pub end: u32,
    #[serde(default = "default_font")]
    pub font: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default)]
    pub color: Color,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub decimal_rounding: Option<u8>,
    #[serde(default = "default_overlay_filename")]
    pub overlay_filename: String,
}

fn default_fps() -> u32 { DEFAULT_FPS }
fn default_font() -> String { "Arial.ttf".to_string() }
fn default_font_size() -> f32 { DEFAULT_FONT_SIZE }
fn default_opacity() -> f32 { DEFAULT_OPACITY }
fn default_overlay_filename() -> String { DEFAULT_OVERLAY_FILENAME.to_string() }

impl SceneConfig {
    pub fn duration_seconds(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub fn total_frames(&self) -> u32 {
        self.duration_seconds() * self.fps
    }
}

// ---------------------------------------------------------------------------
// Label element — static text
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelConfig {
    pub text: String,
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub font: Option<String>,
    #[serde(default)]
    pub font_size: Option<f32>,
    #[serde(default)]
    pub color: Option<Color>,
    #[serde(default)]
    pub opacity: Option<f32>,
}

// ---------------------------------------------------------------------------
// Value element — dynamic telemetry value
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueConfig {
    pub value: ValueType,
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub unit: Option<UnitSystem>,
    #[serde(default)]
    pub font: Option<String>,
    #[serde(default)]
    pub font_size: Option<f32>,
    #[serde(default)]
    pub color: Option<Color>,
    #[serde(default)]
    pub opacity: Option<f32>,
    #[serde(default)]
    pub suffix: Option<String>,
    #[serde(default)]
    pub decimal_rounding: Option<u8>,
    /// Offset in hours to add to GPS time when displaying
    #[serde(default)]
    pub hours_offset: Option<f64>,
    /// strftime format string for time display, e.g. "%H:%M:%S"
    #[serde(default)]
    pub time_format: Option<String>,
}

// ---------------------------------------------------------------------------
// Point marker style on plots
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointStyle {
    #[serde(default)]
    pub color: Option<Color>,
    #[serde(default = "default_point_weight")]
    pub weight: f32,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub edge_color: Option<Color>,
}

fn default_point_weight() -> f32 { DEFAULT_POINT_WEIGHT }

impl Default for PointStyle {
    fn default() -> Self {
        Self {
            color: None,
            weight: DEFAULT_POINT_WEIGHT,
            opacity: DEFAULT_OPACITY,
            edge_color: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Plot point label (text drawn near position markers)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointLabelConfig {
    #[serde(default)]
    pub x_offset: f32,
    #[serde(default)]
    pub y_offset: f32,
    #[serde(default)]
    pub font: Option<String>,
    #[serde(default)]
    pub font_size: Option<f32>,
    #[serde(default)]
    pub color: Option<Color>,
    /// Unit systems to show in the label (e.g. ["imperial", "metric"])
    #[serde(default)]
    pub units: Vec<UnitSystem>,
}

// ---------------------------------------------------------------------------
// Line style for plots
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineStyle {
    #[serde(default = "default_line_width")]
    pub width: f32,
    #[serde(default)]
    pub color: Option<Color>,
}

fn default_line_width() -> f32 { DEFAULT_LINE_WIDTH }

impl Default for LineStyle {
    fn default() -> Self {
        Self { width: DEFAULT_LINE_WIDTH, color: None }
    }
}

// ---------------------------------------------------------------------------
// Fill style for area-under-curve
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillStyle {
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub color: Option<Color>,
}

// ---------------------------------------------------------------------------
// Plot element — course map or elevation profile
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlotConfig {
    pub value: PlotType,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub color: Option<Color>,
    #[serde(default)]
    pub opacity: Option<f32>,
    #[serde(default = "default_margin")]
    pub margin: f64,
    #[serde(default = "default_dpi")]
    pub dpi: u32,
    #[serde(default)]
    pub line: LineStyle,
    #[serde(default)]
    pub fill: Option<FillStyle>,
    /// Rotation in degrees (clockwise)
    #[serde(default)]
    pub rotation: f32,
    /// Position marker(s) to draw at current position
    #[serde(default)]
    pub points: Vec<PointStyle>,
    /// Optional text label drawn near the position marker
    #[serde(default)]
    pub point_label: Option<PointLabelConfig>,
}

fn default_margin() -> f64 { DEFAULT_MARGIN }
fn default_dpi() -> u32 { DEFAULT_DPI }

// ---------------------------------------------------------------------------
// Top-level Template
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub scene: SceneConfig,
    #[serde(default)]
    pub labels: Vec<LabelConfig>,
    #[serde(default)]
    pub values: Vec<ValueConfig>,
    #[serde(default)]
    pub plots: Vec<PlotConfig>,
}

impl Template {
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(json)?)
    }

    pub fn to_json_pretty(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Effective font for a label (falls back to scene default).
    pub fn label_font<'a>(&'a self, label: &'a LabelConfig) -> &'a str {
        label.font.as_deref().unwrap_or(&self.scene.font)
    }

    /// Effective font size for a label.
    pub fn label_font_size(&self, label: &LabelConfig) -> f32 {
        label.font_size.unwrap_or(self.scene.font_size)
    }

    /// Effective color for a label.
    pub fn label_color<'a>(&'a self, label: &'a LabelConfig) -> &'a Color {
        label.color.as_ref().unwrap_or(&self.scene.color)
    }

    /// Effective opacity for a label.
    pub fn label_opacity(&self, label: &LabelConfig) -> f32 {
        label.opacity.unwrap_or(self.scene.opacity)
    }

    /// Effective font for a value element.
    pub fn value_font<'a>(&'a self, value: &'a ValueConfig) -> &'a str {
        value.font.as_deref().unwrap_or(&self.scene.font)
    }

    /// Effective font size for a value element.
    pub fn value_font_size(&self, value: &ValueConfig) -> f32 {
        value.font_size.unwrap_or(self.scene.font_size)
    }

    /// Effective color for a value element.
    pub fn value_color<'a>(&'a self, value: &'a ValueConfig) -> &'a Color {
        value.color.as_ref().unwrap_or(&self.scene.color)
    }

    /// Effective opacity for a value element.
    pub fn value_opacity(&self, value: &ValueConfig) -> f32 {
        value.opacity.unwrap_or(self.scene.opacity)
    }

    /// Effective color for a plot element.
    pub fn plot_color<'a>(&'a self, plot: &'a PlotConfig) -> &'a Color {
        plot.color.as_ref().unwrap_or(&self.scene.color)
    }

    /// Effective opacity for a plot element.
    pub fn plot_opacity(&self, plot: &PlotConfig) -> f32 {
        plot.opacity.unwrap_or(self.scene.opacity)
    }
}

// ---------------------------------------------------------------------------
// Built-in template presets
// ---------------------------------------------------------------------------

impl Template {
    /// A minimal default template for 4K 30fps.
    pub fn default_4k() -> Self {
        Self {
            scene: SceneConfig {
                width: 3840,
                height: 2160,
                fps: 30,
                start: 0,
                end: 60,
                font: "Arial.ttf".to_string(),
                font_size: 30.0,
                color: Color::default(),
                opacity: 1.0,
                decimal_rounding: Some(0),
                overlay_filename: DEFAULT_OVERLAY_FILENAME.to_string(),
            },
            labels: vec![],
            values: vec![],
            plots: vec![],
        }
    }
}
