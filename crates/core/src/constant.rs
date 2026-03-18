/// Conversion factors
pub const MPH_CONVERSION: f64 = 2.23694;   // m/s → mph
pub const KMH_CONVERSION: f64 = 3.6;       // m/s → km/h
pub const FT_CONVERSION: f64 = 3.28084;    // m → ft

/// Default styling values
pub const DEFAULT_DPI: u32 = 300;
pub const DEFAULT_LINE_WIDTH: f32 = 1.75;
pub const DEFAULT_MARGIN: f64 = 0.1;
pub const DEFAULT_POINT_WEIGHT: f32 = 80.0;
pub const DEFAULT_OPACITY: f32 = 1.0;
pub const DEFAULT_COLOR: &str = "#ffffff";
pub const DEFAULT_FONT_SIZE: f32 = 30.0;
pub const DEFAULT_FPS: u32 = 30;
pub const DEFAULT_OVERLAY_FILENAME: &str = "overlay.mov";

/// Returns the application support directory for user data.
/// macOS: ~/Library/Application Support/Pedalmetrics
pub fn app_support_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Pedalmetrics")
}

/// Returns the templates directory (user-editable).
pub fn templates_dir() -> std::path::PathBuf {
    // Prefer a user-visible location so templates are easy to find/share.
    // macOS: ~/Documents/Pedalmetrics/templates
    if let Some(documents) = dirs::document_dir() {
        return documents.join("Pedalmetrics").join("templates");
    }
    app_support_dir().join("templates")
}

/// Returns the uploads directory for GPX files.
pub fn uploads_dir() -> std::path::PathBuf {
    app_support_dir().join("uploads")
}

/// Returns the output directory for rendered videos.
/// macOS: ~/Downloads/Pedalmetrics
pub fn downloads_dir() -> std::path::PathBuf {
    dirs::download_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Pedalmetrics")
}

/// Returns the fonts directory bundled with the binary.
/// During development, looks relative to CARGO_MANIFEST_DIR.
pub fn fonts_dir() -> std::path::PathBuf {
    // In production this should be resolved relative to the binary.
    // CARGO_MANIFEST_DIR is set at compile time; at runtime we use the executable dir.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("fonts");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    // Development fallback: workspace root fonts/
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()              // crates/core  → crates
        .parent().unwrap()              // crates        → workspace root
        .join("fonts")
}
