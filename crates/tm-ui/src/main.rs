#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod graph;
mod icons;
mod perf;
mod table;
mod theme;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([900.0, 570.0])
            .with_min_inner_size([760.0, 460.0])
            .with_title("Диспетчер задач"),
        ..Default::default()
    };
    eframe::run_native(
        "Диспетчер задач",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
