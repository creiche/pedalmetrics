use crate::activity::haversine_distance;
use crate::processing::{lowess_smooth, savgol_filter, zscore_outlier_replace};

/// Compute the elevation angle (degrees) between two consecutive GPS points.
/// Equivalent to Python's `gpxpy.geo.elevation_angle(loc1, loc2)`.
/// Returns `None` for the first point or when horizontal distance is zero.
pub fn elevation_angle(
    lat1: f64, lon1: f64, elev1: f64,
    lat2: f64, lon2: f64, elev2: f64,
) -> Option<f64> {
    let horiz = haversine_distance(lat1, lon1, lat2, lon2);
    if horiz < 1e-6 {
        return None;
    }
    let delta_elev = elev2 - elev1;
    // atan2(Δelevation, horizontal_distance) in degrees
    Some(delta_elev.atan2(horiz).to_degrees())
}

/// Full gradient smoothing pipeline (mirrors Python's `smooth_gradients()`):
/// 1. Remove leading None (no previous point for first sample)
/// 2. Extrapolate the first value backwards
/// 3. Z-score outlier replacement (window=7, threshold=2)
/// 4. LOWESS smoothing (fraction=0.0005, iterations=1)
/// 5. Scale by 1.747 (empirical factor: elevation angle → display percent grade)
pub fn smooth_gradients(raw: Vec<Option<f64>>) -> Vec<f64> {
    let n = raw.len();
    if n == 0 {
        return vec![];
    }

    // Build initial float array: first element gets the same value as the second
    // (there's no "previous point" for index 0)
    let mut grads: Vec<f64> = Vec::with_capacity(n);
    let first_valid = raw.iter().find_map(|v| *v).unwrap_or(0.0);
    grads.push(first_valid);
    for v in raw.iter().skip(1) {
        grads.push(v.unwrap_or(0.0));
    }

    // Z-score outlier replacement
    let grads = zscore_outlier_replace(&grads, 7, 2.0);

    // LOWESS smoothing (fraction = 0.0005 of total length, min 3 points)
    let fraction = 0.0005_f64.max(3.0 / n as f64);
    let grads = lowess_smooth(&grads, fraction, 1);

    // Empirical scale factor (matches Python's 1.747 multiplier)
    grads.into_iter().map(|g| g * 1.747).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elevation_angle_flat() {
        // Same elevation → angle is 0
        let angle = elevation_angle(0.0, 0.0, 100.0, 0.001, 0.001, 100.0);
        assert!(angle.is_some());
        assert!(angle.unwrap().abs() < 0.01);
    }

    #[test]
    fn test_elevation_angle_climb() {
        // Rising elevation → positive angle
        let angle = elevation_angle(0.0, 0.0, 0.0, 0.001, 0.001, 10.0);
        assert!(angle.unwrap() > 0.0);
    }

    #[test]
    fn test_smooth_gradients_length_preserved() {
        let raw: Vec<Option<f64>> = (0..100).map(|i| {
            if i == 0 { None } else { Some(i as f64 * 0.01) }
        }).collect();
        let out = smooth_gradients(raw);
        assert_eq!(out.len(), 100);
    }
}
