//! Section removal is metadata-only, but remains an explicit, safely focused action.
use super::*;

impl Pui {
    pub(in crate::app) fn sidebar_section_confirmation(&mut self, ctx: &egui::Context) {
        let Some(id) = self.section_view.remove.clone() else {
            return;
        };
        let Some(section) = self
            .prefs
            .sidebar_view
            .sections
            .iter()
            .find(|section| section.id == id)
        else {
            self.section_view.remove = None;
            return;
        };
        let name = section.name.clone();
        let choice = confirmations::Dialog::new(
            "remove-sidebar-section",
            "Remove this section?",
            "Chats, files and agent trees are kept. Members return to the selected grouping.",
            "Keep section",
            "Remove section",
        )
        .show(ctx, |ui| {
            confirmations::note(ui, &name);
        });
        match choice {
            Some(confirmations::Choice::Confirm) => {
                self.remove_sidebar_section(&id);
                self.section_view.remove = None;
            }
            Some(confirmations::Choice::Cancel) => self.section_view.remove = None,
            None => {}
        }
    }
}
