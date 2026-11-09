//! Shared shortcut-card geometry. Explicit row heights avoid filling a scroll
//! area's unbounded remaining height with right-aligned actions.
use super::*;

pub(super) fn right_aligned<R>(
    ui: &mut egui::Ui,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), ui.spacing().interact_size.y),
        Layout::right_to_left(Align::Center),
        content,
    )
}

fn text(
    ui: &egui::Ui,
    label: &str,
    description: &str,
    width: f32,
) -> (std::sync::Arc<egui::Galley>, std::sync::Arc<egui::Galley>) {
    let colors = theme::palette(ui.ctx());
    (
        ui.painter().layout(
            label.into(),
            FontId::proportional(13.5),
            colors.text,
            width.max(40.0),
        ),
        ui.painter().layout(
            description.into(),
            FontId::proportional(12.0),
            colors.muted,
            width.max(40.0),
        ),
    )
}

pub(super) fn row_height(ui: &egui::Ui, label: &str, description: &str, width: f32) -> f32 {
    let (title, detail) = text(ui, label, description, width - 86.0);
    (24.0
        + title.size().y
        + if description.is_empty() {
            0.0
        } else {
            4.0 + detail.size().y
        })
    .max(46.0)
}

pub(super) fn choice(
    ui: &mut egui::Ui,
    number: usize,
    label: &str,
    description: &str,
    checked: bool,
    focused: bool,
) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let width = ui.available_width();
    let (title, detail) = text(ui, label, description, width - 86.0);
    let height = row_height(ui, label, description, width);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let active = checked || focused || response.has_focus();
    ui.painter().rect(
        rect,
        10,
        if active || response.hovered() {
            colors.hover
        } else {
            colors.sidebar
        },
        Stroke::new(1.0, if active { colors.accent } else { colors.border }),
        egui::StrokeKind::Inside,
    );
    let badge = egui::Rect::from_min_size(rect.min + egui::vec2(12.0, 10.0), Vec2::splat(24.0));
    ui.painter().rect_filled(badge, 5, colors.bg);
    // Keep the shortcut visible even when selected. Selection has its own check.
    ui.painter().text(
        badge.center(),
        egui::Align2::CENTER_CENTER,
        number.to_string(),
        FontId::proportional(11.0),
        if active { colors.accent } else { colors.muted },
    );
    let pos = rect.min + egui::vec2(48.0, 12.0);
    let detail_pos = pos + egui::vec2(0.0, title.size().y + 4.0);
    ui.painter().galley(pos, title, colors.text);
    if !description.is_empty() {
        ui.painter().galley(detail_pos, detail, colors.muted);
    }
    if checked {
        let c = rect.right_center() - egui::vec2(16.0, 0.0);
        ui.painter().add(egui::Shape::line(
            vec![
                c + egui::vec2(-4.0, 0.0),
                c + egui::vec2(-1.0, 3.0),
                c + egui::vec2(4.0, -3.0),
            ],
            Stroke::new(1.5, colors.accent),
        ));
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), checked, label)
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn editor(
    ui: &mut egui::Ui,
    draft: &mut String,
    id: egui::Id,
    focus: bool,
) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let focused = focus || ui.memory(|m| m.has_focus(id));
    egui::Frame::new()
        .fill(colors.bg)
        .stroke(Stroke::new(
            1.0,
            if focused {
                colors.accent
            } else {
                colors.border
            },
        ))
        .corner_radius(12)
        .inner_margin(14)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Your answer").size(12.0).color(colors.muted));
            ui.add_space(8.0);
            let response = ui.add(
                egui::TextEdit::multiline(draft)
                    .id(id)
                    .frame(false)
                    .desired_width(f32::INFINITY)
                    .desired_rows(3)
                    .font(FontId::proportional(13.5))
                    .char_limit(4000)
                    .hint_text(
                        RichText::new("Share your preference, details or constraints…")
                            .color(colors.dim),
                    )
                    .return_key(egui::KeyboardShortcut::new(
                        egui::Modifiers::SHIFT,
                        egui::Key::Enter,
                    )),
            );
            if focus {
                response.request_focus();
            }
            response
        })
        .inner
}

pub(super) fn submit(ui: &mut egui::Ui, enabled: bool, last: bool) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    ui.add_enabled(
        enabled,
        egui::Button::new(
            RichText::new(if last {
                "Submit & continue"
            } else {
                "Next question"
            })
            .color(if enabled { colors.bg } else { colors.dim }),
        )
        .fill(if enabled {
            colors.accent
        } else {
            colors.surface
        })
        .corner_radius(7),
    )
}
