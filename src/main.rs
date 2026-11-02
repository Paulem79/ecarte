// Pas de console sous Windows en release.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod app;
mod data;
mod doc;
mod export;
mod geo;
mod logs;
mod scene;
mod svg;
mod update;
mod world;

fn main() -> eframe::Result<()> {
    logs::init();
    update::init();
    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("E-cart-é")
            .with_inner_size([1100.0, 660.0])
            .with_min_inner_size([480.0, 320.0]),
        ..Default::default()
    };
    let arg = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    eframe::run_native(
        "E-cart-é",
        opts,
        Box::new(move |cc| {
            let mut a = app::EcarteApp::new(cc);
            if let Some(p) = arg {
                a.open_path(p);
            }
            Ok(Box::new(a))
        }),
    )
}
