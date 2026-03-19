use anyhow::{Context, Result};
use std::path::Path;
use chrono::{DateTime, Utc};
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::gradient::{smooth_gradients, elevation_angle};
use crate::processing::{interpolate_channel, savgol_filter};
use crate::template::ValueType;

// ---------------------------------------------------------------------------
// Raw track point (1 Hz, direct from GPX)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TrackPoint {
    pub time: DateTime<Utc>,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation: Option<f64>,
    pub speed: Option<f64>,        // m/s, derived from GPS distance/time
    pub heart_rate: Option<f64>,
    pub cadence: Option<f64>,
    pub power: Option<f64>,
    pub temperature: Option<f64>,
}

// ---------------------------------------------------------------------------
// Activity — parsed and pre-processed telemetry
// ---------------------------------------------------------------------------

/// Holds parsed GPX data at 1 Hz (one sample per second).
/// Call `trim()` then `interpolate()` before using for rendering.
#[derive(Debug, Clone)]
pub struct Activity {
    /// Available data channels detected in the GPX file
    pub valid_attributes: Vec<ValueType>,

    // Raw 1-Hz arrays, populated from the GPX track
    pub times: Vec<DateTime<Utc>>,
    pub course: Vec<(f64, f64)>,       // (lat, lon)
    pub elevation: Vec<f64>,            // metres
    pub speed: Vec<f64>,                // m/s
    pub gradient: Vec<f64>,             // smoothed percent-grade-like value
    pub heart_rate: Vec<f64>,
    pub cadence: Vec<f64>,
    pub power: Vec<f64>,
    pub temperature: Vec<f64>,

    /// Frames-per-second after interpolation (set by `interpolate()`)
    pub fps: u32,
    /// Whether `interpolate()` has been called
    pub interpolated: bool,
}

impl Activity {
    /// Parse a `.gpx` file from disk into an Activity.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let gpx_str = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read GPX file: {}", path.display()))?;
        Self::from_str(&gpx_str)
    }

    /// Parse from an in-memory GPX string.
    pub fn from_str(gpx_str: &str) -> Result<Self> {
        let cursor = std::io::Cursor::new(gpx_str.as_bytes());
        let gpx_data = gpx::read(cursor).context("Failed to parse GPX content")?;
        let extension_points = parse_trackpoint_extensions(gpx_str);
        Self::from_gpx(gpx_data, Some(extension_points))
    }

    fn from_gpx(gpx_data: gpx::Gpx, extension_points: Option<Vec<TrackPointExtensions>>) -> Result<Self> {
        let track = gpx_data.tracks.into_iter().next()
            .context("GPX file contains no tracks")?;
        let segment = track.segments.into_iter().next()
            .context("GPX track contains no segments")?;

        let points: Vec<gpx::Waypoint> = segment.points;
        if points.is_empty() {
            anyhow::bail!("GPX segment contains no track points");
        }

        let mut times: Vec<DateTime<Utc>> = Vec::with_capacity(points.len());
        let mut course: Vec<(f64, f64)> = Vec::with_capacity(points.len());
        let mut elevation_raw: Vec<f64> = Vec::with_capacity(points.len());
        let mut speed_raw: Vec<f64> = Vec::with_capacity(points.len());
        let mut gradient_raw: Vec<Option<f64>> = Vec::with_capacity(points.len());
        let mut heart_rate_raw: Vec<Option<f64>> = Vec::with_capacity(points.len());
        let mut cadence_raw: Vec<Option<f64>> = Vec::with_capacity(points.len());
        let mut power_raw: Vec<Option<f64>> = Vec::with_capacity(points.len());
        let mut temperature_raw: Vec<Option<f64>> = Vec::with_capacity(points.len());

        let mut prev_point: Option<&gpx::Waypoint> = None;

        for (i, pt) in points.iter().enumerate() {
            let geo = &pt.point();
            let lat = geo.y();
            let lon = geo.x();
            let elev = pt.elevation.unwrap_or(0.0);
            let time = pt.time
                .and_then(|t| t.format().ok())
                .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                .unwrap_or(Utc::now());

            times.push(time);
            course.push((lat, lon));
            elevation_raw.push(elev);

            // Speed: computed from Haversine distance / time delta
            let spd = if i > 0 {
                let prev = prev_point.unwrap();
                let prev_geo = prev.point();
                let dist = haversine_distance(
                    prev_geo.y(), prev_geo.x(),
                    lat, lon,
                );
                let dt = (time - times[i - 1]).num_milliseconds() as f64 / 1000.0;
                if dt > 0.0 { dist / dt } else { 0.0 }
            } else {
                0.0
            };
            speed_raw.push(spd);

            // Gradient: elevation angle between consecutive points (raw, will be smoothed)
            let grad = if i > 0 {
                let prev = prev_point.unwrap();
                let prev_geo = prev.point();
                let prev_elev = prev.elevation.unwrap_or(0.0);
                elevation_angle(prev_geo.y(), prev_geo.x(), prev_elev, lat, lon, elev)
            } else {
                None
            };
            gradient_raw.push(grad);

            // Extensions: parse Garmin TrackPointExtension values (HR/cadence/power/temp)
            let ext = extension_points
                .as_ref()
                .and_then(|values| values.get(i));
            let (hr, cad, pwr, temp) = match ext {
                Some(v) => (v.hr, v.cad, v.power, v.temp),
                None => (None, None, None, None),
            };
            heart_rate_raw.push(hr);
            cadence_raw.push(cad);
            power_raw.push(pwr);
            temperature_raw.push(temp);

            prev_point = Some(pt);
        }

        // Smooth elevation with Savitzky-Golay (window=11, poly=3)
        let elevation = savgol_filter(&elevation_raw, 11, 3);

        // Smooth gradient pipeline
        let gradient = smooth_gradients(gradient_raw);

        // Detect which attributes have actual data
        let mut valid_attributes = vec![
            ValueType::Speed,
            ValueType::Elevation,
            ValueType::Gradient,
            ValueType::Time,
            ValueType::Timecode,
        ];

        let has_hr = heart_rate_raw.iter().any(|v| v.is_some());
        let has_cad = cadence_raw.iter().any(|v| v.is_some());
        let has_pwr = power_raw.iter().any(|v| v.is_some());
        let has_temp = temperature_raw.iter().any(|v| v.is_some());
        let has_course = course.len() > 1;

        if has_hr { valid_attributes.push(ValueType::HeartRate); }
        if has_cad { valid_attributes.push(ValueType::Cadence); }
        if has_pwr { valid_attributes.push(ValueType::Power); }
        if has_temp { valid_attributes.push(ValueType::Temperature); }
        if has_course { valid_attributes.push(ValueType::Distance); }

        // Convert Option<f64> arrays to f64 with 0.0 for missing
        let heart_rate = heart_rate_raw.into_iter().map(|v| v.unwrap_or(0.0)).collect();
        let cadence = cadence_raw.into_iter().map(|v| v.unwrap_or(0.0)).collect();
        let power = power_raw.into_iter().map(|v| v.unwrap_or(0.0)).collect();
        let temperature = temperature_raw.into_iter().map(|v| v.unwrap_or(0.0)).collect();

        Ok(Activity {
            valid_attributes,
            times,
            course,
            elevation,
            speed: speed_raw,
            gradient,
            heart_rate,
            cadence,
            power,
            temperature,
            fps: 1,
            interpolated: false,
        })
    }

    /// Duration of the activity in seconds.
    pub fn duration_seconds(&self) -> usize {
        self.times.len().saturating_sub(1)
    }

    /// Trim the activity to [start_sec, end_sec) from the beginning of the track.
    pub fn trim(&mut self, start_sec: usize, end_sec: usize) -> Result<()> {
        let n = self.times.len();
        anyhow::ensure!(start_sec < end_sec, "trim: start ({}) must be < end ({})", start_sec, end_sec);
        anyhow::ensure!(end_sec <= n, "trim: end ({}) exceeds activity length ({})", end_sec, n);

        self.times    = self.times[start_sec..end_sec].to_vec();
        self.course   = self.course[start_sec..end_sec].to_vec();
        self.elevation = self.elevation[start_sec..end_sec].to_vec();
        self.speed    = self.speed[start_sec..end_sec].to_vec();
        self.gradient = self.gradient[start_sec..end_sec].to_vec();
        self.heart_rate = self.heart_rate[start_sec..end_sec].to_vec();
        self.cadence  = self.cadence[start_sec..end_sec].to_vec();
        self.power    = self.power[start_sec..end_sec].to_vec();
        self.temperature = self.temperature[start_sec..end_sec].to_vec();

        Ok(())
    }

    /// Upsample all channels from 1 Hz to `fps` Hz using linear interpolation.
    /// Must be called after `trim()`. Time array stays at 1 Hz.
    pub fn interpolate(&mut self, fps: u32) {
        if fps == 1 {
            self.fps = 1;
            self.interpolated = true;
            return;
        }

        self.elevation   = interpolate_channel(&self.elevation, fps);
        self.speed       = interpolate_channel(&self.speed, fps);
        self.gradient    = interpolate_channel(&self.gradient, fps);
        self.heart_rate  = interpolate_channel(&self.heart_rate, fps);
        self.cadence     = interpolate_channel(&self.cadence, fps);
        self.power       = interpolate_channel(&self.power, fps);
        self.temperature = interpolate_channel(&self.temperature, fps);

        // Course: interpolate lat and lon separately
        let lats: Vec<f64> = self.course.iter().map(|(lat, _)| *lat).collect();
        let lons: Vec<f64> = self.course.iter().map(|(_, lon)| *lon).collect();
        let ilats = interpolate_channel(&lats, fps);
        let ilons = interpolate_channel(&lons, fps);
        self.course = ilats.into_iter().zip(ilons).collect();

        self.fps = fps;
        self.interpolated = true;
    }

    /// Get the value for a given ValueType at frame index `frame_idx` (post-interpolation).
    /// For Time, pass the 1-Hz second index instead.
    pub fn value_at(&self, vtype: ValueType, frame_idx: usize) -> f64 {
        match vtype {
            ValueType::Speed       => self.speed.get(frame_idx).copied().unwrap_or(0.0),
            ValueType::Elevation   => self.elevation.get(frame_idx).copied().unwrap_or(0.0),
            ValueType::Gradient    => self.gradient.get(frame_idx).copied().unwrap_or(0.0),
            ValueType::HeartRate   => self.heart_rate.get(frame_idx).copied().unwrap_or(0.0),
            ValueType::Cadence     => self.cadence.get(frame_idx).copied().unwrap_or(0.0),
            ValueType::Power       => self.power.get(frame_idx).copied().unwrap_or(0.0),
            ValueType::Temperature => self.temperature.get(frame_idx).copied().unwrap_or(0.0),
            ValueType::Distance    => {
                // Cumulative distance in metres up to this frame
                let fps = self.fps as usize;
                let sec = frame_idx / fps;
                self.cumulative_distance_at(sec)
            }
            ValueType::Time        => {
                // Returns seconds since epoch; caller formats with chrono
                let fps = self.fps as usize;
                let sec = if fps > 0 { frame_idx / fps } else { frame_idx };
                self.times.get(sec)
                    .map(|t| t.timestamp() as f64)
                    .unwrap_or(0.0)
            }
            ValueType::Timecode    => {
                // Same base timestamp as Time; formatter adds frame component.
                let fps = self.fps as usize;
                let sec = if fps > 0 { frame_idx / fps } else { frame_idx };
                self.times.get(sec)
                    .map(|t| t.timestamp() as f64)
                    .unwrap_or(0.0)
            }
        }
    }

    /// Returns the DateTime at a given 1-Hz second index.
    pub fn time_at(&self, second: usize) -> Option<DateTime<Utc>> {
        self.times.get(second).copied()
    }

    /// Cumulative distance (metres) from start to second `sec`.
    pub fn cumulative_distance_at(&self, sec: usize) -> f64 {
        // Use 1 Hz course data regardless of interpolation
        // (course has been interpolated but we only need approximate distance)
        let n = self.times.len().min(sec + 1);
        if n < 2 { return 0.0; }
        let mut total = 0.0f64;
        for i in 1..n {
            let fps = self.fps as usize;
            let idx = i * fps; // first frame of each second in interpolated array
            let prev_idx = (i - 1) * fps;
            let (lat1, lon1) = self.course.get(prev_idx).copied().unwrap_or((0.0, 0.0));
            let (lat2, lon2) = self.course.get(idx).copied().unwrap_or((0.0, 0.0));
            total += haversine_distance(lat1, lon1, lat2, lon2);
        }
        total
    }
}

// ---------------------------------------------------------------------------
// Haversine distance (metres)
// ---------------------------------------------------------------------------

pub fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_000.0; // Earth radius in metres
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    R * c
}

// ---------------------------------------------------------------------------
// Garmin extension parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
struct TrackPointExtensions {
    hr: Option<f64>,
    cad: Option<f64>,
    power: Option<f64>,
    temp: Option<f64>,
}

fn parse_trackpoint_extensions(gpx_xml: &str) -> Vec<TrackPointExtensions> {
    fn local_name(name: &[u8]) -> &[u8] {
        match name.iter().rposition(|b| *b == b':') {
            Some(idx) => &name[idx + 1..],
            None => name,
        }
    }

    fn parse_number(text: &[u8]) -> Option<f64> {
        std::str::from_utf8(text).ok()?.trim().parse::<f64>().ok()
    }

    let mut reader = Reader::from_str(gpx_xml);
    reader.config_mut().trim_text(true);

    let mut points = Vec::new();
    let mut current = TrackPointExtensions::default();
    let mut current_tag: Option<Vec<u8>> = None;
    let mut in_trkpt = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let tag = local_name(e.name().as_ref()).to_vec();
                if tag.as_slice() == b"trkpt" {
                    in_trkpt = true;
                    current = TrackPointExtensions::default();
                    current_tag = None;
                } else if in_trkpt {
                    current_tag = Some(tag);
                }
            }
            Ok(Event::Text(e)) => {
                if !in_trkpt {
                    continue;
                }
                let Some(tag) = current_tag.as_deref() else {
                    continue;
                };
                let Some(value) = parse_number(e.as_ref()) else {
                    continue;
                };

                match tag {
                    b"hr" => current.hr = Some(value),
                    b"cad" | b"cadence" => current.cad = Some(value),
                    b"power" | b"pwr" | b"watts" => current.power = Some(value),
                    b"atemp" | b"temp" | b"temperature" => current.temp = Some(value),
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let tag = local_name(e.name().as_ref()).to_vec();
                if tag.as_slice() == b"trkpt" {
                    points.push(current);
                    in_trkpt = false;
                    current_tag = None;
                } else if in_trkpt {
                    if current_tag.as_deref() == Some(tag.as_slice()) {
                        current_tag = None;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    points
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::ValueType;

    #[test]
    fn test_parse_trackpoint_extensions_from_fixture() {
        let gpx = include_str!("../tests/fixtures/sample_30s.gpx");
        let points = parse_trackpoint_extensions(gpx);
        assert_eq!(points.len(), 31, "expected one extension record per trackpoint");

        assert!(points[0].hr.is_some());
        assert!(points[0].cad.is_some());
        assert!(points[0].power.is_some());
        assert!(points[0].temp.is_some());
    }

    #[test]
    fn test_activity_detects_extension_attributes() {
        let gpx = include_str!("../tests/fixtures/sample_30s.gpx");
        let activity = Activity::from_str(gpx).expect("fixture should parse");

        assert!(activity.valid_attributes.contains(&ValueType::HeartRate));
        assert!(activity.valid_attributes.contains(&ValueType::Cadence));
        assert!(activity.valid_attributes.contains(&ValueType::Power));
        assert!(activity.valid_attributes.contains(&ValueType::Temperature));
    }
}
