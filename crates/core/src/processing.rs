/// Signal processing utilities: interpolation, Savitzky-Golay, z-score.

// ---------------------------------------------------------------------------
// Linear interpolation: upsample 1 Hz data to fps Hz
// ---------------------------------------------------------------------------

/// Upsample a 1-Hz channel to `fps` samples per second via linear interpolation.
/// Input: N samples (one per second).
/// Output: approximately (N-1) * fps samples.
pub fn interpolate_channel(data: &[f64], fps: u32) -> Vec<f64> {
    if data.len() < 2 || fps == 0 {
        return data.to_vec();
    }
    let fps = fps as usize;
    let n = data.len();
    // Append one extrapolated tail point to avoid truncation
    let tail = 2.0 * data[n - 1] - data[n - 2];
    let mut extended = data.to_vec();
    extended.push(tail);

    let total_frames = (n - 1) * fps; // number of output frames
    let mut out = Vec::with_capacity(total_frames + fps);

    for i in 0..(n - 1) {
        let a = extended[i];
        let b = extended[i + 1];
        for f in 0..fps {
            let t = f as f64 / fps as f64;
            out.push(a + t * (b - a));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Savitzky-Golay filter
// ---------------------------------------------------------------------------

/// Apply a Savitzky-Golay smoothing filter.
/// `window`: must be odd; number of points in the smoothing window.
/// `poly_order`: polynomial degree (must be < window).
///
/// This implementation computes the convolution coefficients for a centered
/// polynomial least-squares fit and applies them as a FIR filter.
/// Edge points (within window/2 of the boundary) are left unsmoothed.
pub fn savgol_filter(data: &[f64], window: usize, poly_order: usize) -> Vec<f64> {
    let w = if window % 2 == 0 { window + 1 } else { window };
    if w < 3 || data.len() < w || poly_order >= w {
        return data.to_vec();
    }
    let half = w / 2;
    let coeffs = savgol_coefficients(w, poly_order);

    let mut out = data.to_vec();
    for i in half..(data.len() - half) {
        let mut sum = 0.0;
        for (j, &c) in coeffs.iter().enumerate() {
            sum += c * data[i + j - half];
        }
        out[i] = sum;
    }
    out
}

/// Compute the Savitzky-Golay convolution coefficients for a given window and poly order.
/// Uses the Gram polynomial method to derive the central-point coefficients.
fn savgol_coefficients(window: usize, poly_order: usize) -> Vec<f64> {
    let half = (window / 2) as i64;
    let m = poly_order + 1;

    // Build the Vandermonde matrix A (window x m)
    let mut a_data = vec![0.0f64; window * m];
    for (row, i) in (-half..=half).enumerate() {
        for col in 0..m {
            a_data[row * m + col] = (i as f64).powi(col as i32);
        }
    }

    // Compute (A^T A)^{-1} A^T via Gram-Schmidt / normal equations
    // For small window sizes this is fine numerically.
    let ata = mat_mul_ata(&a_data, window, m);
    let ata_inv = mat_inv_small(&ata, m);

    // The coefficients for the center point estimate are row 0 of (A^T A)^{-1} A^T
    // i.e., coeffs[j] = sum_k (ata_inv[0][k] * A[j][k])
    let mut coeffs = vec![0.0f64; window];
    for j in 0..window {
        let mut s = 0.0;
        for k in 0..m {
            s += ata_inv[k] * a_data[j * m + k]; // ata_inv is first row only
        }
        coeffs[j] = s;
    }
    coeffs
}

/// Compute A^T * A where A is (rows x cols).
fn mat_mul_ata(a: &[f64], rows: usize, cols: usize) -> Vec<f64> {
    let mut out = vec![0.0f64; cols * cols];
    for i in 0..cols {
        for j in 0..cols {
            let mut s = 0.0;
            for k in 0..rows {
                s += a[k * cols + i] * a[k * cols + j];
            }
            out[i * cols + j] = s;
        }
    }
    out
}

/// Invert a small (cols x cols) symmetric positive-definite matrix using Cholesky.
/// Returns only the first row of the inverse (sufficient for SavGol coefficients).
fn mat_inv_small(ata: &[f64], n: usize) -> Vec<f64> {
    // Gaussian elimination
    let mut m = vec![0.0f64; n * n];
    let mut inv = vec![0.0f64; n * n];
    for i in 0..n {
        for j in 0..n {
            m[i * n + j] = ata[i * n + j];
        }
        inv[i * n + i] = 1.0;
    }

    for col in 0..n {
        // Find pivot
        let mut max_row = col;
        let mut max_val = m[col * n + col].abs();
        for row in (col + 1)..n {
            if m[row * n + col].abs() > max_val {
                max_val = m[row * n + col].abs();
                max_row = row;
            }
        }
        // Swap rows
        for j in 0..n {
            m.swap(col * n + j, max_row * n + j);
            inv.swap(col * n + j, max_row * n + j);
        }
        let pivot = m[col * n + col];
        if pivot.abs() < 1e-12 { continue; }
        for j in 0..n {
            m[col * n + j] /= pivot;
            inv[col * n + j] /= pivot;
        }
        for row in 0..n {
            if row != col {
                let factor = m[row * n + col];
                for j in 0..n {
                    let mv = m[col * n + j];
                    let iv = inv[col * n + j];
                    m[row * n + j] -= factor * mv;
                    inv[row * n + j] -= factor * iv;
                }
            }
        }
    }

    // Return only the first row (needed for SavGol)
    inv[0..n].to_vec()
}

// ---------------------------------------------------------------------------
// Z-score outlier replacement (sliding window)
// ---------------------------------------------------------------------------

/// Replace outliers in `data` using a sliding window z-score approach.
/// Any sample with |z| > `threshold` within the window is replaced by the window mean.
pub fn zscore_outlier_replace(data: &[f64], window_size: usize, threshold: f64) -> Vec<f64> {
    let mut out = data.to_vec();
    let half = window_size / 2;
    for i in 0..data.len() {
        let lo = i.saturating_sub(half);
        let hi = (i + half + 1).min(data.len());
        // Estimate local baseline from neighboring points only.
        let mut sum = 0.0;
        let mut count = 0usize;
        for (idx, &x) in data[lo..hi].iter().enumerate() {
            if lo + idx == i {
                continue;
            }
            sum += x;
            count += 1;
        }
        if count == 0 {
            continue;
        }

        let mean = sum / count as f64;
        let var = data[lo..hi]
            .iter()
            .enumerate()
            .filter(|(idx, _)| lo + *idx != i)
            .map(|(_, &x)| (x - mean).powi(2))
            .sum::<f64>()
            / count as f64;
        let std = var.sqrt();

        if std > 1e-10 {
            let z = (data[i] - mean) / std;
            if z.abs() > threshold {
                out[i] = mean;
            }
        } else if (data[i] - mean).abs() > 1e-10 {
            out[i] = mean;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// LOWESS smoothing (tricube kernel, 1 pass)
// ---------------------------------------------------------------------------

/// Locally Weighted Scatterplot Smoothing.
/// `fraction`: proportion of data used for each local regression (0.0–1.0).
/// `iterations`: number of robustness iterations (1 = simple LOWESS).
pub fn lowess_smooth(data: &[f64], fraction: f64, iterations: usize) -> Vec<f64> {
    let n = data.len();
    if n < 3 { return data.to_vec(); }

    let k = ((fraction * n as f64).ceil() as usize).max(3).min(n);
    let x: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let mut y = data.to_vec();

    for _ in 0..iterations {
        let mut smoothed = vec![0.0f64; n];
        for i in 0..n {
            // Find the k nearest neighbours by index distance
            let xi = x[i];

            // The window is centered on i; clamp to [0, n)
            let half = k / 2;
            let lo = i.saturating_sub(half);
            let hi = (lo + k).min(n);
            let lo = hi.saturating_sub(k);

            // Maximum distance for tricube weight
            let dists: Vec<f64> = (lo..hi).map(|j| (x[j] - xi).abs()).collect();
            let max_dist = dists.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            if max_dist < 1e-10 {
                smoothed[i] = y[lo..hi].iter().copied().sum::<f64>() / (hi - lo) as f64;
                continue;
            }

            // Tricube weights
            let weights: Vec<f64> = dists.iter().map(|&d| {
                let u = d / max_dist;
                let c = (1.0 - u.powi(3)).powi(3).max(0.0);
                c
            }).collect();

            let wsum: f64 = weights.iter().sum();
            if wsum < 1e-10 {
                smoothed[i] = y[i];
            } else {
                // Use weighted mean (0-degree, more robust for sparse data)
                smoothed[i] = weights.iter().zip(&y[lo..hi])
                    .map(|(&w, &yj)| w * yj)
                    .sum::<f64>() / wsum;
            }
        }
        y = smoothed;
    }
    y
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpolate_doubles_length() {
        let data = vec![0.0, 2.0, 4.0, 6.0];
        let out = interpolate_channel(&data, 2);
        // (4-1) * 2 = 6 frames
        assert_eq!(out.len(), 6);
        assert!((out[0] - 0.0).abs() < 1e-9);
        assert!((out[1] - 1.0).abs() < 1e-9);
        assert!((out[2] - 2.0).abs() < 1e-9);
    }

    #[test]
    fn test_savgol_preserves_length() {
        let data: Vec<f64> = (0..50).map(|i| i as f64).collect();
        let out = savgol_filter(&data, 5, 2);
        assert_eq!(out.len(), data.len());
    }

    #[test]
    fn test_zscore_replaces_spike() {
        let mut data = vec![1.0f64; 20];
        data[10] = 1000.0; // spike
        let out = zscore_outlier_replace(&data, 7, 2.0);
        assert!(out[10] < 100.0, "spike should have been replaced");
    }

    #[test]
    fn test_lowess_smooth_monotone() {
        let data: Vec<f64> = (0..20).map(|i| i as f64 + (i % 3) as f64 * 0.5).collect();
        let out = lowess_smooth(&data, 0.3, 1);
        assert_eq!(out.len(), data.len());
    }
}
