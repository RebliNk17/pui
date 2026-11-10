//! Small, resolution-independent line icons (no platform-specific glyph fonts).
use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

#[derive(Clone, Copy)]
pub enum Icon {
    Plus,
    Search,
    Sidebar,
    Board,
    Sliders,
    Folder,
    ArrowUp,
    ArrowLeft,
    Bell,
    Shield,
    Stop,
    Branch,
    File,
    Terminal,
    Command,
    Spark,
    Refresh,
    Rewind,
    Close,
    Copy,
    Grip,
    Details,
    Edit,
    Code,
    Expand,
    Collapse,
}

pub fn draw(ui: &Ui, rect: Rect, icon: Icon, color: Color32) {
    draw_scaled(ui, rect, icon, color, 1.0);
}

fn draw_scaled(ui: &Ui, rect: Rect, icon: Icon, color: Color32, scale: f32) {
    let colors = crate::theme::palette(ui.ctx());
    let p = ui.painter();
    let c = rect.center();
    let s = Stroke::new((1.3 * scale).max(1.0), color);
    let pt = |x: f32, y: f32| Pos2::new(c.x + x * scale, c.y + y * scale);
    let line = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([pt(a.0, a.1), pt(b.0, b.1)], s);
    };
    let outline = |x: f32, y: f32, w: f32, h: f32, r: u8| {
        p.rect_stroke(
            Rect::from_min_size(pt(x, y), Vec2::new(w, h) * scale),
            (r as f32 * scale).round() as u8,
            s,
            egui::StrokeKind::Inside,
        );
    };
    match icon {
        Icon::Plus => {
            line((-5.0, 0.0), (5.0, 0.0));
            line((0.0, -5.0), (0.0, 5.0));
        }
        Icon::Search => {
            p.circle_stroke(pt(-1.0, -1.0), 4.5 * scale, s);
            line((2.5, 2.5), (6.0, 6.0));
        }
        Icon::Sidebar => {
            outline(-7.0, -6.0, 14.0, 12.0, 2);
            line((-2.0, -6.0), (-2.0, 6.0));
        }
        Icon::Board => {
            outline(-7.0, -6.0, 14.0, 12.0, 2);
            line((-2.5, -6.0), (-2.5, 6.0));
            line((2.5, -6.0), (2.5, 6.0));
            line((-5.0, -3.0), (-5.0, 1.0));
            line((0.0, -3.0), (0.0, -1.0));
            line((5.0, -3.0), (5.0, 3.0));
        }
        Icon::Sliders => {
            for (y, x) in [(-4.0, -2.0), (3.0, 3.0)] {
                line((-6.0, y), (6.0, y));
                p.circle_filled(pt(x, y), 2.3 * scale, colors.bg);
                p.circle_stroke(pt(x, y), 2.3 * scale, s);
            }
        }
        Icon::Folder => {
            p.add(egui::Shape::closed_line(
                vec![
                    pt(-7.0, -5.0),
                    pt(-2.0, -5.0),
                    pt(0.0, -3.0),
                    pt(7.0, -3.0),
                    pt(7.0, 6.0),
                    pt(-7.0, 6.0),
                ],
                s,
            ));
        }
        Icon::ArrowUp => {
            line((0.0, 6.0), (0.0, -6.0));
            line((0.0, -6.0), (-4.0, -2.0));
            line((0.0, -6.0), (4.0, -2.0));
        }
        Icon::ArrowLeft => {
            line((6.0, 0.0), (-6.0, 0.0));
            line((-6.0, 0.0), (-2.0, -4.0));
            line((-6.0, 0.0), (-2.0, 4.0));
        }
        Icon::Bell => {
            p.add(egui::Shape::line(
                vec![
                    pt(-6.0, 4.0),
                    pt(-4.0, 1.0),
                    pt(-4.0, -3.0),
                    pt(-2.0, -6.0),
                    pt(2.0, -6.0),
                    pt(4.0, -3.0),
                    pt(4.0, 1.0),
                    pt(6.0, 4.0),
                    pt(-6.0, 4.0),
                ],
                s,
            ));
            line((-1.5, 6.0), (1.5, 6.0));
        }
        Icon::Shield => {
            p.add(egui::Shape::closed_line(
                vec![
                    pt(0.0, -7.0),
                    pt(6.0, -4.0),
                    pt(5.0, 3.0),
                    pt(0.0, 7.0),
                    pt(-5.0, 3.0),
                    pt(-6.0, -4.0),
                ],
                s,
            ));
            line((-2.5, 0.0), (-0.5, 2.0));
            line((-0.5, 2.0), (3.0, -2.0));
        }
        Icon::Stop => {
            p.rect_filled(
                Rect::from_center_size(c, Vec2::splat(8.0) * scale),
                2,
                color,
            );
        }
        Icon::Branch => {
            line((-4.0, -3.0), (-4.0, 3.0));
            line((-4.0, 1.0), (4.0, -3.0));
            for (x, y) in [(-4.0, -5.0), (-4.0, 5.0), (5.0, -4.0)] {
                p.circle_stroke(pt(x, y), 2.0 * scale, s);
            }
        }
        Icon::File => {
            p.add(egui::Shape::closed_line(
                vec![
                    pt(-5.0, -7.0),
                    pt(1.0, -7.0),
                    pt(5.0, -3.0),
                    pt(5.0, 7.0),
                    pt(-5.0, 7.0),
                ],
                s,
            ));
            line((1.0, -7.0), (1.0, -3.0));
            line((1.0, -3.0), (5.0, -3.0));
            line((-2.0, 1.0), (2.0, 1.0));
            line((-2.0, 4.0), (2.0, 4.0));
        }
        Icon::Terminal => {
            outline(-7.0, -6.0, 14.0, 12.0, 2);
            line((-4.0, -2.0), (-1.0, 0.0));
            line((-1.0, 0.0), (-4.0, 2.0));
            line((1.0, 3.0), (4.0, 3.0));
        }
        Icon::Command => {
            line((-5.0, -4.0), (0.0, 0.0));
            line((0.0, 0.0), (-5.0, 4.0));
            line((2.0, 5.0), (6.0, 5.0));
        }
        Icon::Spark => {
            p.add(egui::Shape::closed_line(
                vec![
                    pt(0.0, -7.0),
                    pt(2.0, -2.0),
                    pt(7.0, 0.0),
                    pt(2.0, 2.0),
                    pt(0.0, 7.0),
                    pt(-2.0, 2.0),
                    pt(-7.0, 0.0),
                    pt(-2.0, -2.0),
                ],
                s,
            ));
        }
        Icon::Refresh => {
            p.add(egui::Shape::line(
                vec![
                    pt(5.0, -3.0),
                    pt(2.0, -6.0),
                    pt(-3.0, -5.0),
                    pt(-6.0, -1.0),
                    pt(-4.0, 4.0),
                    pt(1.0, 6.0),
                    pt(5.0, 3.0),
                ],
                s,
            ));
            line((5.0, -3.0), (1.0, -3.0));
            line((5.0, -3.0), (5.0, -7.0));
        }
        Icon::Rewind => {
            line((-6.0, -6.0), (-6.0, -1.0));
            line((-6.0, -1.0), (-1.0, -1.0));
            p.add(egui::Shape::line(
                vec![
                    pt(-6.0, -1.0),
                    pt(-3.0, -5.0),
                    pt(2.0, -5.0),
                    pt(6.0, -1.0),
                    pt(5.0, 4.0),
                    pt(1.0, 6.0),
                    pt(-3.0, 5.0),
                ],
                s,
            ));
        }
        Icon::Close => {
            line((-4.0, -4.0), (4.0, 4.0));
            line((-4.0, 4.0), (4.0, -4.0));
        }
        Icon::Copy => {
            // Two offset sheets, with a clear gap between their outlines.
            outline(-2.0, -2.0, 9.0, 10.0, 2);
            p.add(egui::Shape::line(
                vec![
                    pt(-5.0, 4.0),
                    pt(-6.0, 4.0),
                    pt(-6.0, -6.0),
                    pt(3.0, -6.0),
                    pt(3.0, -5.0),
                ],
                s,
            ));
        }
        Icon::Grip => {
            for x in [-2.0, 2.0] {
                for y in [-4.0, 0.0, 4.0] {
                    p.circle_filled(pt(x, y), 0.8 * scale, color);
                }
            }
        }
        Icon::Details => {
            outline(-6.0, -7.0, 12.0, 14.0, 2);
            for y in [-3.5, 0.0, 3.5] {
                p.circle_filled(pt(-3.0, y), 0.7 * scale, color);
                line((-0.5, y), (3.0, y));
            }
        }
        Icon::Edit => {
            line((-5.0, 4.0), (3.0, -5.0));
            line((-2.0, 6.0), (6.0, -3.0));
            line((3.0, -5.0), (6.0, -3.0));
            line((-5.0, 4.0), (-6.0, 7.0));
            line((-6.0, 7.0), (-2.0, 6.0));
        }
        Icon::Expand => {
            line((-6.0, 6.0), (6.0, -6.0));
            line((0.0, -6.0), (6.0, -6.0));
            line((6.0, -6.0), (6.0, 0.0));
            line((-6.0, 0.0), (-6.0, 6.0));
            line((-6.0, 6.0), (0.0, 6.0));
        }
        Icon::Collapse => {
            line((-7.0, 7.0), (-2.0, 2.0));
            line((-7.0, 2.0), (-2.0, 2.0));
            line((-2.0, 2.0), (-2.0, 7.0));
            line((7.0, -7.0), (2.0, -2.0));
            line((2.0, -7.0), (2.0, -2.0));
            line((2.0, -2.0), (7.0, -2.0));
        }
        Icon::Code => {
            line((-3.0, -5.0), (-7.0, 0.0));
            line((-7.0, 0.0), (-3.0, 5.0));
            line((3.0, -5.0), (7.0, 0.0));
            line((7.0, 0.0), (3.0, 5.0));
        }
    }
}

pub fn chevron(ui: &Ui, rect: Rect, open: bool, color: Color32) {
    let c = rect.center();
    let points = if open {
        [
            c + egui::vec2(-2.5, -1.0),
            c + egui::vec2(0.0, 1.5),
            c + egui::vec2(2.5, -1.0),
        ]
    } else {
        [
            c + egui::vec2(-1.0, -2.5),
            c + egui::vec2(1.5, 0.0),
            c + egui::vec2(-1.0, 2.5),
        ]
    };
    ui.painter()
        .add(egui::Shape::line(points.to_vec(), Stroke::new(1.0, color)));
}

pub fn button(ui: &mut Ui, icon: Icon, tip: &str) -> Response {
    let colors = crate::theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 6, colors.hover);
    }
    draw(
        ui,
        rect,
        icon,
        if response.hovered() {
            colors.text
        } else {
            colors.muted
        },
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tip)
}

/// Compact icon + label action with one hit target and no font-symbol fallback.
pub fn labeled_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    let colors = crate::theme::palette(ui.ctx());
    ui.add_enabled(enabled, |ui: &mut Ui| {
        let galley =
            ui.painter()
                .layout_no_wrap(label.into(), egui::FontId::proportional(10.0), colors.dim);
        let icon_size = 12.0;
        let gap = 4.0;
        // Match egui's horizontal-row height so preceding metadata labels do not
        // stay centered on a shorter row while this action extends below them.
        let height = ui.spacing().interact_size.y.max(galley.size().y);
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(galley.size().x + icon_size + gap, height),
            Sense::click(),
        );
        let hovered = response.hovered() || response.has_focus();
        let color = if hovered && ui.is_enabled() {
            colors.text
        } else {
            colors.dim
        };
        if hovered && ui.is_enabled() {
            ui.painter().rect_filled(rect, 5, colors.hover);
        }
        draw_scaled(
            ui,
            Rect::from_center_size(
                rect.left_center() + egui::vec2(icon_size / 2.0, 0.0),
                Vec2::splat(icon_size),
            ),
            icon,
            color,
            icon_size / 16.0,
        );
        ui.painter().galley_with_override_text_color(
            rect.left_center() + egui::vec2(icon_size + gap, -galley.size().y / 2.0),
            galley,
            color,
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
        });
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    })
}

pub fn inline(ui: &mut Ui, icon: Icon, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
    draw(ui, rect, icon, color);
}
