//! Quiet single-line disclosures. No nested icon tile or separate toggle button.
use super::*;

pub(super) fn show(
    section: Section,
    ui: &mut egui::Ui,
    contents: impl FnOnce(&mut egui::Ui),
) -> Response {
    let colors = theme::palette(ui.ctx());
    let id = ui.make_persistent_id(section.id);
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        id,
        section.default_open,
    );
    let width = ui.available_width().max(1.0);
    egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(6)
        .show(ui, |ui| {
            ui.set_width((width - 2.0).max(1.0));
            ui.spacing_mut().item_spacing.y = 0.0;
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::hover());
            // Retain the same persistent IDs as the older disclosure so saved
            // expanded state, keyboard navigation and content IDs survive.
            let mut response = ui.interact(rect, id.with("header"), Sense::click());
            let mut open = state.is_open();
            if response.clicked() {
                response.request_focus();
                open = !open;
            }
            if response.has_focus() && ui.is_enabled() {
                ui.memory_mut(|memory| {
                    memory.set_focus_lock_filter_current(
                        response.id,
                        egui::EventFilter {
                            horizontal_arrows: true,
                            ..Default::default()
                        },
                    )
                });
                ui.input_mut(|input| {
                    if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                        open = true;
                    }
                    if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                        open = false;
                    }
                });
            }
            if state.is_open() != open {
                response.mark_changed();
            }
            state.set_open(open);
            state.store(ui.ctx());
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::CollapsingHeader,
                    ui.is_enabled(),
                    open,
                    &section.title,
                )
            });
            if response.hovered() {
                ui.painter().rect_filled(rect.shrink(1.0), 5, colors.hover);
            }
            if response.has_focus() {
                ui.painter().rect_stroke(
                    rect.shrink(1.5),
                    5,
                    Stroke::new(1.0, colors.accent),
                    egui::StrokeKind::Inside,
                );
            }
            let icon = Rect::from_center_size(
                rect.left_center() + egui::vec2(15.0, 0.0),
                Vec2::splat(13.0),
            );
            icons::draw(ui, icon, section.icon, colors.dim);
            let toggle = Rect::from_center_size(
                rect.right_center() - egui::vec2(14.0, 0.0),
                Vec2::splat(12.0),
            );
            icons::chevron(ui, toggle, open, colors.dim);
            let left = rect.left() + 30.0;
            let right = toggle.left() - 8.0;
            let available = (right - left).max(0.0);
            let title_width = ui
                .painter()
                .layout_no_wrap(
                    section.title.clone(),
                    FontId::proportional(11.5),
                    colors.text,
                )
                .size()
                .x;
            let mut title_right = right;
            let mut hidden_summary = false;
            if !section.summary.is_empty() {
                let summary = ui.painter().layout_no_wrap(
                    section.summary.clone(),
                    FontId::proportional(10.0),
                    colors.dim,
                );
                // Hide only the redundant summary at narrow widths, never the
                // disclosure title; the full summary stays available on hover.
                if title_width + summary.size().x + 18.0 <= available {
                    title_right -= summary.size().x + 14.0;
                    ui.painter().galley(
                        egui::pos2(
                            right - summary.size().x,
                            rect.center().y - summary.size().y / 2.0,
                        ),
                        summary,
                        colors.dim,
                    );
                } else {
                    hidden_summary = true;
                }
            }
            let title_elided = paint(
                ui,
                &section.title,
                egui::pos2(left, rect.center().y),
                (title_right - left).max(0.0),
                11.5,
                colors.text,
            );
            if open {
                ui.painter().line_segment(
                    [rect.left_bottom(), rect.right_bottom()],
                    Stroke::new(1.0, colors.border),
                );
                egui::Frame::new().inner_margin(12).show(ui, |ui| {
                    ui.set_width((rect.width() - 24.0).max(1.0));
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.push_id(id, contents);
                });
            }
            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
            if title_elided || hidden_summary {
                response.on_hover_text(if section.summary.is_empty() {
                    section.title
                } else {
                    format!("{}\n{}", section.title, section.summary)
                })
            } else {
                response
            }
        })
        .inner
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tooltip_tests;
