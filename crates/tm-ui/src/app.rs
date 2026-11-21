//! Главное окно: вкладки, панель инструментов, таблица процессов, диалоги.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Align2, Layout, Pos2, RichText, Sense, Stroke, StrokeKind, Ui};
use tm_core::{
    fmt_bytes, fmt_cpu_time, fmt_filetime_local, fmt_percent, sys::actions, sys::window, GroupKind,
    ProcInfo, Sampler, Snapshot,
};

use crate::icons::IconCache;
use crate::perf::{self, Resource};
use crate::settings::Settings;
use crate::table::{self, Action, Col};
use crate::theme::{self, bold, font, Palette};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Processes,
    Performance,
}

pub struct PendingKill {
    pub pid: u32,
    pub hwnd: Option<isize>,
    pub name: String,
}

pub struct App {
    pub sampler: Sampler,
    pub snapshot: Arc<Snapshot>,
    pub palette: Palette,
    pub dark: bool,
    pub tab: Tab,

    pub columns: Vec<Col>,
    pub sort: Col,
    pub sort_desc: bool,
    pub search: String,
    pub search_id: Option<egui::Id>,
    pub group_by_type: bool,

    pub selection: Option<u32>,
    pub collapsed_groups: HashSet<GroupKind>,
    pub expansion: HashMap<u32, bool>,
    pub rows: Vec<table::Row>,
    pub rows_dirty: bool,
    pub rows_seq: u64,

    pub icons: IconCache,
    pub interval_ms: u64,
    pub paused: bool,

    pub perf_resource: Resource,
    pub properties: Option<u32>,
    properties_cmdline: Option<(u32, Option<String>)>,
    pub pending_kill: Option<PendingKill>,
    pub message: Option<(String, Instant, bool)>,

    pub settings_dirty: bool,
    last_settings_save: Instant,
    cmdline_enabled: bool,
    ctx: egui::Context,
    last_window_size: (f32, f32),
    last_window_pos: Option<(f32, f32)>,
    dark_override: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings) -> Self {
        theme::update_scale(&cc.egui_ctx);
        let dark = settings
            .dark
            .unwrap_or_else(|| !tm_core::sys::registry::apps_use_light_theme());
        let palette = theme::install(&cc.egui_ctx, dark);

        let columns: Vec<Col> = settings
            .columns
            .iter()
            .filter_map(|k| Col::from_key(k))
            .collect();
        let columns = if columns.is_empty() {
            table::DEFAULT_COLUMNS.to_vec()
        } else {
            columns
        };
        let cmdline_enabled = columns.contains(&Col::CommandLine);

        let sampler = Sampler::start(settings.interval_ms.max(100));
        if settings.paused {
            sampler.set_paused(true);
        }
        if cmdline_enabled {
            sampler.set_cmdline_enabled(true);
        }
        let snapshot = sampler.latest();
        let mut app = Self {
            sampler,
            snapshot,
            palette,
            dark,
            tab: if std::env::var("TM_TAB").ok().as_deref() == Some("perf") {
                Tab::Performance
            } else {
                Tab::Processes
            },
            columns,
            sort: Col::from_key(&settings.sort).unwrap_or(Col::Cpu),
            sort_desc: settings.sort_desc,
            search: String::new(),
            search_id: None,
            group_by_type: settings.group_by_type,
            selection: None,
            collapsed_groups: HashSet::new(),
            expansion: HashMap::new(),
            rows: Vec::new(),
            rows_dirty: true,
            rows_seq: 0,
            icons: IconCache::default(),
            interval_ms: settings.interval_ms.max(100),
            paused: settings.paused,
            perf_resource: Resource::from_parts(&settings.perf_kind, settings.perf_index),
            properties: None,
            properties_cmdline: None,
            pending_kill: None,
            message: None,
            settings_dirty: true,
            last_settings_save: Instant::now(),
            cmdline_enabled,
            ctx: cc.egui_ctx.clone(),
            last_window_size: (settings.window_w.max(760.0), settings.window_h.max(460.0)),
            last_window_pos: match (settings.window_x, settings.window_y) {
                (Some(x), Some(y)) => Some((x, y)),
                _ => None,
            },
            dark_override: settings.dark.is_some(),
        };
        app.rebuild_rows();
        if let Ok(pid) = std::env::var("TM_PROPERTIES_PID") {
            if let Ok(pid) = pid.parse::<u32>() {
                app.properties = Some(pid);
                app.properties_cmdline = Some((pid, tm_core::sys::identity::command_line(pid)));
            }
        }
        app
    }

    fn current_settings(&self, ctx: &egui::Context) -> Settings {
        let (mut window_w, mut window_h) = self.last_window_size;
        if let Some(rect) = ctx.input(|i| i.viewport().inner_rect) {
            window_w = rect.width();
            window_h = rect.height();
        }
        Settings {
            columns: self.columns.iter().map(|c| c.key().to_string()).collect(),
            sort: self.sort.key().to_string(),
            sort_desc: self.sort_desc,
            group_by_type: self.group_by_type,
            interval_ms: self.interval_ms,
            paused: self.paused,
            dark: if self.dark_override {
                Some(self.dark)
            } else {
                None
            },
            perf_kind: self.perf_resource.kind().to_string(),
            perf_index: self.perf_resource.index(),
            window_w,
            window_h,
            window_x: self.last_window_pos.map(|p| p.0),
            window_y: self.last_window_pos.map(|p| p.1),
        }
    }

    fn save_settings(&mut self, ctx: &egui::Context) {
        self.current_settings(ctx).save();
        self.settings_dirty = false;
        self.last_settings_save = Instant::now();
    }

    fn sync_cmdline(&mut self) {
        let wanted = self.columns.contains(&Col::CommandLine);
        if wanted != self.cmdline_enabled {
            self.cmdline_enabled = wanted;
            self.sampler.set_cmdline_enabled(wanted);
        }
    }

    pub fn selected_proc(&self) -> Option<&ProcInfo> {
        let pid = self.selection?;
        self.snapshot.proc_by_pid(pid)
    }

    fn rebuild_rows(&mut self) {
        let view = table::View {
            snapshot: &self.snapshot,
            sort: self.sort,
            desc: self.sort_desc,
            search: &self.search,
            group_by_type: self.group_by_type,
            collapsed_groups: &self.collapsed_groups,
            expansion: &self.expansion,
        };
        self.rows = table::build_rows(&view);
        self.rows_dirty = false;
        self.rows_seq = self.snapshot.seq;
    }

    fn set_message(&mut self, text: impl Into<String>, error: bool) {
        self.message = Some((text.into(), Instant::now(), error));
    }

    fn open_properties(&mut self, pid: u32) {
        let cmdline = if let Some(p) = self.snapshot.proc_by_pid(pid) {
            match &p.cmdline {
                Some(text) => Some(text.to_string()),
                None => tm_core::sys::identity::command_line(pid),
            }
        } else {
            None
        };
        self.properties_cmdline = Some((pid, cmdline));
        self.properties = Some(pid);
    }

    fn apply_action(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::EndTask(pid, hwnd) => self.request_end_task(pid, hwnd),
            Action::EndTree(pid) => match actions::end_task_tree(&self.snapshot.procs, pid) {
                Ok(n) => self.set_message(format!("Завершено процессов: {n}"), false),
                Err(e) => self.set_message(e, true),
            },
            Action::OpenLocation(path) => {
                if let Err(e) = actions::open_file_location(&path) {
                    self.set_message(e, true);
                }
            }
            Action::FocusWindow(hwnd) => {
                if !window::focus_window(hwnd) {
                    self.set_message("Не удалось переключиться на окно процесса", true);
                }
            }
            Action::Properties(pid) => self.open_properties(pid),
            Action::Copy(text) => ctx.copy_text(text),
            Action::Elevate => {
                if let Err(e) = actions::restart_elevated() {
                    self.set_message(e, true);
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    fn request_end_task(&mut self, pid: u32, hwnd: Option<isize>) {
        let proc = self.snapshot.proc_by_pid(pid);
        let critical = pid <= 4
            || proc.map(|p| p.session_id == 0).unwrap_or(false)
            || proc
                .map(|p| {
                    matches!(
                        p.name.to_ascii_lowercase().as_str(),
                        "explorer.exe"
                            | "winlogon.exe"
                            | "csrss.exe"
                            | "wininit.exe"
                            | "services.exe"
                    )
                })
                .unwrap_or(false);
        if critical {
            self.pending_kill = Some(PendingKill {
                pid,
                hwnd,
                name: proc.map(|p| p.name.clone()).unwrap_or_default(),
            });
        } else {
            self.end_task_now(pid, hwnd);
        }
    }

    fn end_task_now(&mut self, pid: u32, hwnd: Option<isize>) {
        match actions::end_task(pid, hwnd) {
            Ok(()) => self.set_message("Задача снята", false),
            Err(e) => self.set_message(e, true),
        }
    }

    fn apply_row_out(&mut self, out: table::RowOut, ctx: &egui::Context) {
        if let Some(pid) = out.select {
            self.selection = Some(pid);
        }
        if let Some(pid) = out.toggle_proc {
            let proc = self.snapshot.proc_by_pid(pid);
            let default_open = proc.map(|p| p.group != GroupKind::Apps).unwrap_or(true);
            let current = self.expansion.get(&pid).copied().unwrap_or(default_open);
            self.expansion.insert(pid, !current);
            self.rows_dirty = true;
        }
        if let Some(kind) = out.toggle_group {
            if !self.collapsed_groups.remove(&kind) {
                self.collapsed_groups.insert(kind);
            }
            self.rows_dirty = true;
        }
        if let Some(pid) = out.open_properties {
            self.open_properties(pid);
        }
        for action in out.actions {
            self.apply_action(action, ctx);
        }
    }

    fn tab_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("tabbar")
            .exact_height(theme::pt(40.0))
            .frame(egui::Frame::NONE.fill(self.palette.bg))
            .show(ctx, |ui| {
                let height = ui.available_height();
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    let tabs = [
                        (Tab::Processes, "Процессы"),
                        (Tab::Performance, "Производительность"),
                    ];
                    for (tab, label) in tabs {
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(
                                if tab == Tab::Performance {
                                    172.0
                                } else {
                                    108.0
                                },
                                height,
                            ),
                            Sense::click(),
                        );
                        let active = self.tab == tab;
                        if resp.hovered() && !active {
                            ui.painter().rect_filled(rect, 0.0, self.palette.hover);
                        }
                        if active {
                            let line = egui::Rect::from_min_size(
                                Pos2::new(rect.left() + 10.0, rect.bottom() - 2.0),
                                egui::vec2(rect.width() - 20.0, 2.0),
                            );
                            ui.painter().rect_filled(line, 0.0, self.palette.accent);
                        }
                        let color = if active {
                            self.palette.text_strong
                        } else {
                            self.palette.text_dim
                        };
                        theme::text(
                            ui,
                            rect.center(),
                            Align2::CENTER_CENTER,
                            label,
                            if active { bold(14.5) } else { font(14.5) },
                            color,
                        );
                        if resp.clicked() {
                            self.tab = tab;
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(8.0);
                        let resp = theme::theme_button(ui, &self.palette, self.dark);
                        if resp.on_hover_text("Сменить тему").clicked() {
                            self.dark = !self.dark;
                            self.dark_override = true;
                            self.palette = theme::install(ctx, self.dark);
                            self.settings_dirty = true;
                        }
                        if self.snapshot.system.admin {
                            ui.label(
                                RichText::new("Администратор")
                                    .size(11.5)
                                    .color(self.palette.ok),
                            );
                        }
                    });
                });
            });
    }

    fn toolbar(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let can_kill = self.selected_proc().map(|p| p.pid > 4).unwrap_or(false);
        let columns_before = self.columns.clone();
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            if ui
                .add_enabled(can_kill, egui::Button::new("Снять задачу"))
                .clicked()
            {
                if let Some(p) = self.selected_proc() {
                    let (pid, hwnd) = (p.pid, p.window);
                    self.request_end_task(pid, hwnd);
                }
            }

            ui.add_space(6.0);
            let pause_label = if self.paused {
                "Продолжить"
            } else {
                "Пауза"
            };
            if ui.button(pause_label).clicked() {
                self.paused = !self.paused;
                self.sampler.set_paused(self.paused);
                self.settings_dirty = true;
            }

            ui.add_space(10.0);
            let resp = ui.add_sized(
                [170.0, 22.0],
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Поиск процесса")
                    .desired_width(170.0),
            );
            self.search_id = Some(resp.id);
            if resp.changed() {
                self.rows_dirty = true;
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                egui::ComboBox::from_id_salt("speed")
                    .selected_text(self.speed_label())
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        let mut changed = false;
                        changed |= ui
                            .selectable_value(&mut self.interval_ms, 500, "Высокая")
                            .changed();
                        changed |= ui
                            .selectable_value(&mut self.interval_ms, 1000, "Обычная")
                            .changed();
                        changed |= ui
                            .selectable_value(&mut self.interval_ms, 2000, "Низкая")
                            .changed();
                        if changed {
                            self.paused = false;
                            self.sampler.set_paused(false);
                            self.sampler.set_interval(self.interval_ms);
                            self.settings_dirty = true;
                        }
                    });
                table::columns_menu(ui, &self.palette, &mut self.columns);
                ui.menu_button("Вид", |ui| {
                    if ui
                        .checkbox(&mut self.group_by_type, "Группировать по типу")
                        .changed()
                    {
                        self.rows_dirty = true;
                        self.settings_dirty = true;
                    }
                    if ui.button("Обновить").clicked() {
                        self.sampler.refresh_now();
                        ui.close_menu();
                    }
                });
            });
        });
        if self.columns != columns_before {
            self.sync_cmdline();
            self.settings_dirty = true;
        }
        let _ = ctx;
    }

    fn speed_label(&self) -> &'static str {
        if self.paused {
            "Приостановлено"
        } else {
            match self.interval_ms {
                0..=600 => "Высокая (0,5 с)",
                601..=1500 => "Обычная (1 с)",
                _ => "Низкая (2 с)",
            }
        }
    }

    fn processes_tab(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let toolbar_height = theme::pt(34.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), toolbar_height),
            Layout::left_to_right(Align::Center),
            |ui| self.toolbar(ui, ctx),
        );
        ui.add_space(theme::pt(2.0));

        if !self.snapshot.capabilities.per_process_io {
            let text = if !self.snapshot.system.admin {
                "Диск и сеть по процессам требуют прав администратора.".to_string()
            } else if let Some(e) = &self.snapshot.capabilities.etw_error {
                format!("Данные ETW недоступны. {}", e)
            } else {
                "Диск и сеть по процессам временно недоступны.".to_string()
            };
            if table::draw_banner(ui, &self.palette, &text) {
                self.apply_action(Action::Elevate, ctx);
            }
            ui.add_space(2.0);
        }

        let cols = table::layout(ui.available_width() - 10.0, &self.columns);
        if let Some(col) = table::header(ui, &self.palette, &cols, self.sort, self.sort_desc) {
            if self.sort == col {
                self.sort_desc = !self.sort_desc;
            } else {
                self.sort = col;
                self.sort_desc = matches!(
                    col,
                    Col::Cpu | Col::Memory | Col::Disk | Col::Network | Col::Gpu | Col::Threads
                );
            }
            self.rows_dirty = true;
            self.settings_dirty = true;
        }

        let rows = self.rows.clone();
        let mut out = table::RowOut::new();
        let mut icons = std::mem::take(&mut self.icons);
        {
            let mut draw_ctx = table::DrawCtx {
                palette: &self.palette,
                snapshot: &self.snapshot,
                cols: &cols,
                selected: self.selection,
                icons: &mut icons,
            };
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_rows(ui, theme::row_h(), rows.len(), |ui, range| {
                    for i in range {
                        table::draw_row(ui, &mut draw_ctx, rows[i], &mut out);
                    }
                });
        }
        self.icons = icons;
        self.apply_row_out(out, ctx);
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .exact_height(26.0)
            .frame(egui::Frame::NONE.fill(self.palette.panel))
            .show(ctx, |ui| {
                let s = &self.snapshot.system;
                let mem_pct = if s.mem_total > 0 {
                    s.mem_used as f32 / s.mem_total as f32 * 100.0
                } else {
                    0.0
                };
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(format!("Процессы: {}", self.snapshot.procs.len()))
                            .size(12.0)
                            .color(self.palette.text_dim),
                    );
                    ui.label(
                        RichText::new(format!("ЦП: {}", fmt_percent(s.cpu_total)))
                            .size(12.0)
                            .color(self.palette.text_dim),
                    );
                    ui.label(
                        RichText::new(format!("Память: {}", fmt_percent(mem_pct)))
                            .size(12.0)
                            .color(self.palette.text_dim),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(10.0);
                        let etw = if self.snapshot.capabilities.etw_active {
                            ("ETW активно", self.palette.ok)
                        } else {
                            ("ETW: нет прав", self.palette.warn)
                        };
                        ui.label(RichText::new(etw.0).size(11.5).color(etw.1));
                        if std::env::var_os("TM_DEBUG_ICONS").is_some() {
                            let paths = self
                                .snapshot
                                .procs
                                .iter()
                                .filter(|p| p.exe_path.is_some())
                                .count();
                            ui.label(
                                RichText::new(format!(
                                    "иконки {}/{} путей {}",
                                    self.icons.loaded(),
                                    self.icons.len(),
                                    paths
                                ))
                                .size(11.5)
                                .color(self.palette.warn),
                            );
                        }
                    });
                });
            });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(pid) = self.properties {
            let mut open = true;
            let proc = self.snapshot.proc_by_pid(pid);
            egui::Window::new("Свойства")
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_width(520.0)
                .max_width(640.0)
                .max_height(680.0)
                .show(ctx, |ui| {
                    if let Some(p) = proc {
                        ui.set_max_width(560.0);
                        ui.add(
                            egui::Label::new(RichText::new(p.task_name()).size(17.0).strong())
                                .wrap(),
                        );
                        ui.add_space(6.0);

                        let mut rows: Vec<(&str, String)> = vec![
                            ("PID", p.pid.to_string()),
                            ("Родительский PID", p.parent_pid.to_string()),
                            ("Сессия", p.session_id.to_string()),
                            ("Пользователь", p.user.as_deref().unwrap_or("—").to_string()),
                            (
                                "Повышение прав",
                                if p.elevated { "Да" } else { "Нет" }.to_string(),
                            ),
                            ("Состояние", p.status.label().to_string()),
                            ("Группа", p.group.label().to_string()),
                            ("Начало", fmt_filetime_local(p.create_time)),
                            ("Время ЦП", fmt_cpu_time(p.cpu_time)),
                            (
                                "Память",
                                format!(
                                    "{} (приватно {})",
                                    fmt_bytes(p.working_set),
                                    fmt_bytes(p.private_bytes)
                                ),
                            ),
                            ("Потоки", p.threads.to_string()),
                            ("Дескрипторы", p.handles.to_string()),
                        ];
                        if p.gpu > 0.0 {
                            rows.push(("Графический процессор", fmt_percent(p.gpu)));
                        }
                        rows.push(("Файл", p.exe_path.as_deref().unwrap_or("—").to_string()));
                        if let Some((cmd_pid, Some(cmd))) = &self.properties_cmdline {
                            if *cmd_pid == p.pid {
                                rows.push(("Командная строка", cmd.clone()));
                            }
                        }
                        rows.push((
                            "Описание",
                            p.description.as_deref().unwrap_or("—").to_string(),
                        ));

                        egui::ScrollArea::vertical()
                            .max_height(400.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for (key, value) in &rows {
                                    property_row(ui, &self.palette, key, value);
                                }
                            });
                    } else {
                        ui.label("Процесс уже завершён");
                    }
                });
            if !open {
                self.properties = None;
            }
        }

        if let Some(pending) = &self.pending_kill {
            let open = true;
            let mut confirmed = false;
            let mut cancelled = false;
            egui::Window::new("Завершение процесса")
                .collapsible(false)
                .resizable(false)
                .default_width(420.0)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "Завершить критический процесс «{}» (PID {})?",
                        pending.name, pending.pid
                    ));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Завершить").clicked() {
                            confirmed = true;
                        }
                        if ui.button("Отмена").clicked() {
                            cancelled = true;
                        }
                    });
                });
            let (pid, hwnd) = (pending.pid, pending.hwnd);
            if confirmed {
                self.end_task_now(pid, hwnd);
                self.pending_kill = None;
            } else if cancelled || !open {
                self.pending_kill = None;
            }
        }

        if let Some((text, at, error)) = &self.message {
            if at.elapsed() < Duration::from_secs(5) {
                egui::Area::new(egui::Id::new("toast"))
                    .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-14.0, -38.0))
                    .show(ctx, |ui| {
                        egui::Frame::NONE
                            .fill(self.palette.panel)
                            .stroke(Stroke::new(
                                1.0_f32,
                                if *error {
                                    self.palette.warn
                                } else {
                                    self.palette.border
                                },
                            ))
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.label(RichText::new(text).size(12.5));
                            });
                    });
            } else {
                self.message = None;
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        theme::update_scale(ctx);
        if let Some(rect) = ctx.input(|i| i.viewport().inner_rect) {
            self.last_window_size = (rect.width(), rect.height());
        }
        if let Some(rect) = ctx.input(|i| i.viewport().outer_rect) {
            let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
            if !minimized {
                self.last_window_pos = Some((rect.min.x, rect.min.y));
            }
        }
        let snap = self.sampler.latest();
        if snap.seq != self.snapshot.seq {
            self.snapshot = snap;
            self.rows_dirty = true;
        }
        if self.rows_dirty {
            self.rebuild_rows();
        }

        if std::env::var_os("TM_DEBUG_LAYOUT").is_some() {
            static ONCE: std::sync::Once = std::sync::Once::new();
            ONCE.call_once(|| {
                eprintln!(
                    "layout: ppp={} screen={:?} window={:?}",
                    ctx.pixels_per_point(),
                    ctx.screen_rect(),
                    ctx.input(|i| i.viewport().inner_rect)
                );
            });
        }

        // Ctrl+F — фокус в поиск; Delete — снять задачу с выбранного.
        let (ctrl_f, delete, f5) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::F),
                i.key_pressed(egui::Key::Delete),
                i.key_pressed(egui::Key::F5),
            )
        });
        if ctrl_f {
            if let Some(id) = self.search_id {
                ctx.memory_mut(|m| m.request_focus(id));
            }
        }
        if f5 {
            self.sampler.refresh_now();
        }
        if delete {
            if let Some(p) = self.selected_proc() {
                let (pid, hwnd) = (p.pid, p.window);
                self.request_end_task(pid, hwnd);
            }
        }

        self.tab_bar(ctx);
        self.status_bar(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(self.palette.bg))
            .show(ctx, |ui| {
                ui.add_space(theme::pt(4.0));
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                });
                match self.tab {
                    Tab::Processes => self.processes_tab(ui, ctx),
                    Tab::Performance => perf::ui(self, ui),
                }
            });

        self.dialogs(ctx);
        if self.settings_dirty && self.last_settings_save.elapsed() > Duration::from_millis(700) {
            self.save_settings(ctx);
        }
        ctx.request_repaint_after(Duration::from_millis(200));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let ctx = self.ctx.clone();
        self.save_settings(&ctx);
    }
}

/// Хелпер для вкладки производительности: подписи и плитки.
pub fn stat_card(ui: &mut Ui, palette: &Palette, label: &str, value: &str, width: f32) {
    egui::Frame::NONE
        .fill(palette.panel)
        .stroke(Stroke::new(1.0_f32, palette.border))
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.set_min_width(width);
            ui.vertical(|ui| {
                ui.label(RichText::new(label).size(11.0).color(palette.text_dim));
                ui.label(RichText::new(value).size(14.0).color(palette.text_strong));
            });
        });
}

pub fn graph_rect(ui: &mut Ui, height: f32) -> egui::Rect {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    rect
}

pub fn fill_bg(ui: &Ui, rect: egui::Rect, palette: &Palette) {
    ui.painter().rect_filled(rect, 0.0, palette.panel);
    ui.painter().rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, palette.border),
        StrokeKind::Inside,
    );
}

pub fn dim_label(ui: &mut Ui, palette: &Palette, text: &str) {
    ui.label(RichText::new(text).size(11.5).color(palette.text_dim));
}

/// Строка «ключ — значение» с переносом длинного значения по словам.
fn property_row(ui: &mut Ui, palette: &Palette, key: &str, value: &str) {
    let available = ui.available_width();
    let key_width = 150.0_f32.min(available * 0.42);
    let value_width = (available - key_width - 10.0).max(140.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(key_width);
            ui.add(egui::Label::new(RichText::new(key).size(11.5).color(palette.text_dim)).wrap());
        });
        ui.vertical(|ui| {
            ui.set_width(value_width);
            ui.add(egui::Label::new(RichText::new(value).size(12.5)).wrap());
        });
    });
    ui.add_space(3.0);
}
