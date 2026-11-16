//! Frozen whole-note-list confirmation; no index retargeting after deletions.
use super::*;
pub(in crate::app::tasks) struct Pending {
    owner: String,
    index: usize,
    notes: Vec<domain::checkpoints::Checkpoint>,
}
impl Pui {
    pub(super) fn checkpoint_delete_inputs(&mut self, ctx: &egui::Context) {
        let selected = self.tasks.selected.clone();
        let visible = self.tasks.open && self.checkpoint_delete_available();
        for (id, view) in &mut self.tasks.views {
            if !visible || selected.as_ref() != Some(id) || view.tab != work_item::Tab::Checkpoints
            {
                view.checkpoint_delete = None;
            }
        }
        if self
            .tasks
            .views
            .values()
            .any(|v| v.checkpoint_delete.is_some())
        {
            ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        }
    }
    fn checkpoint_delete_available(&self) -> bool {
        !self.demo
            && !self.is_mirror()
            && !self.agents.exiting
            && !self.tasks.storage_failed
            && self.tasks.store.is_some()
    }
    pub(super) fn checkpoint_delete_row(
        &mut self,
        ui: &mut egui::Ui,
        task: &Task,
        view: &mut work_item::View,
        index: usize,
        controls: &mut Vec<egui::Id>,
    ) {
        let enabled = self.checkpoint_editable(&task.id)
            && view.checkpoint_edit.is_none()
            && view.checkpoint_delete.is_none()
            && prompt_ui::keyboard_available(ui)
            && ui.memory(|m| m.top_modal_layer().is_none())
            && !egui::Popup::is_any_open(ui.ctx());
        let response = ui
            .push_id(
                (
                    "checkpoint-delete",
                    &task.id,
                    view.checkpoint_generation,
                    index,
                ),
                |ui| prompt_ui::button(ui, "Delete note…", prompt_ui::Button::Secondary, enabled),
            )
            .inner;
        controls.push(response.id);
        #[cfg(test)]
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new(("checkpoint-delete-open", index)),
                (response.id, response.rect, response.enabled()),
            )
        });
        if response.clicked() {
            view.checkpoint_delete = Some(Pending {
                owner: task.id.clone(),
                index,
                notes: task.checkpoints.clone(),
            });
        }
    }
    fn delete_checkpoint(&mut self, pending: &Pending) -> Result<(), String> {
        if !self.checkpoint_editable(&pending.owner) {
            return Err("Checkpoint storage unavailable for this task.".into());
        }
        let task = self
            .tasks
            .records
            .iter_mut()
            .find(|t| t.id == pending.owner)
            .ok_or("Task unavailable.")?;
        if task.checkpoints != pending.notes || pending.index >= task.checkpoints.len() {
            return Err("Checkpoint list changed; inspect the notes again.".into());
        }
        task.checkpoints.remove(pending.index);
        self.tasks.save();
        Ok(())
    }
    pub(super) fn checkpoint_delete_dialog(
        &mut self,
        ctx: &egui::Context,
        task: &Task,
        view: &mut work_item::View,
    ) {
        let Some(pending) = &view.checkpoint_delete else {
            return;
        };
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("delete-task-checkpoint"),
        );
        if pending.owner != task.id
            || pending.notes != task.checkpoints
            || !self.checkpoint_editable(&task.id)
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            view.checkpoint_delete = None;
            return;
        }
        let Some(note) = pending.notes.get(pending.index) else {
            view.checkpoint_delete = None;
            return;
        };
        ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        let choice=confirmations::Dialog::new("delete-task-checkpoint","Delete checkpoint note?","Permanently removes only this local user note. Other notes, task status and workflow evidence stay unchanged. Does not run work or send a message.","Cancel","Delete note").show(ctx,|ui|{ui.add(egui::Label::new(theme::small(ui,note.text.chars().take(240).collect::<String>())).wrap().selectable(false));});
        if let Some(choice) = choice {
            let pending = view.checkpoint_delete.take().unwrap();
            if choice == confirmations::Choice::Confirm
                && let Err(error) = self.delete_checkpoint(&pending)
            {
                self.tasks.error = Some(error);
            }
        }
        ctx.input_mut(|i| {
            i.events.retain(|e| {
                !matches!(
                    e,
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
#[cfg(test)]
mod tests;
