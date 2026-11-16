//! Explicit exact-original recorded snapshot re-review, never similar-text reattachment.
use super::*;
impl Comments {
    pub(super) fn can_recheck(
        &self,
        id: u64,
        path: &str,
        recorded: bool,
        diff: &str,
        navigation: &diff_viewer::Navigation,
    ) -> bool {
        if !recorded || self.editor.is_some() {
            return false;
        }
        self.items.iter().any(|item| {
            item.id == id
                && item.stale
                && item.anchor.recorded
                && Some(&item.anchor.owner) == self.owner.as_ref()
                && item.anchor.path == path
                && self.source_root.as_ref() == Some(&item.anchor.root)
                && Some(item.anchor.digest) == navigation.comment_digest()
                && anchor::selected(diff, item.anchor.selection.chars.clone()).as_ref()
                    == Some(&item.anchor.selection)
        })
    }
    pub(super) fn recheck_dialog(
        &mut self,
        ctx: &egui::Context,
        path: &str,
        diff: &str,
        recorded: bool,
        navigation: &diff_viewer::Navigation,
    ) {
        let Some((id, document)) = self.recheck_requested else {
            return;
        };
        if document != navigation.focus_id()
            || !self.can_recheck(id, path, recorded, diff, navigation)
        {
            self.recheck_requested = None;
            return;
        }
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("recheck-recorded-note"),
        );
        if egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.recheck_requested = None;
            return;
        }
        self.confirmation_pass = Some(ctx.cumulative_pass_nr());
        ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        let choice=confirmations::Dialog::new("recheck-recorded-note","Mark original snapshot re-reviewed?","This exact recorded diff and original range match the saved note. Clears STALE only for this note. It does not verify the working tree, change its anchor/text/resolved state, or send anything.","Cancel","Mark re-reviewed").destructive(false).show(ctx,|ui|{let item=self.items.iter().find(|item|item.id==id).unwrap();ui.add(egui::Label::new(theme::small(ui,format!("{} · {}",item.anchor.path,item.anchor.selection.label()))).wrap());ui.add(egui::Label::new(&item.text).wrap().selectable(false));});
        match choice {
            Some(confirmations::Choice::Confirm) => {
                self.recheck_requested = None;
                if self.can_recheck(id, path, recorded, diff, navigation) {
                    self.items
                        .iter_mut()
                        .find(|item| item.id == id)
                        .unwrap()
                        .stale = false;
                    self.storage_dirty = true;
                }
            }
            Some(confirmations::Choice::Cancel) => self.recheck_requested = None,
            None => {}
        }
        ctx.input_mut(|i| {
            i.events.retain(|e| {
                !matches!(
                    e,
                    egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Paste(_)
                )
            })
        });
    }
}
#[cfg(test)]
mod tests;
