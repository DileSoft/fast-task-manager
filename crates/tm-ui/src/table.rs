//! Вкладка «Процессы»: дерево, группы, столбцы, отрисовка строк.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui};
use tm_core::{fmt_bytes, fmt_percent, fmt_rate, GroupKind, ProcInfo, ProcStatus, Snapshot};

use crate::icons::IconCache;
use crate::theme::{self, bold, font, Palette};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Col {
    Name,
    Status,
    Cpu,
    Memory,
    Disk,
    Network,
    Gpu,
    User,
    Threads,
    Pid,
    Path,
    CommandLine,
    Description,
}

impl Col {
    pub fn label(self) -> &'static str {
        match self {
            Col::Name => "Имя",
            Col::Status => "Состояние",
            Col::Cpu => "ЦП",
            Col::Memory => "Память",
            Col::Disk => "Диск",
            Col::Network => "Сеть",
            Col::Gpu => "ГП",
            Col::User => "Пользователь",
            Col::Threads => "Потоки",
            Col::Pid => "PID",
            Col::Path => "Путь к файлу",
            Col::CommandLine => "Командная строка",
            Col::Description => "Описание",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Col::Name => "name",
            Col::Status => "status",
            Col::Cpu => "cpu",
            Col::Memory => "memory",
            Col::Disk => "disk",
            Col::Network => "network",
            Col::Gpu => "gpu",
            Col::User => "user",
            Col::Threads => "threads",
            Col::Pid => "pid",
            Col::Path => "path",
            Col::CommandLine => "command_line",
            Col::Description => "description",
        }
    }

    pub fn from_key(key: &str) -> Option<Col> {
        Some(match key {
            "name" => Col::Name,
            "status" => Col::Status,
            "cpu" => Col::Cpu,
            "memory" => Col::Memory,
            "disk" => Col::Disk,
            "network" => Col::Network,
            "gpu" => Col::Gpu,
            "user" => Col::User,
            "threads" => Col::Threads,
            "pid" => Col::Pid,
            "path" => Col::Path,
            "command_line" => Col::CommandLine,
            "description" => Col::Description,
            _ => return None,
        })
    }

    pub fn all() -> [Col; 13] {
        [
            Col::Name,
            Col::Status,
            Col::Cpu,
            Col::Memory,
            Col::Disk,
            Col::Network,
            Col::Gpu,
            Col::User,
            Col::Threads,
            Col::Pid,
            Col::Path,
            Col::CommandLine,
            Col::Description,
        ]
    }

    pub fn width(self) -> f32 {
        match self {
            Col::Name => 240.0,
            Col::Status => 92.0,
            Col::Cpu => 64.0,
            Col::Memory => 92.0,
            Col::Disk => 82.0,
            Col::Network => 82.0,
            Col::Gpu => 58.0,
            Col::User => 128.0,
            Col::Threads => 62.0,
            Col::Pid => 64.0,
            Col::Path => 260.0,
            Col::CommandLine => 300.0,
            Col::Description => 180.0,
        }
    }

    pub fn numeric(self) -> bool {
        matches!(
            self,
            Col::Cpu | Col::Memory | Col::Disk | Col::Network | Col::Gpu | Col::Threads | Col::Pid
        )
    }

    pub fn flexible(self) -> bool {
        matches!(
            self,
            Col::Name | Col::Path | Col::CommandLine | Col::Description
        )
    }
}

pub const DEFAULT_COLUMNS: [Col; 7] = [
    Col::Name,
    Col::Status,
    Col::Cpu,
    Col::Memory,
    Col::Disk,
    Col::Network,
    Col::Gpu,
];

/// Раскладка столбцов по доступной ширине: имя и описание тянутся.
pub fn layout(available: f32, cols: &[Col]) -> Vec<(Col, f32)> {
    let mut fixed = 0.0;
    let mut flex_count = 0;
    for c in cols {
        if c.flexible() {
            flex_count += 1;
        } else {
            fixed += c.width();
        }
    }
    let mut flex_width = ((available - fixed - 8.0) / flex_count.max(1) as f32).max(120.0);
    if flex_count == 0 {
        flex_width = 0.0;
    }
    cols.iter()
        .map(|c| {
            let w = if c.flexible() { flex_width } else { c.width() };
            (*c, w)
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
pub enum Row {
    Group {
        kind: GroupKind,
        cpu: f32,
        mem: u64,
        count: u32,
        expanded: bool,
    },
    Proc {
        idx: u32,
        depth: u8,
        children: u32,
        expanded: bool,
    },
}

#[derive(Clone, Debug)]
pub enum Action {
    EndTask(u32, Option<isize>),
    EndTree(u32),
    OpenLocation(Arc<str>),
    FocusWindow(isize),
    Properties(u32),
    Copy(String),
    Elevate,
}

pub struct View<'a> {
    pub snapshot: &'a Snapshot,
    pub sort: Col,
    pub desc: bool,
    pub search: &'a str,
    pub group_by_type: bool,
    pub group_hierarchy: bool,
    pub collapsed_groups: &'a HashSet<GroupKind>,
    pub expansion: &'a HashMap<u32, bool>,
}

pub fn build_rows(view: &View<'_>) -> Vec<Row> {
    let snapshot = view.snapshot;
    let mut indices: Vec<u32> = Vec::with_capacity(snapshot.procs.len());
    for (i, p) in snapshot.procs.iter().enumerate() {
        if p.pid == 0 {
            continue; // System Idle Process в списке не показываем
        }
        indices.push(i as u32);
    }

    let search = view.search.trim().to_lowercase();
    if !search.is_empty() {
        let mut filtered: Vec<u32> = indices
            .into_iter()
            .filter(|i| {
                let p = &snapshot.procs[*i as usize];
                p.name.to_lowercase().contains(&search)
                    || p.pid.to_string().contains(&search)
                    || p.title().to_lowercase().contains(&search)
                    || p.description
                        .as_deref()
                        .map(|d| d.to_lowercase().contains(&search))
                        .unwrap_or(false)
            })
            .collect();
        sort_indices(&mut filtered, snapshot, view.sort, view.desc);
        return filtered
            .into_iter()
            .map(|idx| Row::Proc {
                idx,
                depth: 0,
                children: 0,
                expanded: false,
            })
            .collect();
    }

    let order = [GroupKind::Apps, GroupKind::Background, GroupKind::Windows];
    let mut rows = Vec::with_capacity(indices.len() + 3);

    if !view.group_by_type {
        if !view.group_hierarchy {
            let mut flat = indices;
            sort_indices(&mut flat, snapshot, view.sort, view.desc);
            return flat
                .into_iter()
                .map(|idx| Row::Proc {
                    idx,
                    depth: 0,
                    children: 0,
                    expanded: false,
                })
                .collect();
        }
        let mut roots = build_tree(&indices, snapshot);
        sort_indices(&mut roots, snapshot, view.sort, view.desc);
        for idx in roots {
            push_tree(idx, &indices, snapshot, view, 0, &mut rows);
        }
        return rows;
    }

    for kind in order {
        let mut members: Vec<u32> = indices
            .iter()
            .copied()
            .filter(|i| snapshot.procs[*i as usize].group == kind)
            .collect();
        if members.is_empty() {
            continue;
        }
        let expanded = !view.collapsed_groups.contains(&kind);
        let cpu = members
            .iter()
            .map(|i| snapshot.procs[*i as usize].cpu)
            .sum::<f32>()
            .clamp(0.0, 100.0);
        let mem = members
            .iter()
            .map(|i| snapshot.procs[*i as usize].working_set)
            .sum();
        rows.push(Row::Group {
            kind,
            cpu,
            mem,
            count: members.len() as u32,
            expanded,
        });
        if !expanded {
            continue;
        }

        if view.group_hierarchy {
            let mut roots = build_tree(&members, snapshot);
            sort_indices(&mut roots, snapshot, view.sort, view.desc);
            for idx in roots {
                push_tree(idx, &members, snapshot, view, 0, &mut rows);
            }
        } else {
            sort_indices(&mut members, snapshot, view.sort, view.desc);
            for idx in members {
                rows.push(Row::Proc {
                    idx,
                    depth: 0,
                    children: 0,
                    expanded: false,
                });
            }
        }
    }

    rows
}

/// Корни дерева: процессы, чей родитель отсутствует в наборе (или сам себе родитель).
fn build_tree(indices: &[u32], snapshot: &Snapshot) -> Vec<u32> {
    let present: HashSet<u32> = indices
        .iter()
        .map(|i| snapshot.procs[*i as usize].pid)
        .collect();
    indices
        .iter()
        .copied()
        .filter(|i| {
            let p = &snapshot.procs[*i as usize];
            p.parent_pid == p.pid || !present.contains(&p.parent_pid)
        })
        .collect()
}

fn children_of(pid: u32, indices: &[u32], snapshot: &Snapshot) -> Vec<u32> {
    indices
        .iter()
        .copied()
        .filter(|i| {
            let p = &snapshot.procs[*i as usize];
            p.parent_pid == pid && p.pid != pid
        })
        .collect()
}

fn push_tree(
    idx: u32,
    indices: &[u32],
    snapshot: &Snapshot,
    view: &View<'_>,
    depth: u8,
    rows: &mut Vec<Row>,
) {
    let proc = &snapshot.procs[idx as usize];
    let mut children = children_of(proc.pid, indices, snapshot);
    if children.is_empty() {
        rows.push(Row::Proc {
            idx,
            depth,
            children: 0,
            expanded: false,
        });
        return;
    }
    sort_indices(&mut children, snapshot, view.sort, view.desc);
    let default_open = proc.group != GroupKind::Apps;
    let expanded = view
        .expansion
        .get(&proc.pid)
        .copied()
        .unwrap_or(default_open);
    rows.push(Row::Proc {
        idx,
        depth,
        children: children.len() as u32,
        expanded,
    });
    if expanded {
        for child in children {
            push_tree(
                child,
                indices,
                snapshot,
                view,
                depth.saturating_add(1),
                rows,
            );
        }
    }
}

fn sort_indices(indices: &mut [u32], snapshot: &Snapshot, col: Col, desc: bool) {
    indices.sort_by(|a, b| {
        let pa = &snapshot.procs[*a as usize];
        let pb = &snapshot.procs[*b as usize];
        let ord = compare(pa, pb, col);
        if desc {
            ord.reverse()
        } else {
            ord
        }
    });
}

pub fn compare(a: &ProcInfo, b: &ProcInfo, col: Col) -> Ordering {
    let name_ord = || {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.pid.cmp(&b.pid))
    };
    match col {
        Col::Name | Col::Description => name_ord(),
        Col::Status => status_rank(a.status)
            .cmp(&status_rank(b.status))
            .then_with(name_ord),
        Col::Cpu => a
            .cpu
            .partial_cmp(&b.cpu)
            .unwrap_or(Ordering::Equal)
            .then_with(name_ord),
        Col::Memory => a.working_set.cmp(&b.working_set).then_with(name_ord),
        Col::Disk => (a.disk_read_bps + a.disk_write_bps)
            .partial_cmp(&(b.disk_read_bps + b.disk_write_bps))
            .unwrap_or(Ordering::Equal)
            .then_with(name_ord),
        Col::Network => (a.net_recv_bps + a.net_send_bps)
            .partial_cmp(&(b.net_recv_bps + b.net_send_bps))
            .unwrap_or(Ordering::Equal)
            .then_with(name_ord),
        Col::Gpu => a
            .gpu
            .partial_cmp(&b.gpu)
            .unwrap_or(Ordering::Equal)
            .then_with(name_ord),
        Col::User => a
            .user
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .cmp(&b.user.as_deref().unwrap_or("").to_lowercase())
            .then_with(name_ord),
        Col::Threads => a.threads.cmp(&b.threads).then_with(name_ord),
        Col::Pid => a.pid.cmp(&b.pid),
        Col::Path => a
            .exe_path
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .cmp(&b.exe_path.as_deref().unwrap_or("").to_lowercase())
            .then_with(name_ord),
        Col::CommandLine => a
            .cmdline
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .cmp(&b.cmdline.as_deref().unwrap_or("").to_lowercase())
            .then_with(name_ord),
    }
}

fn status_rank(s: ProcStatus) -> u8 {
    match s {
        ProcStatus::Running => 0,
        ProcStatus::Suspended => 1,
        ProcStatus::NotResponding => 2,
    }
}

/// Отрисовка заголовка. Возвращает столбец, по которому кликнули.
pub fn header(
    ui: &mut Ui,
    palette: &Palette,
    cols: &[(Col, f32)],
    sort: Col,
    desc: bool,
) -> Option<Col> {
    let width = cols.iter().map(|(_, w)| *w).sum::<f32>() + 8.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, theme::header_h()), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, palette.header);
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0_f32, palette.border),
    );

    let mut x = rect.left() + 2.0;
    let mut clicked = None;
    let row_rect = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top()),
        Pos2::new(rect.right(), rect.bottom() - 1.0),
    );
    for (col, w) in cols {
        let cell = Rect::from_min_size(Pos2::new(x, rect.top()), egui::vec2(*w, row_rect.height()));
        let resp = ui.interact(cell, egui::Id::new(("hdr", *col as u8)), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(cell, 0.0, palette.hover);
        }
        let align = if col.numeric() {
            Align2::RIGHT_CENTER
        } else {
            Align2::LEFT_CENTER
        };
        let pos = if col.numeric() {
            cell.right_center() - egui::vec2(8.0, 0.0)
        } else {
            cell.left_center() + egui::vec2(8.0, 0.0)
        };
        let color = if *col == sort {
            palette.text_strong
        } else {
            palette.text
        };
        theme::text_clipped(ui, cell, pos, align, col.label(), font(12.5), color);
        if *col == sort {
            let cx = if col.numeric() {
                cell.right() - 3.0
            } else {
                cell.left() + 3.0
            };
            let cy = cell.center().y;
            let dy = 2.0;
            let tri = if desc {
                vec![
                    Pos2::new(cx - 3.0, cy - dy),
                    Pos2::new(cx + 3.0, cy - dy),
                    Pos2::new(cx, cy + dy),
                ]
            } else {
                vec![
                    Pos2::new(cx - 3.0, cy + dy),
                    Pos2::new(cx + 3.0, cy + dy),
                    Pos2::new(cx, cy - dy),
                ]
            };
            ui.painter().add(egui::Shape::convex_polygon(
                tri,
                palette.text_strong,
                Stroke::NONE,
            ));
        }
        if resp.clicked() {
            clicked = Some(*col);
        }
        x += w;
    }
    clicked
}

pub struct RowOut {
    pub select: Option<u32>,
    pub toggle_proc: Option<u32>,
    pub toggle_group: Option<GroupKind>,
    pub actions: Vec<Action>,
    pub open_properties: Option<u32>,
}

impl RowOut {
    pub fn new() -> Self {
        Self {
            select: None,
            toggle_proc: None,
            toggle_group: None,
            actions: Vec::new(),
            open_properties: None,
        }
    }
}

pub struct DrawCtx<'a> {
    pub palette: &'a Palette,
    pub snapshot: &'a Snapshot,
    pub cols: &'a [(Col, f32)],
    pub selected: Option<u32>,
    pub icons: &'a mut IconCache,
}

pub fn draw_row(ui: &mut Ui, ctx: &mut DrawCtx<'_>, row: Row, out: &mut RowOut) {
    let width = ctx.cols.iter().map(|(_, w)| *w).sum::<f32>() + 8.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, theme::row_h()), Sense::click());
    let palette = ctx.palette;

    let hovered = resp.hovered();
    let is_selected = match row {
        Row::Proc { idx, .. } => ctx.selected == Some(ctx.snapshot.procs[idx as usize].pid),
        _ => false,
    };

    let bg = match row {
        Row::Group { .. } => palette.group,
        Row::Proc { .. } => {
            if is_selected {
                palette.selected
            } else if hovered {
                palette.hover
            } else {
                palette.panel
            }
        }
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, bg);
    if resp.hovered() && !is_selected {
        painter.rect_filled(rect, 0.0, palette.hover.gamma_multiply(0.7));
    }

    match row {
        Row::Group {
            kind,
            cpu,
            mem,
            count,
            expanded,
        } => {
            let name_rect =
                Rect::from_min_size(rect.min, egui::vec2(ctx.cols[0].1, theme::row_h()));
            draw_chevron(
                ui,
                palette,
                name_rect.left_center() + egui::vec2(10.0, 0.0),
                expanded,
                true,
            );
            theme::text_clipped(
                ui,
                name_rect,
                Pos2::new(name_rect.left() + 24.0, name_rect.center().y),
                Align2::LEFT_CENTER,
                format!("{} ({})", kind.label(), count),
                bold(13.5),
                palette.text_strong,
            );
            let mut x = rect.left() + 2.0 + ctx.cols[0].1;
            for (col, w) in &ctx.cols[1..] {
                let cell =
                    Rect::from_min_size(Pos2::new(x, rect.top()), egui::vec2(*w, theme::row_h()));
                let text = match col {
                    Col::Cpu => fmt_percent(cpu),
                    Col::Memory => fmt_bytes(mem),
                    _ => String::new(),
                };
                if !text.is_empty() {
                    theme::text_clipped(
                        ui,
                        cell,
                        Pos2::new(cell.right() - 8.0, cell.center().y),
                        Align2::RIGHT_CENTER,
                        text,
                        font(13.0),
                        palette.text_dim,
                    );
                }
                x += w;
            }
            if resp.clicked() {
                out.toggle_group = Some(kind);
            }
        }
        Row::Proc {
            idx,
            depth,
            children,
            expanded,
        } => {
            let proc = &ctx.snapshot.procs[idx as usize];
            let mut x = rect.left() + 2.0;

            // Стрелка дерева.
            let chevron_x = x + 6.0 + depth as f32 * 12.0;
            if children > 0 {
                draw_chevron(
                    ui,
                    palette,
                    Pos2::new(chevron_x + 4.0, rect.center().y),
                    expanded,
                    false,
                );
            }
            x += 14.0 + depth as f32 * 12.0;

            for (col, w) in ctx.cols {
                let cell =
                    Rect::from_min_size(Pos2::new(x, rect.top()), egui::vec2(*w, theme::row_h()));
                match col {
                    Col::Name => {
                        // Иконка.
                        let mut text_x = cell.left() + 2.0;
                        if let Some(p) = &proc.exe_path {
                            if let Some(tex) = ctx.icons.get(ui.ctx(), p) {
                                let icon_rect = Rect::from_center_size(
                                    Pos2::new(text_x + 8.0, cell.center().y),
                                    egui::vec2(16.0, 16.0),
                                );
                                ui.painter().image(
                                    tex,
                                    icon_rect,
                                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                    Color32::WHITE,
                                );
                                text_x += 22.0;
                            }
                        }
                        let color = palette.text;
                        theme::text_clipped(
                            ui,
                            cell,
                            Pos2::new(text_x, cell.center().y),
                            Align2::LEFT_CENTER,
                            proc.task_name(),
                            font(13.0),
                            color,
                        );
                    }
                    Col::Status => {
                        let color = match proc.status {
                            ProcStatus::NotResponding => palette.warn,
                            ProcStatus::Suspended => palette.text_dim,
                            ProcStatus::Running => palette.text,
                        };
                        theme::text_clipped(
                            ui,
                            cell,
                            Pos2::new(cell.left() + 8.0, cell.center().y),
                            Align2::LEFT_CENTER,
                            proc.status.label(),
                            font(12.5),
                            color,
                        );
                    }
                    Col::Cpu => {
                        let color = if proc.cpu >= 1.0 {
                            palette.text
                        } else {
                            palette.text_dim
                        };
                        theme::text_clipped(
                            ui,
                            cell,
                            Pos2::new(cell.right() - 8.0, cell.center().y),
                            Align2::RIGHT_CENTER,
                            fmt_percent(proc.cpu),
                            font(13.0),
                            color,
                        );
                    }
                    Col::Memory => {
                        theme::text_clipped(
                            ui,
                            cell,
                            Pos2::new(cell.right() - 8.0, cell.center().y),
                            Align2::RIGHT_CENTER,
                            fmt_bytes(proc.working_set),
                            font(13.0),
                            palette.text,
                        );
                    }
                    Col::Disk => {
                        let v = proc.disk_read_bps + proc.disk_write_bps;
                        theme::text_clipped(
                            ui,
                            cell,
                            Pos2::new(cell.right() - 8.0, cell.center().y),
                            Align2::RIGHT_CENTER,
                            fmt_rate(v),
                            font(13.0),
                            palette.text,
                        );
                    }
                    Col::Network => {
                        let v = proc.net_recv_bps + proc.net_send_bps;
                        theme::text_clipped(
                            ui,
                            cell,
                            Pos2::new(cell.right() - 8.0, cell.center().y),
                            Align2::RIGHT_CENTER,
                            fmt_rate(v),
                            font(13.0),
                            palette.text,
                        );
                    }
                    Col::Gpu => {
                        if proc.gpu > 0.0 {
                            theme::text_clipped(
                                ui,
                                cell,
                                Pos2::new(cell.right() - 8.0, cell.center().y),
                                Align2::RIGHT_CENTER,
                                fmt_percent(proc.gpu),
                                font(13.0),
                                palette.text,
                            );
                        }
                    }
                    Col::User => {
                        theme::text_clipped(
                            ui,
                            cell,
                            Pos2::new(cell.left() + 8.0, cell.center().y),
                            Align2::LEFT_CENTER,
                            proc.user.as_deref().unwrap_or(""),
                            font(12.5),
                            palette.text_dim,
                        );
                    }
                    Col::Threads => {
                        theme::text_clipped(
                            ui,
                            cell,
                            Pos2::new(cell.right() - 8.0, cell.center().y),
                            Align2::RIGHT_CENTER,
                            proc.threads.to_string(),
                            font(12.5),
                            palette.text_dim,
                        );
                    }
                    Col::Pid => {
                        theme::text_clipped(
                            ui,
                            cell,
                            Pos2::new(cell.right() - 8.0, cell.center().y),
                            Align2::RIGHT_CENTER,
                            proc.pid.to_string(),
                            font(12.5),
                            palette.text_dim,
                        );
                    }
                    Col::Path => {
                        theme::text_clipped(
                            ui,
                            cell,
                            Pos2::new(cell.left() + 8.0, cell.center().y),
                            Align2::LEFT_CENTER,
                            proc.exe_path.as_deref().unwrap_or(""),
                            font(12.5),
                            palette.text_dim,
                        );
                    }
                    Col::CommandLine => {
                        theme::text_clipped(
                            ui,
                            cell,
                            Pos2::new(cell.left() + 8.0, cell.center().y),
                            Align2::LEFT_CENTER,
                            proc.cmdline.as_deref().unwrap_or(""),
                            font(12.5),
                            palette.text_dim,
                        );
                    }
                    Col::Description => {
                        theme::text_clipped(
                            ui,
                            cell,
                            Pos2::new(cell.left() + 8.0, cell.center().y),
                            Align2::LEFT_CENTER,
                            proc.description.as_deref().unwrap_or(""),
                            font(12.5),
                            palette.text_dim,
                        );
                    }
                }
                x += w;
            }

            let clicking_chevron = resp.clicked() && {
                let pos = resp.interact_pointer_pos().unwrap_or(rect.center());
                children > 0 && (pos.x - chevron_x).abs() < 11.0
            };

            if clicking_chevron {
                out.toggle_proc = Some(proc.pid);
            } else if resp.clicked() {
                out.select = Some(proc.pid);
            }
            if resp.double_clicked() {
                out.open_properties = Some(proc.pid);
            }
            if resp.secondary_clicked() {
                out.select = Some(proc.pid);
            }

            // Контекстное меню.
            let pid = proc.pid;
            let path = proc.exe_path.clone();
            let name = proc.name.clone();
            let hwnd = proc.window;
            let actions = &mut out.actions;
            resp.context_menu(|ui| {
                if ui.button("Снять задачу").clicked() {
                    actions.push(Action::EndTask(pid, hwnd));
                    ui.close_menu();
                }
                if ui.button("Снять дерево процессов").clicked() {
                    actions.push(Action::EndTree(pid));
                    ui.close_menu();
                }
                ui.separator();
                if ui
                    .add_enabled(
                        path.is_some(),
                        egui::Button::new("Открыть расположение файла"),
                    )
                    .clicked()
                {
                    if let Some(path) = path.clone() {
                        actions.push(Action::OpenLocation(path));
                    }
                    ui.close_menu();
                }
                if ui
                    .add_enabled(hwnd.is_some(), egui::Button::new("Переключиться на окно"))
                    .clicked()
                {
                    if let Some(hwnd) = hwnd {
                        actions.push(Action::FocusWindow(hwnd));
                    }
                    ui.close_menu();
                }
                if ui.button("Свойства").clicked() {
                    actions.push(Action::Properties(pid));
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Копировать имя").clicked() {
                    actions.push(Action::Copy(name.clone()));
                    ui.close_menu();
                }
                if ui.button("Копировать PID").clicked() {
                    actions.push(Action::Copy(pid.to_string()));
                    ui.close_menu();
                }
                if let Some(path) = path.clone() {
                    if ui.button("Копировать путь").clicked() {
                        actions.push(Action::Copy(path.to_string()));
                        ui.close_menu();
                    }
                }
            });
        }
    }

    let _ = ui.allocate_space(egui::vec2(0.0, 0.0));
}

fn draw_chevron(ui: &Ui, palette: &Palette, center: Pos2, expanded: bool, large: bool) {
    let s = if large { 5.0 } else { 4.0 };
    let points = if expanded {
        vec![
            Pos2::new(center.x - s, center.y - s * 0.5),
            Pos2::new(center.x + s, center.y - s * 0.5),
            Pos2::new(center.x, center.y + s * 0.7),
        ]
    } else {
        vec![
            Pos2::new(center.x - s * 0.5, center.y - s),
            Pos2::new(center.x - s * 0.5, center.y + s),
            Pos2::new(center.x + s * 0.7, center.y),
        ]
    };
    ui.painter().add(egui::Shape::convex_polygon(
        points,
        palette.text_dim,
        Stroke::NONE,
    ));
}

/// Полоска подсказки, когда ETW-данные недоступны.
pub fn draw_banner(ui: &mut Ui, palette: &Palette, text: &str) -> bool {
    let rect = ui.available_rect_before_wrap();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(rect.width(), theme::pt(30.0)), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, palette.header);
    ui.painter().rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, palette.border),
        StrokeKind::Inside,
    );
    let button = Rect::from_min_size(
        Pos2::new(rect.right() - 216.0, rect.top() + 4.0),
        egui::vec2(206.0, 22.0),
    );
    let text_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 8.0, rect.top()),
        Pos2::new(button.left() - 8.0, rect.bottom()),
    );
    theme::text_clipped(
        ui,
        text_rect,
        Pos2::new(text_rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        text,
        font(12.0),
        palette.text_dim,
    );
    let btn = ui.interact(button, egui::Id::new("etw-elevate"), Sense::click());
    let bg = if btn.hovered() {
        palette.button_hover
    } else {
        palette.button
    };
    ui.painter().rect_filled(button, 2.0, bg);
    theme::text(
        ui,
        button.center(),
        Align2::CENTER_CENTER,
        "Перезапустить как администратор",
        font(12.0),
        palette.text,
    );
    btn.clicked()
}

/// Кнопка «столбцы» и меню выбора.
pub fn columns_menu(ui: &mut Ui, palette: &Palette, columns: &mut Vec<Col>) {
    egui::menu::menu_button(ui, "Столбцы", |ui| {
        for col in Col::all() {
            let mut on = columns.contains(&col);
            if ui.checkbox(&mut on, col.label()).clicked() {
                if on {
                    if !columns.contains(&col) {
                        columns.push(col);
                    }
                } else if columns.len() > 1 {
                    columns.retain(|c| *c != col);
                }
            }
        }
        let _ = palette;
    });
}
