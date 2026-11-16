//! Presentation-only editor; exact original note is required at publication.
use super::*;
pub(in crate::app::tasks) struct Editor {
    index: usize,
    original: domain::checkpoints::Checkpoint,
    text: String,
}
impl Pui {
    pub(super) fn checkpoint_editable(&self, id: &str) -> bool {
        !self.demo
            && !self.is_mirror()
            && !self.agents.exiting
            && !self.tasks.storage_failed
            && self.tasks.store.is_some()
            && self.tasks.selected.as_deref() == Some(id)
            && self.tasks.live.as_deref().is_none_or(|live| live == id)
    }
    pub(super) fn checkpoint_edit_row(
        &mut self,
        ui: &mut egui::Ui,
        task: &Task,
        view: &mut work_item::View,
        index: usize,
        controls: &mut Vec<egui::Id>,
    ) {
        let note = &task.checkpoints[index];
        let enabled = view.checkpoint_delete.is_none()
            && self.checkpoint_editable(&task.id)
            && prompt_ui::keyboard_available(ui)
            && ui.memory(|m| m.top_modal_layer().is_none())
            && !egui::Popup::is_any_open(ui.ctx());
        if view
            .checkpoint_edit
            .as_ref()
            .is_some_and(|e| task.checkpoints.get(e.index) != Some(&e.original))
        {
            view.checkpoint_edit = None;
            self.tasks.error =
                Some("Checkpoint changed; editor cancelled. Saved text is unchanged.".into());
        }
        ui.push_id(
            (
                "checkpoint-edit",
                &task.id,
                view.checkpoint_generation,
                index,
                note.created,
                &note.text,
            ),
            |ui| {
                if view
                    .checkpoint_edit
                    .as_ref()
                    .is_some_and(|e| e.index == index)
                {
                    let edit = view.checkpoint_edit.as_mut().unwrap();
                    let mut cancel = false;
                    let mut save = false;
                    ui.add_enabled_ui(enabled, |ui| {
                        let input = ui.add(
                            egui::TextEdit::multiline(&mut edit.text)
                                .desired_width(ui.available_width())
                                .desired_rows(3)
                                .char_limit(2048),
                        );
                        controls.push(input.id);
                        if input.has_focus() {
                            ui.input_mut(|i| {
                                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                                i.consume_key(egui::Modifiers::CTRL, egui::Key::Enter);
                            });
                        }
                        ui.horizontal_wrapped(|ui| {
                            let response = prompt_ui::button(
                                ui,
                                "Save edit",
                                prompt_ui::Button::Secondary,
                                !edit.text.trim().is_empty() && edit.text.len() <= 2048,
                            );
                            controls.push(response.id);
                            save = response.clicked();
                            #[cfg(test)]
                            ui.ctx().data_mut(|d| {
                                d.insert_temp(
                                    egui::Id::new("checkpoint-edit-save"),
                                    (response.id, response.rect, response.enabled()),
                                )
                            });
                            let response = prompt_ui::button(
                                ui,
                                "Cancel edit",
                                prompt_ui::Button::Secondary,
                                true,
                            );
                            controls.push(response.id);
                            cancel = response.clicked();
                        });
                    });
                    if cancel {
                        view.checkpoint_edit = None;
                    } else if save {
                        let edit = view.checkpoint_edit.as_ref().unwrap();
                        match self.save_checkpoint_edit(&task.id, index, &edit.original, &edit.text)
                        {
                            Ok(()) => view.checkpoint_edit = None,
                            Err(error) => self.tasks.error = Some(error),
                        }
                    }
                } else {
                    let response = prompt_ui::button(
                        ui,
                        "Edit note…",
                        prompt_ui::Button::Secondary,
                        enabled && view.checkpoint_edit.is_none(),
                    );
                    controls.push(response.id);
                    #[cfg(test)]
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(
                            egui::Id::new(("checkpoint-edit-open", index)),
                            (response.id, response.rect, response.enabled()),
                        )
                    });
                    if response.clicked() {
                        view.checkpoint_edit = Some(Editor {
                            index,
                            original: note.clone(),
                            text: note.text.clone(),
                        });
                    }
                }
            },
        );
    }
    fn save_checkpoint_edit(
        &mut self,
        id: &str,
        index: usize,
        original: &domain::checkpoints::Checkpoint,
        text: &str,
    ) -> Result<(), String> {
        if !self.checkpoint_editable(id) {
            return Err("Checkpoint storage unavailable for this task.".into());
        }
        let task = self
            .tasks
            .records
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or("Task unavailable.")?;
        task.edit_checkpoint(index, original, text)?;
        self.tasks.save();
        Ok(())
    }
}
#[cfg(test)]
mod tests;
