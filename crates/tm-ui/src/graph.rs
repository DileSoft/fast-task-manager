//! Отрисовка графиков из готовых массивов значений (без аллокаций в кадре,
//! кроме временных точек полилинии).

use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke, StrokeKind, Ui};

use crate::theme::Palette;

/// Рисует график. `series` — пары (значения, цвет). Пустые значения
/// игнорируются. `fixed_max` — фиксированная верхняя граница (например 100%).
pub fn draw(
    ui: &Ui,
    rect: Rect,
    series: &[(&[f32], Color32)],
    fixed_max: Option<f32>,
    palette: &Palette,
    grid: bool,
    fill: bool,
) {
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, palette.graph_bg);

    if grid {
        for i in 1..10 {
            let x = rect.left() + rect.width() * i as f32 / 10.0;
            painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                Stroke::new(1.0_f32, palette.graph_grid),
            );
        }
        for i in 1..5 {
            let y = rect.top() + rect.height() * i as f32 / 5.0;
            painter.line_segment(
                [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                Stroke::new(1.0_f32, palette.graph_grid),
            );
        }
    }
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, palette.border),
        StrokeKind::Inside,
    );

    let max = fixed_max.unwrap_or_else(|| {
        let mut m: f32 = 1.0;
        for (values, _) in series {
            for v in values.iter() {
                if v.is_finite() && *v > m {
                    m = *v;
                }
            }
        }
        if fixed_max.is_none() {
            m *= 1.15;
        }
        m
    });
    if max <= 0.0 {
        return;
    }

    let inner = rect.shrink(1.0);
    for (values, color) in series {
        let values = tail(values, 60);
        if values.len() < 2 {
            continue;
        }
        let step = inner.width() / (values.len() - 1) as f32;
        let y_of = |v: f32| inner.bottom() - (v.clamp(0.0, max) / max) * inner.height();

        if fill {
            let fill_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 38);
            for i in 0..values.len() - 1 {
                let x0 = inner.left() + i as f32 * step;
                let x1 = x0 + step;
                let y0 = y_of(values[i]);
                let y1 = y_of(values[i + 1]);
                painter.add(Shape::convex_polygon(
                    vec![
                        Pos2::new(x0, y0),
                        Pos2::new(x1, y1),
                        Pos2::new(x1, inner.bottom()),
                        Pos2::new(x0, inner.bottom()),
                    ],
                    fill_color,
                    Stroke::NONE,
                ));
            }
        }

        let points: Vec<Pos2> = values
            .iter()
            .enumerate()
            .map(|(i, v)| Pos2::new(inner.left() + i as f32 * step, y_of(*v)))
            .collect();
        painter.add(Shape::line(points, Stroke::new(1.4_f32, *color)));
    }
}

pub fn tail(values: &[f32], n: usize) -> &[f32] {
    if values.len() > n {
        &values[values.len() - n..]
    } else {
        values
    }
}

/// Маленький график для списка ресурсов и ядер.
pub fn sparkline(ui: &Ui, rect: Rect, values: &[f32], color: Color32, palette: &Palette) {
    draw(ui, rect, &[(values, color)], None, palette, false, true);
}

/// Заливка «шкалы состава» (например, память: в работе/кэш/свободно).
pub fn stacked_bar(ui: &Ui, rect: Rect, parts: &[(f32, Color32)]) {
    let total: f32 = parts.iter().map(|(v, _)| *v).sum();
    if total <= 0.0 {
        return;
    }
    let painter = ui.painter().with_clip_rect(rect);
    let mut x = rect.left();
    for (value, color) in parts {
        let w = rect.width() * (*value / total);
        let seg = Rect::from_min_size(Pos2::new(x, rect.top()), egui::vec2(w, rect.height()));
        painter.rect_filled(seg, 0.0, *color);
        x += w;
    }
}
