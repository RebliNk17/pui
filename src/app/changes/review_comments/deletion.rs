//! Exact-note, local-only deletion with a shared safe-default confirmation.
use super::*;
impl Comments {
    pub(in crate::app) fn confirming(&self) -> bool {
        self.delete_requested.is_some() || self.clear_requested
    }
    pub(in crate::app) fn cancel_confirmation(&mut self) {
        self.delete_requested = None;
        self.clear_requested = false;
    }
    pub(super) fn request_delete(&mut self, id: u64) -> bool {
        if self.confirming()
            || !self
                .items
                .iter()
                .any(|item| item.id == id && Some(&item.anchor.owner) == self.owner.as_ref())
        {
            return false;
        }
        self.delete_requested = Some(id);
        true
    }
    pub(super) fn confirm_delete(&mut self) -> bool {
        let Some(id) = self.delete_requested.take() else {
            return false;
        };
        let Some(index) = self
            .items
            .iter()
            .position(|item| item.id == id && Some(&item.anchor.owner) == self.owner.as_ref())
        else {
            return false;
        };
        self.items.remove(index);
        self.storage_dirty = true;
        self.error = None;
        true
    }
    /// Called only while this exact visible embedded review owns the store.
    /// No note body/path copy is retained by the request: its stable ID is enough.
    pub(super) fn deletion_dialog(
        &mut self,
        ctx: &egui::Context,
        navigation: &diff_viewer::Navigation,
    ) {
        let own_layer = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("delete-local-review-note"),
        );
        if prompt_ui::modal_owns_current_pass(ctx)
            || egui::Popup::is_any_open(ctx)
            || ctx.memory(|memory| {
                memory
                    .top_modal_layer()
                    .is_some_and(|layer| layer != own_layer)
            })
        {
            return;
        }
        let Some(id) = self.delete_requested else {
            return;
        };
        let Some(item) = self
            .items
            .iter()
            .find(|item| item.id == id && Some(&item.anchor.owner) == self.owner.as_ref())
        else {
            self.delete_requested = None;
            return;
        };
        self.confirmation_pass = Some(ctx.cumulative_pass_nr());
        // Modified Enter/Backspace are never destructive confirmation or an
        // underlying agent approval/Stop action. Native plain activation remains.
        ctx.input_mut(|input| input.events.retain(|event| !matches!(event,
            egui::Event::Key {key:egui::Key::Enter|egui::Key::Backspace,modifiers,..} if modifiers.command || modifiers.ctrl
        )));
        let description = "Deletes only this saved local note. Other notes and unfinished comment text stay. Context already added to the draft is unchanged. No Git or remote action.";
        let result = confirmations::Dialog::new(
            "delete-local-review-note",
            "Delete local review note?",
            description,
            "Cancel",
            "Delete note",
        )
        .show(ctx, |ui| {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    format!("{} · {}", item.anchor.path, item.anchor.selection.label()),
                ))
                .wrap(),
            );
            let excerpt = item.text.chars().take(160).collect::<String>();
            ui.add(egui::Label::new(theme::small(ui, excerpt)).wrap());
        });
        match result {
            Some(confirmations::Choice::Confirm) => {
                self.confirm_delete();
                navigation.focus(ctx);
            }
            Some(confirmations::Choice::Cancel) => {
                self.delete_requested = None;
                navigation.focus(ctx);
            }
            None => {}
        }
        // Opening/dismissal events may not become late question/composer input.
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                !matches!(
                    event,
                    egui::Event::Key { .. }
                        | egui::Event::Text(_)
                        | egui::Event::Paste(_)
                        | egui::Event::Copy
                        | egui::Event::Cut
                        | egui::Event::Ime(_)
                )
            })
        });
    }
}
impl Changes {
    pub(in crate::app) fn demo_review_note_delete(&mut self) {
        if let Some(id) = self.review_comments.items.first().map(|item| item.id) {
            self.review_comments.request_delete(id);
        }
    }
}

#[cfg(test)]
mod tests;
