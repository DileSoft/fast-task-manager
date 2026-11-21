//! Вкладка «Производительность»: список ресурсов и крупные графики.

use eframe::egui::{self, Align, Align2, Layout, Pos2, RichText, Sense, Stroke};
use tm_core::{fmt_bytes, fmt_mhz, fmt_percent, fmt_rate, fmt_uptime, Snapshot};

use crate::app::{dim_label, fill_bg, graph_rect, stat_card, App};
use crate::graph;
use crate::theme::{bold, font, Palette};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Cpu,
    Memory,
    Disk,
    Network,
    Gpu,
}

impl Resource {
    pub fn label(self) -> &'static str {
        match self {
            Resource::Cpu => "ЦП",
            Resource::Memory => "Память",
            Resource::Disk => "Диск 0",
            Resource::Network => "Сеть",
            Resource::Gpu => "ГП 0",
        }
    }

    fn color(self, palette: &Palette) -> egui::Color32 {
        match self {
            Resource::Cpu => palette.accent,
            Resource::Memory => palette.mem,
            Resource::Disk => palette.disk,
            Resource::Network => palette.net,
            Resource::Gpu => palette.gpu,
        }
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let snapshot = app.snapshot.clone();
    let palette = app.palette;

    // Готовим ряды истории один раз на кадр.
    let hist = &snapshot.history;
    let cpu: Vec<f32> = hist.iter().map(|h| h.cpu).collect();
    let mem: Vec<f32> = hist.iter().map(|h| h.mem).collect();
    let disk: Vec<f32> = hist.iter().map(|h| h.disk).collect();
    let net_r: Vec<f32> = hist.iter().map(|h| h.net_recv).collect();
    let net_s: Vec<f32> = hist.iter().map(|h| h.net_send).collect();
    let gpu: Vec<f32> = hist.iter().map(|h| h.gpu).collect();

    let height = ui.available_height();
    let total_width = ui.available_width();
    let left_width = 238.0;
    let right_width = (total_width - left_width - 40.0).max(300.0);
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(left_width, height),
            Layout::top_down(Align::Min),
            |ui| {
                ui.add_space(4.0);
                let resources = available_resources(&snapshot);
                for res in resources {
                    let series: &[f32] = match res {
                        Resource::Cpu => &cpu,
                        Resource::Memory => &mem,
                        Resource::Disk => &disk,
                        Resource::Network => &net_r,
                        Resource::Gpu => &gpu,
                    };
                    let value = resource_value(&snapshot, res);
                    let selected = app.perf_resource == res;
                    left_item(ui, &palette, res, value, series, selected);
                }
            },
        );
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(6.0);
        ui.allocate_ui_with_layout(
            egui::vec2(right_width, height),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_width(right_width - 16.0);
                ui.add_space(6.0);
                if !available_resources(&snapshot).contains(&app.perf_resource) {
                    app.perf_resource = Resource::Cpu;
                }
                if let Some(v) = ui.data_mut(|d| d.remove_temp::<u8>(egui::Id::new("perf_click"))) {
                    app.perf_resource = match v {
                        0 => Resource::Cpu,
                        1 => Resource::Memory,
                        2 => Resource::Disk,
                        3 => Resource::Network,
                        _ => Resource::Gpu,
                    };
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(right_width - 18.0);
                        right_panel(
                            app, ui, &snapshot, &cpu, &mem, &disk, &net_r, &net_s, &gpu,
                        );
                    });
            },
        );
    });
}

fn available_resources(snapshot: &Snapshot) -> Vec<Resource> {
    let mut list = vec![
        Resource::Cpu,
        Resource::Memory,
        Resource::Disk,
        Resource::Network,
    ];
    if snapshot.capabilities.gpu_available || snapshot.system.gpu_dedicated > 0 {
        list.push(Resource::Gpu);
    }
    list
}

fn resource_value(snapshot: &Snapshot, res: Resource) -> String {
    let s = &snapshot.system;
    match res {
        Resource::Cpu => fmt_percent(s.cpu_total),
        Resource::Memory => {
            let pct = if s.mem_total > 0 {
                s.mem_used as f32 / s.mem_total as f32 * 100.0
            } else {
                0.0
            };
            fmt_percent(pct)
        }
        Resource::Disk => fmt_percent(s.disk_active),
        Resource::Network => {
            let v = s.net_recv_bps.max(s.net_send_bps) as f32;
            if v < 1024.0 {
                "0 Кбит/с".to_string()
            } else {
                fmt_rate(v as f64)
            }
        }
        Resource::Gpu => fmt_percent(s.gpu),
    }
}

fn left_item(
    ui: &mut egui::Ui,
    palette: &Palette,
    res: Resource,
    value: String,
    series: &[f32],
    selected: bool,
) {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 56.0), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 2.0, palette.selected);
        painter.rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
            0.0,
            palette.accent,
        );
    } else if resp.hovered() {
        painter.rect_filled(rect, 2.0, palette.hover);
    }

    painter.text(
        Pos2::new(rect.left() + 12.0, rect.top() + 16.0),
        Align2::LEFT_CENTER,
        res.label(),
        bold(13.5),
        palette.text_strong,
    );
    painter.text(
        Pos2::new(rect.left() + 12.0, rect.top() + 36.0),
        Align2::LEFT_CENTER,
        value,
        font(12.0),
        palette.text_dim,
    );

    let spark = egui::Rect::from_min_size(
        Pos2::new(rect.right() - 86.0, rect.top() + 14.0),
        egui::vec2(74.0, 30.0),
    );
    if spark.left() > rect.left() + 100.0 {
        graph::sparkline(ui, spark, series, res.color(palette), palette);
    }
    if resp.clicked() {
        // Клик обрабатывается в вызывающем коде через сравнение.
    }
    // Возвращаем факт клика через ответ — используем ui.data для простоты.
    if resp.clicked() {
        ui.data_mut(|d| d.insert_temp(egui::Id::new("perf_click"), res as u8));
    }
}

#[allow(clippy::too_many_arguments)]
fn right_panel(
    app: &mut App,
    ui: &mut egui::Ui,
    snapshot: &Snapshot,
    cpu: &[f32],
    mem: &[f32],
    disk: &[f32],
    net_r: &[f32],
    net_s: &[f32],
    gpu: &[f32],
) {
    let palette = app.palette;
    let s = &snapshot.system;
    let res = app.perf_resource;
    let title = match res {
        Resource::Cpu => "ЦП",
        Resource::Memory => "Память",
        Resource::Disk => "Диск 0",
        Resource::Network => "Сеть",
        Resource::Gpu => "Графический процессор 0",
    };
    let subtitle = match res {
        Resource::Cpu => format!(
            "{}  ·  {} логических процессоров  ·  {}",
            s.cpu_name,
            s.cores_logical,
            fmt_mhz(s.cpu_speed_mhz.max(s.cpu_base_mhz))
        ),
        Resource::Memory => format!(
            "{} из {}  ·  {} доступно",
            fmt_bytes(s.mem_used),
            fmt_bytes(s.mem_total),
            fmt_bytes(s.mem_avail)
        ),
        Resource::Disk => format!(
            "Диск 0  ·  {} свободно из {}",
            fmt_bytes(s.disk_free),
            fmt_bytes(s.disk_total)
        ),
        Resource::Network => format!(
            "{}  ·  канал {}",
            s.net_name,
            fmt_speed(s.net_link_bps)
        ),
        Resource::Gpu => format!(
            "{}  ·  выделено {}",
            s.gpu_name,
            fmt_bytes(s.gpu_dedicated)
        ),
    };

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(title).size(21.0).color(palette.text_strong));
            ui.label(RichText::new(subtitle).size(11.5).color(palette.text_dim));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let big = match res {
                Resource::Cpu => fmt_percent(s.cpu_total),
                Resource::Memory => fmt_percent(if s.mem_total > 0 {
                    s.mem_used as f32 / s.mem_total as f32 * 100.0
                } else {
                    0.0
                }),
                Resource::Disk => fmt_percent(s.disk_active),
                Resource::Network => {
                    format!("↓ {}  ↑ {}", fmt_rate(s.net_recv_bps), fmt_rate(s.net_send_bps))
                }
                Resource::Gpu => fmt_percent(s.gpu),
            };
            ui.label(RichText::new(big).size(26.0).color(palette.text_strong));
        });
    });
    ui.add_space(6.0);

    let graph_height = (ui.available_height() * 0.35).clamp(120.0, 220.0);
    let rect = graph_rect(ui, graph_height);

    match res {
        Resource::Cpu => {
            graph::draw(
                ui,
                rect,
                &[(cpu, palette.accent)],
                Some(100.0),
                &palette,
                true,
                true,
            );
            ui.add_space(8.0);
            stat_row(
                ui,
                &palette,
                &[
                    ("Использование", fmt_percent(s.cpu_total)),
                    ("Скорость", fmt_mhz(s.cpu_speed_mhz.max(s.cpu_base_mhz))),
                    (
                        "Ядра",
                        format!("{} логических", s.cores_logical),
                    ),
                    ("Процессы", s.processes.to_string()),
                    ("Потоки", s.threads.to_string()),
                    ("Дескрипторы", s.handles.to_string()),
                    ("Время работы", fmt_uptime(s.uptime_secs)),
                    (
                        "В режиме ядра",
                        format!("{:.1}%", s.cpu_kernel),
                    ),
                ],
            );
            ui.add_space(8.0);
            core_grid(ui, &palette, snapshot, graph_height);
        }
        Resource::Memory => {
            graph::draw(
                ui,
                rect,
                &[(mem, palette.mem)],
                Some(100.0),
                &palette,
                true,
                true,
            );
            ui.add_space(8.0);
            composition(ui, &palette, s);
            ui.add_space(8.0);
            stat_row(
                ui,
                &palette,
                &[
                    ("В работе", fmt_bytes(s.mem_used)),
                    ("Доступно", fmt_bytes(s.mem_avail)),
                    ("Кэш", fmt_bytes(s.mem_cached)),
                    ("Свободно", fmt_bytes(s.mem_free)),
                    (
                        "Выделено",
                        format!("{} / {}", fmt_bytes(s.mem_committed), fmt_bytes(s.mem_commit_limit)),
                    ),
                    ("Пулы", format!("{} / {}", fmt_bytes(s.mem_paged_pool), fmt_bytes(s.mem_nonpaged_pool))),
                ],
            );
        }
        Resource::Disk => {
            graph::draw(
                ui,
                rect,
                &[(disk, palette.disk)],
                Some(100.0),
                &palette,
                true,
                true,
            );
            ui.add_space(8.0);
            stat_row(
                ui,
                &palette,
                &[
                    ("Активное время", fmt_percent(s.disk_active)),
                    ("Чтение", fmt_rate(s.disk_read_bps)),
                    ("Запись", fmt_rate(s.disk_write_bps)),
                    ("Средний отклик", format!("{:.2} мс", s.disk_response_ms)),
                    ("Ёмкость", fmt_bytes(s.disk_total)),
                    ("Свободно", fmt_bytes(s.disk_free)),
                ],
            );
        }
        Resource::Network => {
            graph::draw(
                ui,
                rect,
                &[(net_r, palette.net), (net_s, palette.warn)],
                None,
                &palette,
                true,
                true,
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                legend_dot(ui, palette.net, &format!("Приём {}", fmt_rate(s.net_recv_bps)));
                legend_dot(ui, palette.warn, &format!("Отправка {}", fmt_rate(s.net_send_bps)));
            });
            ui.add_space(8.0);
            stat_row(
                ui,
                &palette,
                &[
                    ("Приём", fmt_rate(s.net_recv_bps)),
                    ("Отправка", fmt_rate(s.net_send_bps)),
                    ("Канал", fmt_speed(s.net_link_bps)),
                    ("Адаптер", s.net_name.clone()),
                ],
            );
        }
        Resource::Gpu => {
            graph::draw(
                ui,
                rect,
                &[(gpu, palette.gpu)],
                Some(100.0),
                &palette,
                true,
                true,
            );
            ui.add_space(8.0);
            stat_row(
                ui,
                &palette,
                &[
                    ("Загрузка", fmt_percent(s.gpu)),
                    ("Выделенная память", fmt_bytes(s.gpu_dedicated)),
                    ("Общая память", fmt_bytes(s.gpu_shared)),
                    ("Адаптер", s.gpu_name.clone()),
                ],
            );
        }
    }
}

fn stat_row(ui: &mut egui::Ui, palette: &Palette, items: &[(&str, String)]) {
    let card = 136.0;
    let gap = 6.0;
    let per_row = (((ui.available_width() + gap) / (card + 22.0 + gap)).floor() as usize).clamp(1, 4);
    for chunk in items.chunks(per_row) {
        ui.horizontal(|ui| {
            for (label, value) in chunk {
                stat_card(ui, palette, label, value, card);
            }
        });
        ui.add_space(4.0);
    }
}

fn legend_dot(ui: &mut egui::Ui, color: egui::Color32, text: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
    ui.painter().rect_filled(rect.shrink(1.0), 1.0, color);
    ui.label(RichText::new(text).size(11.5));
}

fn composition(ui: &mut egui::Ui, palette: &Palette, s: &tm_core::SystemStats) {
    let total = s.mem_total.max(1);
    let parts = [
        (s.mem_used as f32 / total as f32 * 100.0, palette.accent),
        (s.mem_cached as f32 / total as f32 * 100.0, palette.mem),
        (s.mem_free as f32 / total as f32 * 100.0, palette.border),
    ];
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 22.0),
        Sense::hover(),
    );
    fill_bg(ui, rect, palette);
    let inner = rect.shrink(3.0);
    graph::stacked_bar(ui, inner, &parts);
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        legend_dot(ui, palette.accent, "В работе");
        legend_dot(ui, palette.mem, "Кэш");
        legend_dot(ui, palette.border, "Свободно");
        legend_dot(ui, palette.warn, "Изменённые");
        dim_label(ui, palette, &format!("изменённые {}", fmt_bytes(s.mem_modified)));
    });
}

fn core_grid(ui: &mut egui::Ui, palette: &Palette, snapshot: &Snapshot, graph_height: f32) {
    let cores = snapshot.core_history.len();
    if cores == 0 {
        return;
    }
    let remaining = ui.available_height();
    if remaining < 70.0 {
        return;
    }
    let _ = graph_height;
    let columns = 2usize;
    let gap = 6.0;
    let cell_w = (ui.available_width() - gap * (columns as f32 - 1.0)) / columns as f32;
    let rows = (cores + columns - 1) / columns;
    let cell_h = ((remaining.min(140.0) - gap * (rows as f32 - 1.0)) / rows as f32).clamp(38.0, 52.0);
    ui.add_space(1.0);
    ui.separator();
    ui.add_space(4.0);

    for chunk_start in (0..cores).step_by(columns) {
        ui.horizontal(|ui| {
            for i in chunk_start..(chunk_start + columns).min(cores) {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(cell_w, cell_h), Sense::hover());
                let hist = &snapshot.core_history[i];
                let last = hist.last().copied().unwrap_or(0.0);
                ui.painter().with_clip_rect(rect).text(
                    Pos2::new(rect.left() + 3.0, rect.top() + 4.0),
                    Align2::LEFT_TOP,
                    format!("Ядро {i}  {:.0}%", last),
                    font(11.0),
                    palette.text_dim,
                );
                let inner = egui::Rect::from_min_max(
                    Pos2::new(rect.left(), rect.top() + 18.0),
                    rect.right_bottom(),
                );
                graph::sparkline(ui, inner, hist, palette.accent, palette);
            }
        });
    }
}

fn fmt_speed(bits_per_sec: u64) -> String {
    if bits_per_sec == 0 {
        return "—".to_string();
    }
    let gbit = bits_per_sec as f64 / 1e9;
    if gbit >= 1.0 {
        format!("{gbit:.1} Гбит/с")
    } else {
        format!("{:.0} Мбит/с", bits_per_sec as f64 / 1e6)
    }
}

pub fn _unused(_: &Stroke, _: &Palette) {}
