//! Shared, compact file-review chrome. Labels stay in tooltips/widget metadata.
use super::*;

pub(in crate::app) struct FileNavigation {
    pub previous: bool,
    pub next: bool,
    pub position: Option<(usize, usize)>,
    pub status: String,
    pub warning: Option<String>,
    pub git: bool,
    pub loading: bool,
}
#[derive(Default)]
pub(in crate::app) struct Action {
    pub back: bool,
    pub next: Option<bool>,
}
#[derive(Clone, Copy)]
pub(super) enum Direction {
    Left,
    Right,
    Up,
    Down,
}

pub(super) fn step_button(
    ui: &mut egui::Ui,
    direction: Direction,
    enabled: bool,
    label: &str,
) -> egui::Response {
    let response = ui
        .add_enabled(
            enabled,
            egui::Button::new("")
                .frame(false)
                .min_size(Vec2::splat(24.0)),
        )
        .on_hover_text(label);
    let colors = theme::palette(ui.ctx());
    let color = if !response.enabled() {
        colors.dim.gamma_multiply(0.45)
    } else if response.hovered() || response.has_focus() {
        colors.text
    } else {
        colors.muted
    };
    let points = [
        egui::vec2(-2.0, -4.0),
        egui::vec2(2.0, 0.0),
        egui::vec2(-2.0, 4.0),
    ]
    .map(|p| {
        response.rect.center()
            + match direction {
                Direction::Right => p,
                Direction::Left => -p,
                Direction::Up => egui::vec2(p.y, -p.x),
                Direction::Down => egui::vec2(p.y, p.x),
            }
    });
    ui.painter()
        .add(egui::Shape::line(points.to_vec(), Stroke::new(1.3, color)));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), label)
    });
    response
}

pub(super) fn files(
    ui: &mut egui::Ui,
    navigation: &mut navigation::Navigation,
    files: &FileNavigation,
) -> Option<bool> {
    let mut step = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.horizontal(|ui| {
            let before = step_button(
                ui,
                Direction::Left,
                files.previous,
                "Previous file · Alt/Option+←",
            );
            let label = if files.loading {
                "Loading…".into()
            } else if let Some((index, count)) = files.position {
                format!("{} / {count}", index + 1)
            } else {
                "—".into()
            };
            let counter = ui
                .label(theme::small(ui, label))
                .on_hover_text(&files.status);
            counter.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Label,
                    ui.is_enabled(),
                    format!(
                        "{} · {}",
                        if files.git {
                            "Current Git"
                        } else {
                            "Recorded edits"
                        },
                        files.status
                    ),
                )
            });
            let after = step_button(ui, Direction::Right, files.next, "Next file · Alt/Option+→");
            navigation.register(6, before.id);
            navigation.register(7, after.id);
            if before.clicked() {
                step = Some(false);
            }
            if after.clicked() {
                step = Some(true);
            }
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                let prefix = if files.git { "git" } else { "review" };
                data.insert_temp(
                    egui::Id::new(format!("{prefix}-previous-file")),
                    (before.id, before.rect, before.enabled()),
                );
                data.insert_temp(
                    egui::Id::new(format!("{prefix}-next-file")),
                    (after.id, after.rect, after.enabled()),
                );
            });
        });
    });
    step
}
