//! Theme previews are small native-painted workspaces, not external images.
use super::{page::section, *};

pub(super) fn styles(ui: &mut egui::Ui, mode: theme::Theme, styles: &mut theme::Styles) {
    section(
        ui,
        "Theme style",
        "Choose a style for this mode. Your dark and light selections are remembered separately.",
    );
    let columns = if ui.available_width() >= 600.0 { 4 } else { 2 };
    match mode {
        theme::Theme::Dark => {
            for row in theme::DarkStyle::ALL.chunks(columns) {
                ui.columns(columns, |cols| {
                    for (ui, &style) in cols.iter_mut().zip(row) {
                        if preview(
                            ui,
                            ("dark-style", style.label()),
                            style.label(),
                            style.description(),
                            style.palette(),
                            styles.dark == style,
                        )
                        .clicked()
                        {
                            styles.dark = style;
                        }
                    }
                });
                ui.add_space(8.0);
            }
        }
        theme::Theme::Light => {
            for row in theme::LightStyle::ALL.chunks(columns) {
                ui.columns(columns, |cols| {
                    for (ui, &style) in cols.iter_mut().zip(row) {
                        if preview(
                            ui,
                            ("light-style", style.label()),
                            style.label(),
                            style.description(),
                            style.palette(),
                            styles.light == style,
                        )
                        .clicked()
                        {
                            styles.light = style;
                        }
                    }
                });
                ui.add_space(8.0);
            }
        }
    }
}

fn preview(
    ui: &mut egui::Ui,
    id: (&str, &str),
    label: &str,
    description: &str,
    colors: &theme::Palette,
    selected: bool,
) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 90.0), Sense::hover());
    let response = ui.interact(rect, ui.id().with(id), Sense::click());
    if response.clicked() {
        response.request_focus();
    }
    let painter = ui.painter();
    painter.rect(
        rect.shrink(1.0),
        10,
        colors.bg,
        Stroke::new(
            if selected { 2.0 } else { 1.0 },
            if selected || response.hovered() || response.has_focus() {
                colors.accent
            } else {
                colors.border
            },
        ),
        egui::StrokeKind::Inside,
    );
    let stage = egui::Rect::from_min_max(
        rect.min + egui::vec2(9.0, 9.0),
        rect.right_top() + egui::vec2(-9.0, 53.0),
    );
    painter.rect_filled(stage, 4, colors.surface);
    let sidebar =
        egui::Rect::from_min_size(stage.min, Vec2::new(stage.width() * 0.25, stage.height()));
    painter.rect_filled(sidebar, 4, colors.sidebar);
    for offset in [10.0, 18.0, 26.0] {
        painter.line_segment(
            [
                sidebar.min + egui::vec2(5.0, offset),
                sidebar.min + egui::vec2(sidebar.width() - 5.0, offset),
            ],
            Stroke::new(2.0, colors.border),
        );
    }
    let content = stage.min + egui::vec2(sidebar.width() + 7.0, 9.0);
    painter.line_segment(
        [content, content + egui::vec2(stage.width() * 0.40, 0.0)],
        Stroke::new(3.0, colors.accent),
    );
    painter.line_segment(
        [
            content + egui::vec2(0.0, 9.0),
            content + egui::vec2(stage.width() * 0.52, 9.0),
        ],
        Stroke::new(2.0, colors.muted),
    );
    painter.rect_filled(
        egui::Rect::from_min_max(
            stage.center() + egui::vec2(0.0, 6.0),
            stage.max - egui::vec2(6.0, 6.0),
        ),
        3,
        colors.user_bg,
    );
    painter.text(
        rect.left_bottom() + egui::vec2(12.0, -17.0),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.0),
        colors.text,
    );
    let center = rect.right_bottom() - egui::vec2(16.0, 17.0);
    painter.circle_stroke(
        center,
        5.0,
        Stroke::new(
            1.0,
            if selected {
                colors.accent
            } else {
                colors.border
            },
        ),
    );
    if selected {
        painter.circle_filled(center, 2.5, colors.accent);
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, label)
    });
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(description)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_wrap_at_narrow_widths_and_can_be_selected_with_the_keyboard() {
        for width in [400.0, 760.0] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let mode = theme::Theme::Dark;
            let mut selected = theme::Styles::default();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(width, 900.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        styles(ui, mode, &mut selected);
                        assert!(ui.min_rect().right() <= ui.max_rect().right() + 1.0);
                    });
                },
            );
        }
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut clicked = false;
        for events in [
            vec![],
            vec![egui::Event::Key {
                key: egui::Key::Space,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let response = preview(
                            ui,
                            ("test", "forest"),
                            "Forest",
                            "Evergreen",
                            theme::DarkStyle::Forest.palette(),
                            false,
                        );
                        response.request_focus();
                        clicked |= response.clicked();
                    });
                },
            );
        }
        assert!(clicked, "Space should activate a focused style preview");
    }
}
