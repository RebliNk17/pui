//! Compact terminal navigation chrome; native drawing, no glyph-font fallback.
use super::*;

pub(super) fn shell_row(
    ui: &mut egui::Ui,
    tab: &Tab,
    selected: bool,
    enabled: bool,
) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let response = ui
        .push_id(tab.id, |ui| {
            ui.add_enabled(
                enabled,
                egui::Button::new("")
                    .fill(if selected {
                        colors.surface
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(if selected {
                        Stroke::new(1.0, colors.border.gamma_multiply(0.7))
                    } else {
                        Stroke::NONE
                    })
                    .corner_radius(5)
                    .min_size(Vec2::new(ui.available_width(), 28.0)),
            )
        })
        .inner;
    let active = response.hovered() || response.has_focus();
    let color = if !response.enabled() {
        colors.dim
    } else if selected || active {
        colors.text
    } else {
        colors.muted
    };
    let icon = egui::Rect::from_center_size(
        response.rect.left_center() + egui::vec2(15.0, 0.0),
        Vec2::splat(16.0),
    );
    icons::draw(
        ui,
        icon,
        if tab.starting.is_some() {
            Icon::Clock
        } else {
            Icon::Terminal
        },
        if selected { colors.accent } else { colors.dim },
    );
    let label = format!("Shell {}", tab.label);
    let mut job =
        egui::text::LayoutJob::simple_singleline(label.clone(), FontId::proportional(11.0), color);
    job.wrap.max_rows = 1;
    job.wrap.max_width = (response.rect.width() - 38.0).max(1.0);
    let galley = ui.painter().layout_job(job);
    let text_pos = response.rect.left_center() + egui::vec2(29.0, -galley.size().y * 0.5);
    ui.painter().galley(text_pos, galley.clone(), color);
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.shrink(1.0),
            4,
            Stroke::new(1.0, colors.accent),
            egui::StrokeKind::Inside,
        );
    }
    let status = if tab.starting.is_some() {
        "Starting"
    } else if tab.running() {
        "Shell process running · command status unknown"
    } else if tab.demo.is_some() {
        "Demo · no process"
    } else {
        "Not running"
    };
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            response.enabled(),
            selected,
            format!("{label} · {status}"),
        )
    });
    #[cfg(test)]
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            egui::Id::new(("terminal-shell-icon", tab.id)),
            (icon, text_pos, galley.elided),
        )
    });
    response.on_hover_text(format!("{label} · {status}\nCtrl+Tab / Ctrl+Shift+Tab switches shells. Hiding keeps them running.")).on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn new_shell(ui: &mut egui::Ui, enabled: bool) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let response = ui.add_enabled(
        enabled,
        egui::Button::new("")
            .frame(false)
            .min_size(Vec2::new((ui.available_width() - 30.0).max(40.0), 24.0)),
    );
    let color = if !response.enabled() {
        colors.dim.gamma_multiply(0.5)
    } else if response.hovered() || response.has_focus() {
        colors.text
    } else {
        colors.muted
    };
    let center = response.rect.left_center() + egui::vec2(9.0, 0.0);
    ui.painter().line_segment(
        [center - egui::vec2(4.0, 0.0), center + egui::vec2(4.0, 0.0)],
        Stroke::new(1.2, color),
    );
    ui.painter().line_segment(
        [center - egui::vec2(0.0, 4.0), center + egui::vec2(0.0, 4.0)],
        Stroke::new(1.2, color),
    );
    ui.painter().text(
        response.rect.left_center() + egui::vec2(19.0, 0.0),
        egui::Align2::LEFT_CENTER,
        "New",
        FontId::proportional(10.0),
        color,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), "New shell")
    });
    response.on_hover_text("New shell · up to 8 per project / 32 per window")
}
