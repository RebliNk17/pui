//! Local run references, not assertions about its outcome or executable intent.
use super::*;
impl Pui {
    pub(in crate::app::tasks) fn checkpoint_link_action(
        &mut self,
        ui: &mut egui::Ui,
        task: &Task,
        view: &mut work_item::View,
        run: &Run,
    ) {
        let enabled = self.checkpoint_editable(&task.id)
            && view.checkpoint_draft.is_empty()
            && view.checkpoint_edit.is_none()
            && view.checkpoint_delete.is_none()
            && run.id.len() <= 128
            && task.runs.iter().filter(|r| r.id == run.id).count() == 1
            && prompt_ui::keyboard_available(ui)
            && ui.memory(|m| m.top_modal_layer().is_none())
            && !egui::Popup::is_any_open(ui.ctx());
        let response=ui.push_id(("checkpoint-link-run",&task.id,&run.id),|ui|prompt_ui::button(ui,"Write note linked to this stage",prompt_ui::Button::Secondary,enabled)).inner.on_hover_text("Switch to Notes with this exact recorded stage attached. Nothing saved until Save checkpoint. Existing unfinished note must be saved or cleared first.");
        #[cfg(test)]
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new("checkpoint-link-action"),
                (response.id, response.rect, response.enabled()),
            )
        });
        let pass = ui.ctx().cumulative_pass_nr();
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new("checkpoint-link-control"),
                (task.id.clone(), pass, response.id),
            )
        });
        if response.clicked() {
            view.checkpoint_run = Some(run.id.clone());
            view.tab = work_item::Tab::Checkpoints;
        }
    }
    pub(super) fn checkpoint_link_row(
        &mut self,
        ui: &mut egui::Ui,
        task: &Task,
        view: &mut work_item::View,
        index: usize,
        controls: &mut Vec<egui::Id>,
    ) {
        let Some(id) = &task.checkpoints[index].run else {
            return;
        };
        let mut runs = task.runs.iter().filter(|r| &r.id == id);
        let run = runs.next().filter(|_| runs.next().is_none());
        let enabled = run.is_some()
            && view.checkpoint_edit.is_none()
            && view.checkpoint_delete.is_none()
            && prompt_ui::keyboard_available(ui)
            && ui.memory(|m| m.top_modal_layer().is_none())
            && !egui::Popup::is_any_open(ui.ctx());
        let label = run
            .map(|r| {
                format!(
                    "Inspect linked stage · {} · attempt {}",
                    r.role.label(),
                    u16::from(r.attempt) + 1
                )
            })
            .unwrap_or_else(|| "Linked stage unavailable".into());
        let response = ui
            .push_id(
                (
                    "checkpoint-inspect-run",
                    &task.id,
                    view.checkpoint_generation,
                    index,
                    id,
                ),
                |ui| prompt_ui::button(ui, &label, prompt_ui::Button::Secondary, enabled),
            )
            .inner;
        controls.push(response.id);
        #[cfg(test)]
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new(("checkpoint-inspect-run", index)),
                (response.id, response.rect, response.enabled()),
            )
        });
        if response.clicked() {
            self.tasks.selected_run = Some(id.clone());
            view.tab = work_item::Tab::Activity;
        }
    }
}
#[cfg(test)]
mod tests;
