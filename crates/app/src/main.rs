mod app;
mod ui;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Pedalmetrics")
            .with_min_inner_size([1024.0, 700.0])
            .with_inner_size([1440.0, 900.0])
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "Pedalmetrics",
        native_options,
        Box::new(|cc| Ok(Box::new(app::PedalmetricsApp::new(cc)))),
    )
}

fn load_icon() -> egui::IconData {
    // Embed the app icon at compile time; fall back to empty if not found.
    let icon_bytes = include_bytes!("../../../assets/icon.png");
    if let Ok(img) = image::load_from_memory(icon_bytes) {
        let img = img.into_rgba8();
        let (w, h) = img.dimensions();
        egui::IconData {
            rgba: img.into_raw(),
            width: w,
            height: h,
        }
    } else {
        egui::IconData::default()
    }
}
