//! Вкладка «Производительность»: список ресурсов, графики и карточки.
//!
//! Ресурсы динамические: ЦП, память, каждый физический диск, каждый сетевой
//! адаптер и (если есть счётчики) графический процессор.

use eframe::egui::{self, Align, Align2, Layout, Pos2, RichText, Sense};
use tm_core::{fmt_bytes, fmt_mhz, fmt_percent, fmt_rate, fmt_uptime, Snapshot, SystemStats};

use crate::app::{dim_label, fill_bg, graph_rect, stat_card, App};
use crate::graph;
use crate::theme::{bold, font, Palette};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resource {
    Cpu,
    Memory,
    Disk(usize),
    Network(usize),
    Gpu,
}

impl Resource {
    pub fn kind(self) -> &'static str {
        match self {
            Resource::Cpu => "cpu",
            Resource::Memory => "memory",
            Resource::Disk(_) => "disk",
            Resource::Network(_) => "network",
            Resource::Gpu => "gpu",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Resource::Disk(i) | Resource::Network(i) => i,
            _ => 0,
        }
    }

    pub fn from_parts(kind: &str, index: usize) -> Self {
        match kind {
            "memory" => Resource::Memory,
            "disk" => Resource::Disk(index),
            "network" => Resource::Network(index),
            "gpu" => Resource::Gpu,
            _ => Resource::Cpu,
        }
    }

    fn color(self, palette: &Palette) -> egui::Color32 {
        match self {
            Resource::Cpu => palette.accent,
            Resource::Memory => palette.mem,
            Resource::Disk(_) => palette.disk,
            Resource::Network(_) => palette.net,
            Resource::Gpu => palette.gpu,
        }
    }
}

fn label(snapshot: &Snapshot, res: Resource) -> String {
    match res {
        Resource::Cpu => "ЦП".to_string(),
        Resource::Memory => "Память".to_string(),
        Resource::Disk(i) => snapshot
            .disks
            .get(i)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| "Диск".to_string()),
        Resource::Network(i) => snapshot
            .nets
            .get(i)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| "Сеть".to_string()),
        Resource::Gpu => "ГП 0".to_string(),
    }
}

/// Подзаголовок пункта: название оборудования, к которому относится ресурс.
fn detail(snapshot: &Snapshot, res: Resource) -> String {
    let s = &snapshot.system;
    match res {
        Resource::Cpu => s.cpu_name.clone(),
        Resource::Memory => format!("ОЗУ · {}", fmt_bytes(s.mem_total)),
        Resource::Disk(i) => snapshot
            .disks
            .get(i)
            .map(|d| {
                let model = d.model.trim();
                let volumes = d.volumes.trim();
                match (volumes.is_empty(), model.is_empty()) {
                    (false, false) => format!("{volumes}  ·  {model}"),
                    (true, false) => model.to_string(),
                    (false, true) => format!("тома {volumes}"),
                    (true, true) => "физический диск".to_string(),
                }
            })
            .unwrap_or_default(),
        Resource::Network(i) => snapshot
            .nets
            .get(i)
            .map(|n| {
                if n.description.trim().is_empty() {
                    "сетевой адаптер".to_string()
                } else {
                    n.description.clone()
                }
            })
            .unwrap_or_default(),
        Resource::Gpu => {
            if s.gpu_name.trim().is_empty() {
                "графический процессор".to_string()
            } else {
                s.gpu_name.clone()
            }
        }
    }
}

fn available(snapshot: &Snapshot) -> Vec<Resource> {
    let mut list = vec![Resource::Cpu, Resource::Memory];
    for i in 0..snapshot.disks.len() {
        list.push(Resource::Disk(i));
    }
    for i in 0..snapshot.nets.len() {
        list.push(Resource::Network(i));
    }
    if snapshot.capabilities.gpu_available || snapshot.system.gpu_dedicated > 0 {
        list.push(Resource::Gpu);
    }
    list
}

fn value(snapshot: &Snapshot, res: Resource) -> String {
    let s = &snapshot.system;
    match res {
        Resource::Cpu => fmt_percent(s.cpu_total),
        Resource::Memory => fmt_percent(mem_percent(s)),
        Resource::Disk(i) => snapshot
            .disks
            .get(i)
            .map(|d| fmt_percent(d.active))
            .unwrap_or_else(|| "0%".to_string()),
        Resource::Network(i) => snapshot
            .nets
            .get(i)
            .map(|n| {
                let v = n.recv_bps.max(n.send_bps) as f32;
                if v < 1024.0 {
                    "0 Кбит/с".to_string()
                } else {
                    fmt_rate(v as f64)
                }
            })
            .unwrap_or_default(),
        Resource::Gpu => fmt_percent(s.gpu),
    }
}

fn mem_percent(s: &SystemStats) -> f32 {
    if s.mem_total > 0 {
        s.mem_used as f32 / s.mem_total as f32 * 100.0
    } else {
        0.0
    }
}

fn series(snapshot: &Snapshot, res: Resource) -> Vec<f32> {
    match res {
        Resource::Cpu => snapshot.history.iter().map(|h| h.cpu).collect(),
        Resource::Memory => snapshot.history.iter().map(|h| h.mem).collect(),
        Resource::Disk(i) => snapshot.disk_history.get(i).cloned().unwrap_or_default(),
        Resource::Network(i) => snapshot
            .net_recv_history
            .get(i)
            .cloned()
            .unwrap_or_default(),
        Resource::Gpu => snapshot.history.iter().map(|h| h.gpu).collect(),
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let snapshot = app.snapshot.clone();
    let palette = app.palette;
    let resources = available(&snapshot);
    if !resources.contains(&app.perf_resource) {
        app.perf_resource = resources.first().copied().unwrap_or(Resource::Cpu);
    }

    let height = ui.available_height();
    let total_width = ui.available_width();
    let left_width = 262.0;
    let right_width = (total_width - left_width - 40.0).max(300.0);

    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(left_width, height),
            Layout::top_down(Align::Min),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("perf-resources")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(left_width - 10.0);
                        ui.add_space(2.0);
                        for res in &resources {
                            let selected = app.perf_resource == *res;
                            if left_item(ui, &palette, &snapshot, *res, selected) {
                                app.perf_resource = *res;
                                app.settings_dirty = true;
                            }
                        }
                        ui.add_space(6.0);
                    });
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
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(right_width - 18.0);
                        right_panel(app, ui, &snapshot, app.perf_resource);
                    });
            },
        );
    });
}

fn left_item(
    ui: &mut egui::Ui,
    palette: &Palette,
    snapshot: &Snapshot,
    res: Resource,
    selected: bool,
) -> bool {
    let label = label(snapshot, res);
    let detail = detail(snapshot, res);
    let value = value(snapshot, res);
    let data = series(snapshot, res);
    let color = res.color(palette);
    let label = label.as_str();
    let detail = detail.as_str();
    let series = data.as_slice();
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 66.0), Sense::click());
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

    let label_clip = egui::Rect::from_min_max(
        Pos2::new(rect.left() + 10.0, rect.top()),
        Pos2::new(rect.right() - 92.0, rect.top() + 26.0),
    );
    crate::theme::text_clipped(
        ui,
        label_clip,
        Pos2::new(rect.left() + 12.0, rect.top() + 13.0),
        Align2::LEFT_CENTER,
        label,
        bold(12.5),
        palette.text_strong,
    );
    theme_text(
        ui,
        Pos2::new(rect.left() + 12.0, rect.top() + 32.0),
        Align2::LEFT_CENTER,
        value,
        font(12.0),
        palette.text,
    );
    let detail_clip = egui::Rect::from_min_max(
        Pos2::new(rect.left() + 10.0, rect.top() + 38.0),
        Pos2::new(rect.right() - 6.0, rect.bottom()),
    );
    crate::theme::text_clipped(
        ui,
        detail_clip,
        Pos2::new(rect.left() + 12.0, rect.top() + 51.0),
        Align2::LEFT_CENTER,
        detail,
        font(10.5),
        palette.text_dim,
    );

    let spark = egui::Rect::from_min_size(
        Pos2::new(rect.right() - 86.0, rect.top() + 16.0),
        egui::vec2(74.0, 28.0),
    );
    if spark.left() > rect.left() + 100.0 {
        graph::sparkline(ui, spark, series, color, palette);
    }
    let tip = if detail.is_empty() {
        label.to_string()
    } else {
        format!("{label}\n{detail}")
    };
    resp.on_hover_text(tip).clicked()
}

fn theme_text(
    ui: &egui::Ui,
    pos: Pos2,
    align: Align2,
    text: impl ToString,
    font_id: egui::FontId,
    color: egui::Color32,
) {
    crate::theme::text(ui, pos, align, text, font_id, color);
}

fn right_panel(app: &mut App, ui: &mut egui::Ui, snapshot: &Snapshot, res: Resource) {
    let palette = app.palette;
    let s = &snapshot.system;

    let (title, subtitle, big) = match res {
        Resource::Cpu => (
            "ЦП".to_string(),
            format!(
                "{}  ·  {} логических процессоров  ·  {}",
                s.cpu_name,
                s.cores_logical,
                fmt_mhz(s.cpu_speed_mhz.max(s.cpu_base_mhz))
            ),
            fmt_percent(s.cpu_total),
        ),
        Resource::Memory => (
            "Память".to_string(),
            format!(
                "{} из {}  ·  {} доступно",
                fmt_bytes(s.mem_used),
                fmt_bytes(s.mem_total),
                fmt_bytes(s.mem_avail)
            ),
            fmt_percent(mem_percent(s)),
        ),
        Resource::Disk(i) => {
            let disk = snapshot.disks.get(i);
            (
                disk.map(|d| d.name.clone()).unwrap_or_default(),
                disk.map(|d| {
                    let volumes = if d.volumes.is_empty() {
                        "физический диск".to_string()
                    } else {
                        format!("тома {}", d.volumes)
                    };
                    format!(
                        "{}  ·  {} свободно из {}",
                        volumes,
                        fmt_bytes(d.free),
                        fmt_bytes(d.total)
                    )
                })
                .unwrap_or_default(),
                fmt_percent(disk.map(|d| d.active).unwrap_or(0.0)),
            )
        }
        Resource::Network(i) => {
            let net = snapshot.nets.get(i);
            (
                net.map(|n| n.name.clone()).unwrap_or_default(),
                net.map(|n| format!("{}  ·  канал {}", n.description, fmt_speed(n.link_bps)))
                    .unwrap_or_default(),
                net.map(|n| format!("↓ {}  ↑ {}", fmt_rate(n.recv_bps), fmt_rate(n.send_bps)))
                    .unwrap_or_default(),
            )
        }
        Resource::Gpu => (
            "Графический процессор 0".to_string(),
            format!("{}  ·  выделено {}", s.gpu_name, fmt_bytes(s.gpu_dedicated)),
            fmt_percent(s.gpu),
        ),
    };

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(title).size(21.0).color(palette.text_strong));
            ui.label(RichText::new(subtitle).size(11.5).color(palette.text_dim));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(big).size(26.0).color(palette.text_strong));
        });
    });
    ui.add_space(6.0);

    let graph_height = (ui.available_height() * 0.35).clamp(120.0, 220.0);
    let rect = graph_rect(ui, graph_height);

    match res {
        Resource::Cpu => {
            let cpu: Vec<f32> = snapshot.history.iter().map(|h| h.cpu).collect();
            graph::draw(
                ui,
                rect,
                &[(&cpu, palette.accent)],
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
                    ("Ядра", format!("{} логических", s.cores_logical)),
                    ("Процессы", s.processes.to_string()),
                    ("Потоки", s.threads.to_string()),
                    ("Дескрипторы", s.handles.to_string()),
                    ("Время работы", fmt_uptime(s.uptime_secs)),
                    ("В режиме ядра", format!("{:.1}%", s.cpu_kernel)),
                ],
            );
            ui.add_space(8.0);
            core_grid(ui, &palette, snapshot);
        }
        Resource::Memory => {
            let mem: Vec<f32> = snapshot.history.iter().map(|h| h.mem).collect();
            graph::draw(
                ui,
                rect,
                &[(&mem, palette.mem)],
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
                        format!(
                            "{} / {}",
                            fmt_bytes(s.mem_committed),
                            fmt_bytes(s.mem_commit_limit)
                        ),
                    ),
                    (
                        "Пулы",
                        format!(
                            "{} / {}",
                            fmt_bytes(s.mem_paged_pool),
                            fmt_bytes(s.mem_nonpaged_pool)
                        ),
                    ),
                ],
            );
        }
        Resource::Disk(i) => {
            let hist = snapshot.disk_history.get(i).cloned().unwrap_or_default();
            graph::draw(
                ui,
                rect,
                &[(&hist, palette.disk)],
                Some(100.0),
                &palette,
                true,
                true,
            );
            ui.add_space(8.0);
            let d = snapshot.disks.get(i);
            stat_row(
                ui,
                &palette,
                &[
                    (
                        "Активное время",
                        fmt_percent(d.map(|d| d.active).unwrap_or(0.0)),
                    ),
                    ("Чтение", fmt_rate(d.map(|d| d.read_bps).unwrap_or(0.0))),
                    ("Запись", fmt_rate(d.map(|d| d.write_bps).unwrap_or(0.0))),
                    (
                        "Средний отклик",
                        format!("{:.2} мс", d.map(|d| d.response_ms).unwrap_or(0.0)),
                    ),
                    ("Ёмкость", fmt_bytes(d.map(|d| d.total).unwrap_or(0))),
                    ("Свободно", fmt_bytes(d.map(|d| d.free).unwrap_or(0))),
                ],
            );
        }
        Resource::Network(i) => {
            let recv = snapshot
                .net_recv_history
                .get(i)
                .cloned()
                .unwrap_or_default();
            let send = snapshot
                .net_send_history
                .get(i)
                .cloned()
                .unwrap_or_default();
            graph::draw(
                ui,
                rect,
                &[(&recv, palette.net), (&send, palette.warn)],
                None,
                &palette,
                true,
                true,
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                legend_dot(
                    ui,
                    palette.net,
                    &format!(
                        "Приём {}",
                        fmt_rate(snapshot.nets.get(i).map(|n| n.recv_bps).unwrap_or(0.0))
                    ),
                );
                legend_dot(
                    ui,
                    palette.warn,
                    &format!(
                        "Отправка {}",
                        fmt_rate(snapshot.nets.get(i).map(|n| n.send_bps).unwrap_or(0.0))
                    ),
                );
            });
            ui.add_space(8.0);
            let n = snapshot.nets.get(i);
            stat_row(
                ui,
                &palette,
                &[
                    ("Приём", fmt_rate(n.map(|n| n.recv_bps).unwrap_or(0.0))),
                    ("Отправка", fmt_rate(n.map(|n| n.send_bps).unwrap_or(0.0))),
                    ("Канал", fmt_speed(n.map(|n| n.link_bps).unwrap_or(0))),
                    (
                        "Адаптер",
                        n.map(|n| n.description.clone()).unwrap_or_default(),
                    ),
                ],
            );
        }
        Resource::Gpu => {
            let gpu: Vec<f32> = snapshot.history.iter().map(|h| h.gpu).collect();
            graph::draw(
                ui,
                rect,
                &[(&gpu, palette.gpu)],
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
    let _ = app;
}

fn stat_row(ui: &mut egui::Ui, palette: &Palette, items: &[(&str, String)]) {
    let card = 136.0;
    let gap = 6.0;
    let per_row =
        (((ui.available_width() + gap) / (card + 22.0 + gap)).floor() as usize).clamp(1, 4);
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

fn composition(ui: &mut egui::Ui, palette: &Palette, s: &SystemStats) {
    let total = s.mem_total.max(1);
    let parts = [
        (s.mem_used as f32 / total as f32 * 100.0, palette.accent),
        (s.mem_cached as f32 / total as f32 * 100.0, palette.mem),
        (s.mem_free as f32 / total as f32 * 100.0, palette.border),
    ];
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), Sense::hover());
    fill_bg(ui, rect, palette);
    let inner = rect.shrink(3.0);
    graph::stacked_bar(ui, inner, &parts);
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        legend_dot(ui, palette.accent, "В работе");
        legend_dot(ui, palette.mem, "Кэш");
        legend_dot(ui, palette.border, "Свободно");
        legend_dot(ui, palette.warn, "Изменённые");
        dim_label(
            ui,
            palette,
            &format!("изменённые {}", fmt_bytes(s.mem_modified)),
        );
    });
}

fn core_grid(ui: &mut egui::Ui, palette: &Palette, snapshot: &Snapshot) {
    let cores = snapshot.core_history.len();
    if cores == 0 {
        return;
    }
    let remaining = ui.available_height();
    if remaining < 70.0 {
        return;
    }
    let columns = 2usize;
    let gap = 6.0;
    let cell_w = (ui.available_width() - gap * (columns as f32 - 1.0)) / columns as f32;
    let rows = cores.div_ceil(columns);
    let cell_h =
        ((remaining.min(140.0) - gap * (rows as f32 - 1.0)) / rows as f32).clamp(38.0, 52.0);
    ui.add_space(1.0);
    ui.separator();
    ui.add_space(4.0);

    for chunk_start in (0..cores).step_by(columns) {
        ui.horizontal(|ui| {
            for (i, hist) in snapshot
                .core_history
                .iter()
                .enumerate()
                .skip(chunk_start)
                .take(columns)
            {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(cell_w, cell_h), Sense::hover());
                let last = hist.last().copied().unwrap_or(0.0);
                crate::theme::text_clipped(
                    ui,
                    rect,
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
