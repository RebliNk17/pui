//! Explicit parent base; changing presentation never schedules a read.
use super::*;
#[derive(Default)]
pub(super) struct Comparison {
    pub(super) base: usize,
    pub(super) controls: Vec<egui::Id>,
    pub(super) key: Option<egui::Id>,
}
impl Comparison {
    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        detail: &workspace::history::details::Detail,
    ) -> bool {
        let key = self.key.take();
        self.controls.clear();
        let mut changed = false;
        if detail.parents.len() > 1 {
            ui.horizontal_wrapped(|ui|{ui.label(theme::small(ui,"Compare with"));for(index,parent)in detail.parents.iter().enumerate(){let response=ui.push_id(("commit-base",&detail.oid,parent),|ui|ui.add(egui::Button::selectable(self.base==index,theme::small(ui,format!("Parent {}",index+1))))).inner.on_hover_text(format!("Compare with exact parent {parent}. Clears loaded files/patch; does not read Git until Load is selected."));self.controls.push(response.id);
 #[cfg(test)]ui.ctx().data_mut(|d|d.insert_temp(popup_id().with(("commit-base",index)),(response.id,response.rect)));
 if (response.clicked()||key==Some(response.id))&&self.base!=index{self.base=index;changed=true;}}});
        }
        changed
    }
}
#[cfg(test)]
mod tests;
