//! Explicit navigation only. Never finds similar text, opens files or reanchors notes.
use super::*;

impl Comments {
    pub(super) fn jump_available(
        &self,
        id: u64,
        path: &str,
        recorded: bool,
        navigation: &diff_viewer::Navigation,
    ) -> bool {
        self.items.iter().any(|item| {
            item.id == id
                && !item.stale
                && Some(&item.anchor.owner) == self.owner.as_ref()
                && item.anchor.path == path
                && item.anchor.recorded == recorded
                && Some(item.anchor.digest) == navigation.comment_digest()
                && self.source_root.as_ref() == Some(&item.anchor.root)
        })
    }
    pub(super) fn jump(
        &self,
        id: u64,
        ctx: &egui::Context,
        path: &str,
        diff: &str,
        recorded: bool,
        navigation: &mut diff_viewer::Navigation,
    ) -> bool {
        if self.confirming() || !self.jump_available(id, path, recorded, navigation) {
            return false;
        }
        let Some(item) = self.items.iter().find(|item| item.id == id) else {
            return false;
        };
        // Revalidate the original character range only on explicit activation.
        // Cached document digest is computed once when the immutable review opens.
        if anchor::selected(diff, item.anchor.selection.chars.clone()).as_ref()
            != Some(&item.anchor.selection)
        {
            return false;
        }
        let Some(row) = item.anchor.selection.rows.0.checked_sub(1) else {
            return false;
        };
        navigation.reveal_comment(ctx, row)
    }
}

#[cfg(test)]
mod tests;
