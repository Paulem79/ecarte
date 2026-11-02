// Pas de console sous Windows en release.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod app;
mod data;
mod doc;
mod export;
mod geo;
mod scene;
mod svg;
mod world;

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("E-cart-é")
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native("E-cart-é", opts, Box::new(|cc| Ok(Box::new(app::EcarteApp::new(cc)))))
}
