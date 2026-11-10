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
        let response = prompt_ui::modal(ctx, self.id, 360.0, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            heading(ui, self.title, self.description, self.destructive);
            ui.add_space(16.0);
            // Only the details scroll. The title and decision buttons stay visible.
            egui::ScrollArea::vertical()
                .id_salt(self.id.with("details"))
                .max_height((ctx.screen_rect().height() - ui.min_rect().height() - 150.0).max(40.0))
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

pub(super) fn heading(ui: &mut egui::Ui, title: &str, description: &str, destructive: bool) {
    prompt_ui::badge(
        ui,
        if destructive {
            Icon::Stop
        } else {
            Icon::Branch
        },
        destructive,
    );
    prompt_ui::heading(ui, title, description);
}

pub(super) fn note(ui: &mut egui::Ui, text: &str) {
    prompt_ui::note(ui, text);
}

pub(super) fn actions(
    ui: &mut egui::Ui,
    cancel: &str,
    confirm: &str,
    destructive: bool,
    enabled: bool,
    focus_cancel: bool,
) -> Option<Choice> {
    let mut choice = None;
    prompt_ui::footer(ui, |ui| {
        let kind = if destructive {
            prompt_ui::Button::Danger
        } else {
            prompt_ui::Button::Primary
        };
        if prompt_ui::button(ui, confirm, kind, enabled).clicked() {
            choice = Some(Choice::Confirm);
        }
        let cancel = prompt_ui::button(ui, cancel, prompt_ui::Button::Secondary, true);
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
