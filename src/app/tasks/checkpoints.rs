//! Explicit local user notes, never a workflow transition or execution authorization.
use super::*;
pub(super) mod deletion;
pub(super) mod edit;
mod links;
impl Pui {
    pub(in crate::app) fn checkpoint_inputs(&mut self, ctx: &egui::Context) {
        self.checkpoint_delete_inputs(ctx);
        if !self.tasks.open
            || ctx.memory(|m| m.top_modal_layer().is_some())
            || egui::Popup::is_any_open(ctx)
        {
            return;
        }
        let focused = ctx.memory(|m| m.focused());
        if let Some((owner, pass, id)) = ctx.data(|d| {
            d.get_temp::<(String, u64, egui::Id)>(egui::Id::new("checkpoint-link-control"))
        }) && self.tasks.selected.as_ref() == Some(&owner)
            && ctx.cumulative_pass_nr().saturating_sub(pass) <= 2
            && focused == Some(id)
        {
            ctx.input_mut(|i| {
                for m in [egui::Modifiers::COMMAND, egui::Modifiers::CTRL] {
                    i.consume_key(m, egui::Key::Enter);
                    i.consume_key(m, egui::Key::Backspace);
                }
            });
        }
        let controls = ctx.data(|d| {
            d.get_temp::<(String, u64, [egui::Id; 2])>(egui::Id::new("checkpoint-controls"))
        });
        let Some((owner, pass, controls)) = controls else {
            return;
        };
        if self.tasks.selected.as_ref() != Some(&owner)
            || ctx.cumulative_pass_nr().saturating_sub(pass) > 2
            || self
                .tasks
                .views
                .get(&owner)
                .is_none_or(|v| v.tab != work_item::Tab::Checkpoints)
        {
            return;
        }
        let edits = ctx.data(|d| {
            d.get_temp::<(String, u64, Vec<egui::Id>)>(egui::Id::new("checkpoint-edit-controls"))
        });
        let owns_id = |id: egui::Id| {
            focused == Some(id) || (focused.is_none() && ctx.memory(|m| m.focus_lost_by_escape(id)))
        };
        let owns = controls.iter().any(|id| owns_id(*id))
            || edits.as_ref().is_some_and(|(task, pass, ids)| {
                task == &owner
                    && ctx.cumulative_pass_nr().saturating_sub(*pass) <= 2
                    && ids.iter().any(|id| owns_id(*id))
            });
        if owns
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            && let Some(view) = self.tasks.views.get_mut(&owner)
        {
            view.checkpoint_edit = None;
        }
        if owns {
            ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
                i.consume_key(egui::Modifiers::CTRL, egui::Key::Enter);
                i.consume_key(egui::Modifiers::CTRL, egui::Key::Backspace);
            });
        }
    }
    pub(super) fn task_checkpoints(
        &mut self,
        ui: &mut egui::Ui,
        task: &Task,
        view: &mut work_item::View,
    ) {
        if view.checkpoint_snapshot != task.checkpoints {
            view.checkpoint_generation = view.checkpoint_generation.saturating_add(1);
            view.checkpoint_snapshot = task.checkpoints.clone();
            view.checkpoint_edit = None;
            view.checkpoint_delete = None;
        }
        ui.label(RichText::new("Local checkpoints").strong());
        ui.label(theme::small(
            ui,
            "User notes, not verified evidence. Saving never changes task status or runs work.",
        ));
        let mut edit_controls = Vec::new();
        for (index, note) in task.checkpoints.iter().enumerate().rev() {
            ui.label(theme::small(
                ui,
                format!(
                    "{} · observed {} · user-written",
                    chrono::DateTime::from_timestamp(note.created.min(i64::MAX as u64) as i64, 0)
                        .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
                        .unwrap_or_else(|| "Unknown time".into()),
                    note.phase.label()
                ),
            ));
            ui.add(egui::Label::new(&note.text).wrap().selectable(false));
            self.checkpoint_link_row(ui, task, view, index, &mut edit_controls);
            self.checkpoint_edit_row(ui, task, view, index, &mut edit_controls);
            self.checkpoint_delete_row(ui, task, view, index, &mut edit_controls);
        }
        if view.checkpoint_run.is_some() {
            let response = prompt_ui::button(
                ui,
                "Remove stage link",
                prompt_ui::Button::Secondary,
                prompt_ui::keyboard_available(ui),
            );
            edit_controls.push(response.id);
            if response.clicked() {
                view.checkpoint_run = None;
            }
        }
        let pass = ui.ctx().cumulative_pass_nr();
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new("checkpoint-edit-controls"),
                (task.id.clone(), pass, edit_controls),
            )
        });
        self.checkpoint_delete_dialog(ui.ctx(), task, view);
        let enabled = view.checkpoint_delete.is_none()
            && view.checkpoint_edit.is_none()
            && !self.demo
            && !self.is_mirror()
            && !self.tasks.storage_failed
            && self.tasks.store.is_some()
            && self.tasks.live.as_ref().is_none_or(|id| id == &task.id)
            && self.tasks.selected.as_ref() == Some(&task.id);
        if let Some(run) = &view.checkpoint_run {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    format!("Linked stage: {run} · navigation reference, not verified evidence"),
                ))
                .wrap(),
            );
        }
        ui.add_enabled_ui(enabled, |ui| {
            let response = ui.add(
                egui::TextEdit::multiline(&mut view.checkpoint_draft)
                    .id_salt(("task-checkpoint", &task.id))
                    .hint_text("Record progress or a handoff note…")
                    .desired_width(ui.available_width())
                    .desired_rows(2)
                    .char_limit(2048),
            );
            if response.has_focus() {
                ui.input_mut(|i| {
                    i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                    i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
                });
            }
            let valid = !view.checkpoint_draft.trim().is_empty()
                && view.checkpoint_draft.len() <= 2048
                && task.checkpoints.len() < 32;
            let save = prompt_ui::button(
                ui,
                "Save checkpoint",
                prompt_ui::Button::Secondary,
                valid && prompt_ui::keyboard_available(ui),
            );
            #[cfg(test)]
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    egui::Id::new("save-task-checkpoint"),
                    (save.id, save.rect, save.enabled()),
                )
            });
            let pass = ui.ctx().cumulative_pass_nr();
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    egui::Id::new("checkpoint-controls"),
                    (task.id.clone(), pass, [response.id, save.id]),
                )
            });
            if save.clicked() {
                if let Err(error) = self.save_checkpoint_link(
                    &task.id,
                    &view.checkpoint_draft,
                    view.checkpoint_run.as_deref(),
                ) {
                    self.tasks.error = Some(error);
                } else {
                    view.checkpoint_draft.clear();
                    view.checkpoint_run = None;
                }
            }
        });
        ui.add_space(12.0);
    }
    #[cfg(test)]
    fn save_checkpoint(&mut self, id: &str, text: &str) -> Result<(), String> {
        self.save_checkpoint_link(id, text, None)
    }
    fn save_checkpoint_link(
        &mut self,
        id: &str,
        text: &str,
        run: Option<&str>,
    ) -> Result<(), String> {
        if self.agents.exiting
            || self.demo
            || self.is_mirror()
            || self.tasks.storage_failed
            || self.tasks.store.is_none()
            || self.tasks.selected.as_deref() != Some(id)
            || self.tasks.live.as_deref().is_some_and(|live| live != id)
        {
            return Err("Checkpoint storage is unavailable for this task.".into());
        }
        let task = self
            .tasks
            .records
            .iter_mut()
            .find(|task| task.id == id)
            .ok_or("Task no longer available.")?;
        if let Some(run) = run {
            task.add_linked_checkpoint(text, run)?;
        } else {
            task.add_checkpoint(text)?;
        }
        self.tasks.save();
        Ok(())
    }
}
#[cfg(test)]
mod tests;
