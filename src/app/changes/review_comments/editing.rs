//! Transactional text-only edits, retaining the exact original saved anchor.
use super::*;
impl Comments {
    pub(super) fn begin_edit(&mut self, id: u64) -> bool {
        if self.confirming() || self.editor.is_some() {
            self.error = Some("Finish or cancel the existing local comment first.");
            return false;
        }
        let Some(item) = self
            .items
            .iter()
            .find(|item| item.id == id && Some(&item.anchor.owner) == self.owner.as_ref())
        else {
            return false;
        };
        // Editing retains the saved note unchanged until Save. Reserve the
        // bounded pending clone/growth too, not just its final replacement.
        if self
            .bytes()
            .saturating_add(item.anchor.bytes())
            .saturating_add(MAX_TEXT)
            > MAX_TOTAL
        {
            self.error =
                Some("Local review payload is full. Delete another note before editing this one.");
            return false;
        }
        self.editor = Some(Editor {
            editing: Some(id),
            anchor: item.anchor.clone(),
            text: item.text.clone(),
            first: true,
            stale: item.stale,
        });
        self.editor_controls = [None; 3];
        self.expanded = true;
        self.error = None;
        true
    }
    pub(super) fn save_edit(&mut self, id: u64) -> bool {
        let Some(editor) = self
            .editor
            .as_ref()
            .filter(|editor| editor.editing == Some(id))
        else {
            return false;
        };
        // A new active owner may not save an invisible parked edit. No index or
        // text matching, replacement-note adoption or anchor relocation.
        if Some(&editor.anchor.owner) != self.owner.as_ref() || self.confirming() {
            self.error = Some("Return to this note's original repository/session before saving.");
            return false;
        }
        let Some(index) = self
            .items
            .iter()
            .position(|item| item.id == id && item.anchor.owner == editor.anchor.owner)
        else {
            self.error = Some(
                "The original note is unavailable. Cancel this edit; it will not recreate a deleted note.",
            );
            return false;
        };
        let editor = self.editor.take().unwrap();
        self.items[index].text = editor.text;
        self.storage_dirty = true;
        self.items[index].stale |= editor.stale;
        // ID, original anchor/digest/coordinates, source and resolved state stay.
        self.editor_controls = [None; 3];
        self.error = None;
        true
    }
}
impl Changes {
    pub(in crate::app) fn demo_review_note_edit(&mut self) {
        self.review_comments.editor = None;
        if let Some(id) = self.review_comments.items.first().map(|item| item.id) {
            self.review_comments.begin_edit(id);
        }
    }
}
#[cfg(test)]
mod tests;
