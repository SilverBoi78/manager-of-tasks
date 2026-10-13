//! Reusable drawing helpers: graphs, Fluent buttons, table headers, icons.

use crate::sys::icons::IconIndex;
use crate::sys::{HISTORY, Series};
use crate::theme::{Palette, blend, regular, semibold};
use egui::{
    Align, Align2, Color32, CornerRadius, Layout, Mesh, Pos2, Rect, Response, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2, pos2, vec2,
};
use egui_phosphor::regular as ph;

pub struct Line<'a> {
    pub data: &'a Series,
    pub color: Color32,
    pub fill: bool,
    pub dashed: bool,
}

impl<'a> Line<'a> {
    pub fn filled(data: &'a Series, color: Color32) -> Self {
        Self {
            data,
            color,
            fill: true,
            dashed: false,
        }
    }
    pub fn dashed(data: &'a Series, color: Color32) -> Self {
        Self {
            data,
            color,
            fill: false,
            dashed: true,
        }
    }
}

#[allow(clippy::too_many_arguments)]
/// Paint a Task Manager style graph: grid that scrolls with time, filled line, colored border.
pub fn paint_graph(
    painter: &egui::Painter,
    rect: Rect,
    pal: &Palette,
    lines: &[Line],
    max: f32,
    border: Color32,
    phase: u64,
    grid: bool,
) {
    painter.rect_filled(rect, 0, pal.graph_bg);
    let grid_color = pal.grid_of(border);
    if grid {
        for i in 1..10 {
            let y = rect.top() + rect.height() * i as f32 / 10.0;
            painter.hline(rect.x_range(), y, Stroke::new(1.0, grid_color));
        }
        let step = HISTORY / 10;
        let dx = rect.width() / (HISTORY - 1) as f32;
        for i in 0..HISTORY {
            if (i as u64 + phase).is_multiple_of(step as u64) {
                let x = rect.left() + i as f32 * dx;
                painter.vline(x, rect.y_range(), Stroke::new(1.0, grid_color));
            }
        }
    }
    let max = if max <= 0.0 { 1.0 } else { max };
    let clip = painter.with_clip_rect(rect);
    for line in lines {
        let n = line.data.0.len();
        if n < 2 {
            continue;
        }
        let dx = rect.width() / (HISTORY - 1) as f32;
        let offset = HISTORY.saturating_sub(n) as f32 * dx;
        let pts: Vec<Pos2> = line
            .data
            .0
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let f = (v / max).clamp(0.0, 1.0);
                pos2(
                    rect.left() + offset + i as f32 * dx,
                    rect.bottom() - f * rect.height(),
                )
            })
            .collect();
        if line.fill {
            let fill = pal.fill_of(line.color);
            let mut mesh = Mesh::default();
            for w in pts.windows(2) {
                let base = mesh.vertices.len() as u32;
                mesh.colored_vertex(w[0], fill);
                mesh.colored_vertex(w[1], fill);
                mesh.colored_vertex(pos2(w[1].x, rect.bottom()), fill);
                mesh.colored_vertex(pos2(w[0].x, rect.bottom()), fill);
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base, base + 2, base + 3);
            }
            clip.add(Shape::mesh(mesh));
        }
        let stroke = Stroke::new(1.3, line.color);
        if line.dashed {
            clip.extend(Shape::dashed_line(&pts, stroke, 4.0, 3.0));
        } else {
            clip.add(Shape::line(pts, stroke));
        }
    }
    painter.rect_stroke(rect, 0, Stroke::new(1.0, border), StrokeKind::Inside);
}

#[allow(clippy::too_many_arguments)]
/// A graph with the small labels around it ("% Utilization", "100%", "60 seconds", "0").
pub fn labeled_graph(
    ui: &mut Ui,
    pal: &Palette,
    size: Vec2,
    top_left: &str,
    top_right: &str,
    bottom_left: &str,
    lines: &[Line],
    max: f32,
    border: Color32,
    phase: u64,
) -> Response {
    let label_h = 16.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(size.x, size.y + label_h * 2.0), Sense::click());
    let painter = ui.painter_at(rect);
    let small = regular(11.0);
    painter.text(
        rect.left_top(),
        Align2::LEFT_TOP,
        top_left,
        small.clone(),
        pal.text_weak,
    );
    painter.text(
        rect.right_top(),
        Align2::RIGHT_TOP,
        top_right,
        small.clone(),
        pal.text_weak,
    );
    let g = Rect::from_min_max(
        pos2(rect.left(), rect.top() + label_h),
        pos2(rect.right(), rect.bottom() - label_h),
    );
    paint_graph(&painter, g, pal, lines, max, border, phase, true);
    painter.text(
        pos2(rect.left(), rect.bottom()),
        Align2::LEFT_BOTTOM,
        bottom_left,
        small.clone(),
        pal.text_weak,
    );
    painter.text(
        rect.right_bottom(),
        Align2::RIGHT_BOTTOM,
        "0",
        small,
        pal.text_weak,
    );
    resp
}

/// A big value with a small label above it (Performance page statistics).
pub fn stat(ui: &mut Ui, pal: &Palette, label: &str, value: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(
            egui::RichText::new(label)
                .font(regular(12.0))
                .color(pal.text_weak),
        );
        ui.label(
            egui::RichText::new(value)
                .font(regular(20.0))
                .color(pal.text),
        );
    });
}

/// Fluent command bar button: icon + label, flat until hovered.
pub fn command_button(
    ui: &mut Ui,
    pal: &Palette,
    icon: &str,
    label: &str,
    enabled: bool,
) -> Response {
    let text = format!("{icon}  {label}");
    let color = if enabled {
        pal.text
    } else {
        pal.text_weak.gamma_multiply(0.6)
    };
    let galley = ui.painter().layout_no_wrap(text, regular(13.0), color);
    let size = vec2(galley.size().x + 20.0, 32.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    if enabled && (resp.hovered() || resp.is_pointer_button_down_on()) {
        let fill = if resp.is_pointer_button_down_on() {
            blend(pal.hover, pal.text, 0.06)
        } else {
            pal.hover
        };
        ui.painter().rect_filled(rect, 4, fill);
    }
    ui.painter().galley(
        pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    resp
}

/// Toggle variant of a command button (shows a check-like highlight when on).
pub fn command_toggle(
    ui: &mut Ui,
    pal: &Palette,
    icon: &str,
    label: &str,
    on: bool,
    enabled: bool,
) -> Response {
    let resp = command_button(ui, pal, icon, label, enabled);
    if on {
        ui.painter().rect_stroke(
            resp.rect,
            4,
            Stroke::new(1.0, pal.accent),
            StrokeKind::Inside,
        );
    }
    resp
}

/// Page title row with commands on the right, like Windows 11 Task Manager.
pub fn page_header(ui: &mut Ui, pal: &Palette, title: &str, commands: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.set_height(40.0);
        ui.label(
            egui::RichText::new(title)
                .font(semibold(20.0))
                .color(pal.text),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            commands(ui);
        });
    });
    ui.add_space(4.0);
}

pub enum HeaderAlign {
    Left,
    Right,
}

/// Sortable column header. `total` is the large percentage shown above the
/// title on the Processes tab.
pub fn header_cell(
    ui: &mut Ui,
    pal: &Palette,
    title: &str,
    total: Option<&str>,
    sorted: Option<bool>,
    align: HeaderAlign,
) -> Response {
    let rect = ui.max_rect();
    let resp = ui.interact(rect, ui.id().with(title), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 0, pal.hover);
    }
    let painter = ui.painter();
    let (anchor_top, anchor_bottom, x) = match align {
        HeaderAlign::Left => (Align2::LEFT_TOP, Align2::LEFT_BOTTOM, rect.left() + 6.0),
        HeaderAlign::Right => (Align2::RIGHT_TOP, Align2::RIGHT_BOTTOM, rect.right() - 6.0),
    };
    let title_w = painter
        .layout_no_wrap(title.to_string(), regular(12.0), pal.text_weak)
        .size()
        .x;
    let fits = title_w <= rect.width() - 12.0;
    let (anchor_bottom, x_title) = if fits {
        (anchor_bottom, x)
    } else {
        (Align2::LEFT_BOTTOM, rect.left() + 6.0)
    };
    if let Some(total) = total {
        painter.text(
            pos2(x, rect.top() + 4.0),
            anchor_top,
            total,
            semibold(15.0),
            pal.text,
        );
        painter.text(
            pos2(x_title, rect.bottom() - 5.0),
            anchor_bottom,
            title,
            regular(12.0),
            pal.text_weak,
        );
    } else {
        let a = match (&align, fits) {
            (HeaderAlign::Right, true) => Align2::RIGHT_CENTER,
            _ => Align2::LEFT_CENTER,
        };
        painter.text(
            pos2(x_title, rect.center().y),
            a,
            title,
            regular(12.0),
            pal.text_weak,
        );
    }
    if let Some(asc) = sorted {
        let glyph = if asc { ph::CARET_UP } else { ph::CARET_DOWN };
        painter.text(
            pos2(rect.center().x, rect.top() + 1.0),
            Align2::CENTER_TOP,
            glyph,
            regular(9.0),
            pal.text_weak,
        );
    }
    painter.vline(
        rect.right(),
        rect.y_range().shrink(4.0),
        Stroke::new(1.0, pal.border),
    );
    resp
}

/// Paint a heat-map background in a table cell.
pub fn heat_bg(ui: &Ui, pal: &Palette, frac: f32) {
    let rect = ui.max_rect().expand2(vec2(3.5, -1.0));
    ui.painter().rect_filled(rect, 0, pal.heat(frac));
}

pub fn right_text(ui: &mut Ui, text: impl Into<egui::WidgetText>) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add(egui::Label::new(text).truncate());
    });
}

/// Application icon from the icon theme, or a generic glyph.
pub fn app_icon(
    ui: &mut Ui,
    pal: &Palette,
    icons: Option<&IconIndex>,
    name: Option<&str>,
    fallback: &str,
    size: f32,
) {
    if let (Some(idx), Some(name)) = (icons, name)
        && let Some(path) = idx.lookup(name)
    {
        let uri = format!("file://{}", path.display());
        ui.add(egui::Image::new(uri).fit_to_exact_size(vec2(size, size)));
        return;
    }
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        fallback,
        regular(size - 2.0),
        pal.text_weak,
    );
}

/// Small rounded "pill" toggle between options (e.g. System | User).
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    pal: &Palette,
    value: &mut T,
    options: &[(T, &str)],
) -> bool {
    let mut changed = false;
    egui::Frame::new()
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (v, label) in options {
                let on = *value == *v;
                let galley =
                    ui.painter()
                        .layout_no_wrap(label.to_string(), regular(12.0), pal.text);
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(galley.size().x + 18.0, 24.0), Sense::click());
                if on {
                    ui.painter().rect_filled(rect, 4, pal.selected);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 4, pal.hover);
                }
                ui.painter()
                    .galley(rect.center() - galley.size() / 2.0, galley, pal.text);
                if resp.clicked() && !on {
                    *value = *v;
                    changed = true;
                }
            }
        });
    changed
}

/// A Fluent "settings card": a rounded box with a title/description on the left
/// and a control on the right.
pub fn setting_card(
    ui: &mut Ui,
    pal: &Palette,
    icon: &str,
    title: &str,
    desc: &str,
    control: impl FnOnce(&mut Ui),
) {
    egui::Frame::new()
        .fill(pal.card)
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(icon)
                        .font(regular(18.0))
                        .color(pal.text),
                );
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(egui::RichText::new(title).font(regular(13.5)));
                    if !desc.is_empty() {
                        ui.label(
                            egui::RichText::new(desc)
                                .font(regular(11.5))
                                .color(pal.text_weak),
                        );
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), control);
            });
        });
    ui.add_space(3.0);
}

/// Windows 11 style on/off toggle switch.
pub fn toggle(ui: &mut Ui, pal: &Palette, on: &mut bool) -> Response {
    let size = vec2(40.0, 20.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let radius = rect.height() / 2.0;
    let fill = if *on {
        pal.accent
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if *on { pal.accent } else { pal.text_weak };
    ui.painter().rect(
        rect,
        radius,
        fill,
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );
    let knob_r = if resp.hovered() { 7.0 } else { 6.0 };
    let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    let knob = if *on {
        if pal.dark {
            Color32::BLACK
        } else {
            Color32::WHITE
        }
    } else {
        pal.text_weak
    };
    ui.painter()
        .circle_filled(pos2(x, rect.center().y), knob_r, knob);
    resp
}
