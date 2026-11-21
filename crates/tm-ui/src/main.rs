#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod graph;
mod icons;
mod perf;
mod settings;
mod table;
mod theme;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let settings = settings::Settings::load();
    let width = settings.window_w.clamp(760.0, 4000.0);
    let height = settings.window_h.clamp(460.0, 3000.0);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([width, height])
            .with_min_inner_size([760.0, 460.0])
            .with_title("Диспетчер задач")
            .with_icon(app_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Диспетчер задач",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, settings)))),
    )
}

/// Иконка приложения: тёмный квадрат с тремя столбиками графика.
fn app_icon() -> egui::IconData {
    const SIZE: usize = 64;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    let background = [0x1d, 0x24, 0x30];
    let accent = [0x4c, 0xc2, 0xff];

    let radius = 12.0f32;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let inset = radius;
            let cx = px.clamp(inset, SIZE as f32 - inset);
            let cy = py.clamp(inset, SIZE as f32 - inset);
            let dist = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
            let inside = dist <= radius
                && px > 1.0
                && py > 1.0
                && px < SIZE as f32 - 1.0
                && py < SIZE as f32 - 1.0;
            if !inside {
                continue;
            }
            let idx = (y * SIZE + x) * 4;
            rgba[idx..idx + 4].copy_from_slice(&[background[0], background[1], background[2], 255]);

            // Три столбика разной высоты.
            let bars = [(14usize, 26usize), (27, 38), (40, 48)];
            for (bx, top) in bars {
                if x >= bx && x < bx + 8 && y >= top && y < 50 {
                    rgba[idx..idx + 4].copy_from_slice(&[accent[0], accent[1], accent[2], 255]);
                }
            }
        }
    }

    egui::IconData {
        rgba,
        width: SIZE as u32,
        height: SIZE as u32,
    }
}
