//! Тема оформления: палитра Win10, шрифт Segoe UI, размеры строк.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Pos2, Rect, Stroke,
    TextStyle, Visuals,
};

/// Текущий масштаб (пикселей на точку), обновляется каждый кадр.
static PPP: AtomicU32 = AtomicU32::new(1250);

pub const ROW_H: f32 = 26.0;
pub const HEADER_H: f32 = 32.0;
pub const FONT_SIZE: f32 = 13.5;

pub fn update_scale(ctx: &egui::Context) {
    let ppp = ctx.pixels_per_point();
    if ppp.is_finite() && ppp > 0.1 {
        PPP.store((ppp * 1000.0) as u32, Ordering::Relaxed);
    }
}

pub fn ppp() -> f32 {
    PPP.load(Ordering::Relaxed) as f32 / 1000.0
}

/// Размер в точках, выровненный по целым физическим пикселям.
pub fn pt(points: f32) -> f32 {
    let ppp = ppp();
    (points * ppp).round() / ppp
}

pub fn row_h() -> f32 {
    pt(ROW_H)
}

pub fn header_h() -> f32 {
    pt(HEADER_H)
}

/// Позиция, привязанная к сетке физических пикселей — без «полупиксельного» мыла.
pub fn snap_pos(pos: Pos2) -> Pos2 {
    let ppp = ppp();
    Pos2::new((pos.x * ppp).round() / ppp, (pos.y * ppp).round() / ppp)
}

pub fn text(
    ui: &egui::Ui,
    pos: Pos2,
    anchor: Align2,
    text: impl ToString,
    font: FontId,
    color: Color32,
) {
    ui.painter().text(snap_pos(pos), anchor, text, font, color);
}

pub fn text_clipped(
    ui: &egui::Ui,
    clip: Rect,
    pos: Pos2,
    anchor: Align2,
    text: impl ToString,
    font: FontId,
    color: Color32,
) {
    ui.painter()
        .with_clip_rect(clip)
        .text(snap_pos(pos), anchor, text, font, color);
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub header: Color32,
    pub group: Color32,
    pub row_alt: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_strong: Color32,
    pub accent: Color32,
    pub input: Color32,
    pub button: Color32,
    pub button_hover: Color32,
    pub button_active: Color32,
    pub graph_bg: Color32,
    pub graph_grid: Color32,
    pub warn: Color32,
    pub ok: Color32,
    pub mem: Color32,
    pub disk: Color32,
    pub net: Color32,
    pub gpu: Color32,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            bg: Color32::from_rgb(0x20, 0x20, 0x20),
            panel: Color32::from_rgb(0x27, 0x27, 0x27),
            header: Color32::from_rgb(0x2b, 0x2b, 0x2b),
            group: Color32::from_rgb(0x25, 0x25, 0x25),
            row_alt: Color32::from_rgb(0x23, 0x23, 0x23),
            hover: Color32::from_rgb(0x30, 0x30, 0x30),
            selected: Color32::from_rgb(0x2d, 0x4b, 0x63),
            border: Color32::from_rgb(0x3a, 0x3a, 0x3a),
            text: Color32::from_rgb(0xe6, 0xe6, 0xe6),
            text_dim: Color32::from_rgb(0x9d, 0x9d, 0x9d),
            text_strong: Color32::from_rgb(0xff, 0xff, 0xff),
            accent: Color32::from_rgb(0x4c, 0xc2, 0xff),
            input: Color32::from_rgb(0x1a, 0x1a, 0x1a),
            button: Color32::from_rgb(0x33, 0x33, 0x33),
            button_hover: Color32::from_rgb(0x3d, 0x3d, 0x3d),
            button_active: Color32::from_rgb(0x2a, 0x2a, 0x2a),
            graph_bg: Color32::from_rgb(0x14, 0x1a, 0x1e),
            graph_grid: Color32::from_rgb(0x2a, 0x33, 0x39),
            warn: Color32::from_rgb(0xf0, 0xa3, 0x5e),
            ok: Color32::from_rgb(0x6c, 0xcb, 0x5f),
            mem: Color32::from_rgb(0xb1, 0x80, 0xf0),
            disk: Color32::from_rgb(0x6c, 0xcb, 0x5f),
            net: Color32::from_rgb(0x4c, 0xc2, 0xff),
            gpu: Color32::from_rgb(0xf0, 0xa3, 0x5e),
        }
    }

    pub fn light() -> Self {
        Self {
            bg: Color32::from_rgb(0xf2, 0xf2, 0xf2),
            panel: Color32::from_rgb(0xff, 0xff, 0xff),
            header: Color32::from_rgb(0xf7, 0xf7, 0xf7),
            group: Color32::from_rgb(0xec, 0xec, 0xec),
            row_alt: Color32::from_rgb(0xf8, 0xf8, 0xf8),
            hover: Color32::from_rgb(0xe9, 0xf3, 0xfd),
            selected: Color32::from_rgb(0xcc, 0xe8, 0xff),
            border: Color32::from_rgb(0xd6, 0xd6, 0xd6),
            text: Color32::from_rgb(0x1a, 0x1a, 0x1a),
            text_dim: Color32::from_rgb(0x60, 0x60, 0x60),
            text_strong: Color32::from_rgb(0x00, 0x00, 0x00),
            accent: Color32::from_rgb(0x00, 0x67, 0xc0),
            input: Color32::from_rgb(0xff, 0xff, 0xff),
            button: Color32::from_rgb(0xfb, 0xfb, 0xfb),
            button_hover: Color32::from_rgb(0xef, 0xef, 0xef),
            button_active: Color32::from_rgb(0xe5, 0xe5, 0xe5),
            graph_bg: Color32::from_rgb(0xfa, 0xfa, 0xfa),
            graph_grid: Color32::from_rgb(0xe2, 0xe2, 0xe2),
            warn: Color32::from_rgb(0xc2, 0x6a, 0x00),
            ok: Color32::from_rgb(0x2e, 0x8b, 0x2e),
            mem: Color32::from_rgb(0x8b, 0x5c, 0xd6),
            disk: Color32::from_rgb(0x2e, 0x8b, 0x2e),
            net: Color32::from_rgb(0x00, 0x67, 0xc0),
            gpu: Color32::from_rgb(0xb8, 0x6a, 0x00),
        }
    }
}

/// Кнопка переключения светлой/тёмной темы. Рисуется вручную, чтобы не
/// зависеть от наличия символов ☀/☾ в Segoe UI.
pub fn theme_button(ui: &mut egui::Ui, palette: &Palette, dark: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(30.0, 24.0), egui::Sense::click());
    let bg = if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, palette.hover);
        palette.hover
    } else {
        palette.bg
    };
    let center = rect.center();
    if dark {
        ui.painter().circle_filled(center, 6.0, palette.text);
        ui.painter()
            .circle_filled(center + egui::vec2(3.2, -2.0), 5.4, bg);
    } else {
        ui.painter().circle_filled(center, 4.5, palette.warn);
        for i in 0..8 {
            let angle = i as f32 * std::f32::consts::TAU / 8.0;
            let dir = egui::vec2(angle.cos(), angle.sin());
            ui.painter().line_segment(
                [center + dir * 6.5, center + dir * 8.5],
                Stroke::new(1.2_f32, palette.warn),
            );
        }
    }
    resp
}

pub fn font(size: f32) -> FontId {
    FontId::new(pt(size), FontFamily::Name("segoe".into()))
}

pub fn bold(size: f32) -> FontId {
    FontId::new(pt(size), FontFamily::Name("segoe-bold".into()))
}

/// Устанавливает шрифты и визуальный стиль. Возвращает палитру.
pub fn install(ctx: &egui::Context, dark: bool) -> Palette {
    let mut fonts = FontDefinitions::default();

    let mut add = |name: &str, path: &str, family: &str| {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert(name.to_owned(), Arc::new(FontData::from_owned(bytes)));
            fonts
                .families
                .entry(FontFamily::Name(family.into()))
                .or_default()
                .insert(0, name.to_owned());
        }
    };
    add("segoe", r"C:\Windows\Fonts\segoeui.ttf", "segoe");
    add("segoe-bold", r"C:\Windows\Fonts\seguisb.ttf", "segoe-bold");
    add(
        "segoe-icons",
        r"C:\Windows\Fonts\segmdl2.ttf",
        "segoe-icons",
    );
    // Основной текстовый шрифт — Segoe UI, с фолбэком на встроенный.
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "segoe".to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "segoe".to_owned());
    ctx.set_fonts(fonts);

    let p = if dark {
        Palette::dark()
    } else {
        Palette::light()
    };
    let mut style = (*ctx.style()).clone();
    style.visuals = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    style.visuals.panel_fill = p.bg;
    style.visuals.window_fill = p.panel;
    style.visuals.extreme_bg_color = p.input;
    style.visuals.faint_bg_color = p.row_alt;
    style.visuals.override_text_color = Some(p.text);
    style.visuals.selection.bg_fill = p.selected;
    style.visuals.selection.stroke = Stroke::new(1.0_f32, p.accent);
    style.visuals.window_stroke = Stroke::new(1.0_f32, p.border);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    style.visuals.widgets.inactive.bg_fill = p.button;
    style.visuals.widgets.inactive.weak_bg_fill = p.button;
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    style.visuals.widgets.hovered.bg_fill = p.button_hover;
    style.visuals.widgets.hovered.weak_bg_fill = p.button_hover;
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.text_strong);
    style.visuals.widgets.active.bg_fill = p.button_active;
    style.visuals.widgets.active.weak_bg_fill = p.button_active;
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.text_strong);
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(9.0, 3.0);
    style.spacing.menu_margin = egui::Margin::same(4);

    style.text_styles.insert(TextStyle::Body, font(FONT_SIZE));
    style.text_styles.insert(TextStyle::Button, font(FONT_SIZE));
    style.text_styles.insert(TextStyle::Small, font(12.0));
    style.text_styles.insert(TextStyle::Heading, bold(18.0));
    style
        .text_styles
        .insert(TextStyle::Monospace, font(FONT_SIZE));
    ctx.set_style(style);

    p
}
