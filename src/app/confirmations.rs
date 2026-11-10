//! Shared confirmation surfaces. Explicit action only; Escape/backdrop cancel.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Cancel,
    Confirm,
}

pub(super) struct Dialog<'a> {
    id: egui::Id,
    title: &'a str,
    description: &'a str,
    cancel: &'a str,
    confirm: &'a str,
    destructive: bool,
    enabled: bool,
}
impl<'a> Dialog<'a> {
    pub fn new(
        id: &str,
        title: &'a str,
        description: &'a str,
        cancel: &'a str,
        confirm: &'a str,
    ) -> Self {
        Self {
            id: egui::Id::new(id),
            title,
            description,
            cancel,
            confirm,
            destructive: true,
            enabled: true,
        }
    }
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn show(self, ctx: &egui::Context, body: impl FnOnce(&mut egui::Ui)) -> Option<Choice> {
        let frame_number = ctx.cumulative_frame_nr();
        let first = ctx.data_mut(|data| {
            let previous = data.get_temp::<u64>(self.id.with("last-frame"));
            data.insert_temp(self.id.with("last-frame"), frame_number);
            previous.is_none_or(|previous| previous.saturating_add(1) < frame_number)
        });
        let width = (ctx.screen_rect().width() - 64.0).clamp(200.0, 420.0);
        let response = egui::Modal::new(self.id)
            .frame(surface(ctx))
            .backdrop_color(Color32::from_black_alpha(145))
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 0.0;
                heading(ui, self.title, self.description, self.destructive);
                ui.add_space(16.0);
                // Only the details scroll. The title and decision buttons stay visible.
                egui::ScrollArea::vertical()
                    .id_salt(self.id.with("details"))
                    .max_height((ctx.screen_rect().height() - 238.0).max(64.0))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        body(ui);
                    });
                ui.add_space(22.0);
                actions(
                    ui,
                    self.cancel,
                    self.confirm,
                    self.destructive,
                    self.enabled,
                    first,
                )
            });
        if response.should_close() {
            Some(Choice::Cancel)
        } else {
            response.inner
        }
    }
}

pub(super) fn surface(ctx: &egui::Context) -> egui::Frame {
    let colors = theme::palette(ctx);
    egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(16)
        .inner_margin(24)
        .shadow(egui::epaint::Shadow {
            offset: [0, 10],
            blur: 32,
            spread: 0,
            color: Color32::from_black_alpha(65),
        })
}

pub(super) fn heading(ui: &mut egui::Ui, title: &str, description: &str, destructive: bool) {
    let colors = theme::palette(ui.ctx());
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let (badge, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
        ui.painter().rect_filled(
            badge,
            10,
            if destructive {
                colors.error_bg
            } else {
                colors.accent_bg
            },
        );
        icons::draw(
            ui,
            badge,
            if destructive {
                Icon::Stop
            } else {
                Icon::Branch
            },
            if destructive {
                colors.red
            } else {
                colors.accent
            },
        );
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 7.0;
            ui.add(
                egui::Label::new(RichText::new(title).size(18.0).strong().color(colors.text))
                    .wrap(),
            );
            if !description.is_empty() {
                ui.add(
                    egui::Label::new(RichText::new(description).size(13.0).color(colors.muted))
                        .wrap(),
                );
            }
        });
    });
}

pub(super) fn note(ui: &mut egui::Ui, text: &str) {
    let colors = theme::palette(ui.ctx());
    egui::Frame::new()
        .fill(colors.bg)
        .corner_radius(9)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(text).size(12.0).color(colors.muted)).wrap());
        });
}

pub(super) fn actions(
    ui: &mut egui::Ui,
    cancel: &str,
    confirm: &str,
    destructive: bool,
    enabled: bool,
    focus_cancel: bool,
) -> Option<Choice> {
    let colors = theme::palette(ui.ctx());
    let mut choice = None;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.spacing_mut().button_padding = egui::vec2(14.0, 9.0);
        let fill = if destructive {
            Color32::from_rgb(177, 57, 49)
        } else {
            Color32::from_rgb(101, 78, 210)
        };
        let action = egui::Button::new(RichText::new(confirm).size(12.5).color(Color32::WHITE))
            .fill(fill)
            .stroke(Stroke::new(1.0, fill))
            .corner_radius(8)
            .min_size(egui::vec2(100.0, 36.0));
        if ui.add_enabled(enabled, action).clicked() {
            choice = Some(Choice::Confirm);
        }
        let cancel = ui.add(
            egui::Button::new(RichText::new(cancel).size(12.5).color(colors.text))
                .fill(colors.surface)
                .stroke(Stroke::new(1.0, colors.border))
                .corner_radius(8)
                .min_size(egui::vec2(100.0, 36.0)),
        );
        if focus_cancel {
            cancel.request_focus();
        }
        if cancel.clicked() {
            choice = Some(Choice::Cancel);
        }
    });
    choice
}

#[cfg(test)]
#[path = "confirmation_tests.rs"]
mod tests;
