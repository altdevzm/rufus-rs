mod app;
mod device;
mod engine;
mod types;

use app::RufusApp;
use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([460.0, 640.0])
            .with_min_inner_size([420.0, 580.0])
            .with_title("Rufus-RS — Bootable USB Creator"),
        ..Default::default()
    };

    eframe::run_native(
        "Rufus-RS",
        options,
        Box::new(|_cc| Ok(Box::new(RufusApp::default()))),
    )
}
