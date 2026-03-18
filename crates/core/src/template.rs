use serde::{Deserialize, Serialize};
use serde::de;
use serde::ser::SerializeMap;
use std::collections::HashMap;
use crate::constant::{
    DEFAULT_COLOR, DEFAULT_DPI, DEFAULT_FONT_SIZE, DEFAULT_FPS, DEFAULT_LINE_WIDTH,
    DEFAULT_OPACITY, DEFAULT_OVERLAY_FILENAME, DEFAULT_POINT_RADIUS,
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
    Timecode,
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
            Self::Timecode => "Timecode",
            Self::Temperature => "Temperature",
        }
    }

    pub fn key(&self) -> &'static str {
        match self {
            Self::Speed => "speed",
            Self::Power => "power",
            Self::HeartRate => "heart_rate",
            Self::Cadence => "cadence",
            Self::Gradient => "gradient",
            Self::Elevation => "elevation",
            Self::Distance => "distance",
            Self::Time => "time",
            Self::Timecode => "timecode",
            Self::Temperature => "temperature",
        }
    }

}

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

impl PlotType {
    pub fn key(&self) -> &'static str {
        match self {
            Self::Course => "course",
            Self::Elevation => "elevation",
        }
    }

}

const ALL_PLOT_TYPES: [PlotType; 2] = [PlotType::Course, PlotType::Elevation];

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
    /// Optional label that stays anchored to this value.
    #[serde(default)]
    pub value_label: Option<String>,
    /// Placement of the attached label relative to the value text.
    #[serde(default)]
    pub value_label_position: Option<ValueLabelPosition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ValueConfigFields {
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
    #[serde(default)]
    pub hours_offset: Option<f64>,
    #[serde(default)]
    pub time_format: Option<String>,
    #[serde(default)]
    pub value_label: Option<String>,
    #[serde(default)]
    pub value_label_position: Option<ValueLabelPosition>,
}

impl From<&ValueConfig> for ValueConfigFields {
    fn from(v: &ValueConfig) -> Self {
        Self {
            x: v.x,
            y: v.y,
            unit: v.unit,
            font: v.font.clone(),
            font_size: v.font_size,
            color: v.color.clone(),
            opacity: v.opacity,
            suffix: v.suffix.clone(),
            decimal_rounding: v.decimal_rounding,
            hours_offset: v.hours_offset,
            time_format: v.time_format.clone(),
            value_label: v.value_label.clone(),
            value_label_position: v.value_label_position,
        }
    }
}

impl ValueConfigFields {
    fn into_value_config(self, value: ValueType) -> ValueConfig {
        ValueConfig {
            value,
            x: self.x,
            y: self.y,
            unit: self.unit,
            font: self.font,
            font_size: self.font_size,
            color: self.color,
            opacity: self.opacity,
            suffix: self.suffix,
            decimal_rounding: self.decimal_rounding,
            hours_offset: self.hours_offset,
            time_format: self.time_format,
            value_label: self.value_label,
            value_label_position: self.value_label_position,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueLabelPosition {
    Above,
    Below,
}

// ---------------------------------------------------------------------------
// Point marker style on plots
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointStyle {
    #[serde(default)]
    pub color: Option<Color>,
    #[serde(default = "default_point_radius")]
    pub radius: f32,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub edge_color: Option<Color>,
}

fn default_point_radius() -> f32 { DEFAULT_POINT_RADIUS }

impl Default for PointStyle {
    fn default() -> Self {
        Self {
            color: None,
            radius: DEFAULT_POINT_RADIUS,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlotConfigFields {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub color: Option<Color>,
    #[serde(default)]
    pub opacity: Option<f32>,
    #[serde(default = "default_dpi")]
    pub dpi: u32,
    #[serde(default)]
    pub line: LineStyle,
    #[serde(default)]
    pub fill: Option<FillStyle>,
    #[serde(default)]
    pub rotation: f32,
    #[serde(default)]
    pub points: Vec<PointStyle>,
    #[serde(default)]
    pub point_label: Option<PointLabelConfig>,
}

impl From<&PlotConfig> for PlotConfigFields {
    fn from(p: &PlotConfig) -> Self {
        Self {
            x: p.x,
            y: p.y,
            width: p.width,
            height: p.height,
            color: p.color.clone(),
            opacity: p.opacity,
            dpi: p.dpi,
            line: p.line.clone(),
            fill: p.fill.clone(),
            rotation: p.rotation,
            points: p.points.clone(),
            point_label: p.point_label.clone(),
        }
    }
}

impl PlotConfigFields {
    fn into_plot_config(self, value: PlotType) -> PlotConfig {
        PlotConfig {
            value,
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            color: self.color,
            opacity: self.opacity,
            dpi: self.dpi,
            line: self.line,
            fill: self.fill,
            rotation: self.rotation,
            points: self.points,
            point_label: self.point_label,
        }
    }
}

fn deserialize_values<'de, D>(deserializer: D) -> Result<Vec<ValueConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ValuesRepr {
        List(Vec<ValueConfig>),
        Map(HashMap<String, ValueConfigFields>),
    }

    match ValuesRepr::deserialize(deserializer)? {
        ValuesRepr::List(values) => Ok(values),
        ValuesRepr::Map(mut map) => {
            let mut out = Vec::new();
            for vt in ALL_VALUE_TYPES {
                if let Some(fields) = map.remove(vt.key()) {
                    out.push(fields.into_value_config(vt));
                }
            }
            if let Some((bad_key, _)) = map.into_iter().next() {
                return Err(de::Error::custom(format!(
                    "Unknown value key '{bad_key}'. Expected one of: speed, power, heart_rate, cadence, gradient, elevation, distance, time, temperature"
                )));
            }
            Ok(out)
        }
    }
}

fn serialize_values<S>(values: &Vec<ValueConfig>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let mut map = serializer.serialize_map(None)?;
    for vt in ALL_VALUE_TYPES {
        if let Some(cfg) = values.iter().find(|v| v.value == vt) {
            let fields = ValueConfigFields::from(cfg);
            map.serialize_entry(vt.key(), &fields)?;
        }
    }
    map.end()
}

fn deserialize_plots<'de, D>(deserializer: D) -> Result<Vec<PlotConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum PlotsRepr {
        List(Vec<PlotConfig>),
        Map(HashMap<String, PlotConfigFields>),
    }

    match PlotsRepr::deserialize(deserializer)? {
        PlotsRepr::List(plots) => Ok(plots),
        PlotsRepr::Map(mut map) => {
            let mut out = Vec::new();
            for pt in ALL_PLOT_TYPES {
                if let Some(fields) = map.remove(pt.key()) {
                    out.push(fields.into_plot_config(pt));
                }
            }
            if let Some((bad_key, _)) = map.into_iter().next() {
                return Err(de::Error::custom(format!(
                    "Unknown plot key '{bad_key}'. Expected one of: course, elevation"
                )));
            }
            Ok(out)
        }
    }
}

fn serialize_plots<S>(plots: &Vec<PlotConfig>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let mut map = serializer.serialize_map(None)?;
    for pt in ALL_PLOT_TYPES {
        if let Some(cfg) = plots.iter().find(|p| p.value == pt) {
            let fields = PlotConfigFields::from(cfg);
            map.serialize_entry(pt.key(), &fields)?;
        }
    }
    map.end()
}

fn default_dpi() -> u32 { DEFAULT_DPI }

// ---------------------------------------------------------------------------
// Top-level Template
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub scene: SceneConfig,
    #[serde(default)]
    pub labels: Vec<LabelConfig>,
    #[serde(default, deserialize_with = "deserialize_values", serialize_with = "serialize_values")]
    pub values: Vec<ValueConfig>,
    #[serde(default, deserialize_with = "deserialize_plots", serialize_with = "serialize_plots")]
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

#[cfg(test)]
mod tests {
        use super::*;
    use serde_json::Value as JsonValue;

        #[test]
        fn value_label_fields_round_trip_through_json() {
                let mut template = Template::default_4k();
                template.values.push(ValueConfig {
                        value: ValueType::Speed,
                        x: 120,
                        y: 640,
                        unit: Some(UnitSystem::Imperial),
                        font: Some("Arial.ttf".to_string()),
                        font_size: Some(96.0),
                        color: Some(Color::new("#ffffff")),
                        opacity: Some(0.9),
                        suffix: Some(" mph".to_string()),
                        decimal_rounding: Some(0),
                        hours_offset: None,
                        time_format: None,
                        value_label: Some("MPH".to_string()),
                        value_label_position: Some(ValueLabelPosition::Below),
                });

                let json = template.to_json_pretty().expect("template should serialize");
                let parsed = Template::from_json(&json).expect("template should deserialize");

                assert_eq!(parsed.values.len(), 1);
                let value = &parsed.values[0];
                assert_eq!(value.value_label.as_deref(), Some("MPH"));
                assert_eq!(value.value_label_position, Some(ValueLabelPosition::Below));
        }

        #[test]
        fn value_label_fields_default_to_none_when_missing() {
                let json = r##"
                {
                    "scene": {
                        "width": 1920,
                        "height": 1080,
                        "fps": 30,
                        "start": 0,
                        "end": 60,
                        "font": "Arial.ttf",
                        "font_size": 30.0,
                        "color": "#ffffff",
                        "opacity": 1.0,
                        "decimal_rounding": 0,
                        "overlay_filename": "overlay.mov"
                    },
                    "values": [
                        {
                            "value": "speed",
                            "x": 100,
                            "y": 200
                        }
                    ]
                }
                "##;

                let parsed = Template::from_json(json).expect("template should deserialize");
                assert_eq!(parsed.values.len(), 1);
                let value = &parsed.values[0];
                assert_eq!(value.value_label, None);
                assert_eq!(value.value_label_position, None);
        }

        #[test]
        fn template_allows_schema_metadata_field() {
                let json = r##"
                {
                    "$schema": "./template.schema.json",
                    "scene": {
                        "width": 1920,
                        "height": 1080,
                        "fps": 30,
                        "start": 0,
                        "end": 60,
                        "font": "Arial.ttf",
                        "font_size": 30.0,
                        "color": "#ffffff",
                        "opacity": 1.0,
                        "decimal_rounding": 0,
                        "overlay_filename": "overlay.mov"
                    },
                    "values": [
                        {
                            "value": "speed",
                            "x": 100,
                            "y": 200
                        }
                    ]
                }
                "##;

                let parsed = Template::from_json(json).expect("template should deserialize with $schema metadata");
                assert_eq!(parsed.scene.width, 1920);
                assert_eq!(parsed.values.len(), 1);
                assert_eq!(parsed.values[0].value, ValueType::Speed);
        }

        #[test]
        fn template_accepts_keyed_values_and_plots() {
                let json = r##"
                {
                    "scene": {
                        "width": 1920,
                        "height": 1080,
                        "fps": 30,
                        "start": 0,
                        "end": 60,
                        "font": "Arial.ttf",
                        "font_size": 30.0,
                        "color": "#ffffff",
                        "opacity": 1.0,
                        "decimal_rounding": 0,
                        "overlay_filename": "overlay.mov"
                    },
                    "values": {
                        "speed": {
                            "x": 100,
                            "y": 200,
                            "unit": "imperial",
                            "font_size": 90
                        },
                        "power": {
                            "x": 300,
                            "y": 400
                        }
                    },
                    "plots": {
                        "course": {
                            "x": 20,
                            "y": 30,
                            "width": 600,
                            "height": 400
                        },
                        "elevation": {
                            "x": 0,
                            "y": 900,
                            "width": 1920,
                            "height": 120
                        }
                    }
                }
                "##;

                let parsed = Template::from_json(json).expect("template should deserialize keyed values/plots");
                assert_eq!(parsed.values.len(), 2);
                assert!(parsed.values.iter().any(|v| v.value == ValueType::Speed && v.x == 100));
                assert!(parsed.values.iter().any(|v| v.value == ValueType::Power && v.x == 300));
                assert_eq!(parsed.plots.len(), 2);
                assert!(parsed.plots.iter().any(|p| p.value == PlotType::Course && p.width == 600));
                assert!(parsed.plots.iter().any(|p| p.value == PlotType::Elevation && p.height == 120));
        }

        #[test]
        fn template_serializes_values_and_plots_as_keyed_objects() {
                let mut template = Template::default_4k();
                template.values.push(ValueConfig {
                        value: ValueType::Speed,
                        x: 120,
                        y: 640,
                        unit: Some(UnitSystem::Imperial),
                        font: None,
                        font_size: Some(96.0),
                        color: None,
                        opacity: None,
                        suffix: None,
                        decimal_rounding: Some(0),
                        hours_offset: None,
                        time_format: None,
                        value_label: Some("MPH".to_string()),
                        value_label_position: Some(ValueLabelPosition::Below),
                });
                template.plots.push(PlotConfig {
                        value: PlotType::Course,
                        x: 10,
                        y: 20,
                        width: 300,
                        height: 200,
                        color: None,
                        opacity: None,
                        dpi: default_dpi(),
                        line: LineStyle::default(),
                        fill: None,
                        rotation: 0.0,
                        points: Vec::new(),
                        point_label: None,
                });

                let json = template.to_json_pretty().expect("template should serialize");
                let parsed_json: JsonValue = serde_json::from_str(&json).expect("json should parse");

                assert!(parsed_json["values"].is_object());
                assert!(parsed_json["values"]["speed"].is_object());
                assert!(parsed_json["plots"].is_object());
                assert!(parsed_json["plots"]["course"].is_object());
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
