//! Explicit exact-snapshot recorded-note navigation. No disk/Git read or reanchoring.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Open {
    note: u64,
    position: usize, // reverse recorded list order; never trusted without recheck
    document: egui::Id,
    revision: u64,
    pass: u64,
}
fn bounded(outputs: &[RecordedFile]) -> bool {
    outputs.len() <= 256
        && outputs
            .iter()
            .all(|file| file.path.len() <= 4096 && file.diff.len() <= MAX_DIFF_BYTES)
}
impl Comments {
    pub(in crate::app::changes) fn clear_recorded_targets(&mut self) {
        self.recorded_targets.clear();
        self.recorded_binding = None;
        self.recorded_open = None;
    }
    fn recorded_note(&self, item: &Comment) -> bool {
        !item.stale
            && item.anchor.recorded
            && Some(&item.anchor.owner) == self.owner.as_ref()
            && self.source_root.as_ref() == Some(&item.anchor.root)
            && item.anchor.root == item.anchor.owner.root
    }
    /// Cheap bounded path/owner projection only. Hash/range validation waits for
    /// explicit activation; never hash every note's file during idle rendering.
    pub(in crate::app::changes) fn prepare_recorded_targets(
        &mut self,
        outputs: &[RecordedFile],
        revision: u64,
        document: &diff_viewer::Document,
        available: bool,
    ) {
        self.recorded_targets.clear();
        if !available || !document.3 || self.confirming() || !bounded(outputs) {
            self.recorded_binding = None;
            self.recorded_open = None;
            return;
        }
        self.recorded_binding = Some((document.2.focus_id(), revision));
        for item in &self.items {
            if !self.recorded_note(item) || item.anchor.path == document.0 {
                continue;
            }
            let mut matches = outputs
                .iter()
                .rev()
                .enumerate()
                .filter(|(_, file)| file.path == item.anchor.path);
            if let Some((position, _)) = matches.next()
                && matches.next().is_none()
            {
                self.recorded_targets.push((item.id, position));
            }
        }
    }
    pub(super) fn request_recorded_open(&mut self, id: u64, document: egui::Id, pass: u64) -> bool {
        let Some((bound, revision)) = self
            .recorded_binding
            .filter(|(bound, _)| *bound == document)
        else {
            return false;
        };
        let Some((_, position)) = self.recorded_targets.iter().find(|(note, _)| *note == id) else {
            return false;
        };
        self.recorded_open = Some(Open {
            note: id,
            position: *position,
            document: bound,
            revision,
            pass,
        });
        true
    }
    pub(in crate::app::changes) fn take_recorded_open(
        &mut self,
        ctx: &egui::Context,
        outputs: &[RecordedFile],
        revision: u64,
        document: &diff_viewer::Document,
    ) -> Option<(usize, diff_viewer::Document)> {
        let requested = self.recorded_open.take()?;
        if requested.pass != ctx.cumulative_pass_nr()
            || requested.document != document.2.focus_id()
            || requested.revision != revision
            || self.recorded_binding != Some((requested.document, revision))
            || !document.3
            || self.confirming()
            || !bounded(outputs)
            || egui::Popup::is_any_open(ctx)
            || ctx.memory(|memory| memory.top_modal_layer().is_some())
        {
            return None;
        }
        let item = self
            .items
            .iter()
            .find(|item| item.id == requested.note && self.recorded_note(item))?;
        let file = outputs.iter().rev().nth(requested.position)?;
        if file.path != item.anchor.path
            || file.diff.len() > MAX_DIFF_BYTES
            || file.path.len() > 4096
            || outputs
                .iter()
                .filter(|candidate| candidate.path == file.path)
                .count()
                != 1
        {
            return None;
        }
        // Compute the target's immutable navigation/digest once on explicit Open,
        // before cloning its body. No full-list body snapshots or text matching.
        let mut navigation = diff_viewer::Navigation::new(&file.diff);
        if navigation.comment_digest() != Some(item.anchor.digest)
            || anchor::selected(&file.diff, item.anchor.selection.chars.clone()).as_ref()
                != Some(&item.anchor.selection)
        {
            // A failed explicit comparison is freshness evidence, not a chance
            // to guess a new anchor. Mark this original note stale monotonically.
            let note = item.id;
            if let Some(item) = self.items.iter_mut().find(|item| item.id == note) {
                item.stale = true;
            }
            if let Some(editor) = self
                .editor
                .as_mut()
                .filter(|editor| editor.editing == Some(note))
            {
                editor.stale = true;
            }
            self.error =
                Some("The original recorded snapshot is unavailable. The note was not reattached.");
            return None;
        }
        let row = item.anchor.selection.rows.0.checked_sub(1)?;
        // All identity/source/range checks precede the deliberate focus transfer
        // and reveal. It clears only the previous document's text selection.
        if !navigation.reveal_comment(ctx, row) {
            return None;
        }
        let target = diff_viewer::Document(file.path.clone(), file.diff.clone(), navigation, true);
        self.recorded_targets.clear();
        self.recorded_binding = None;
        self.controls.clear();
        self.marker_controls.clear();
        self.marker_document = None;
        self.reveal_note = None;
        self.bound_document = None;
        Some((requested.position, target))
    }
}

impl Pui {
    pub(in crate::app) fn demo_review_note_files(&mut self) {
        if !self.demo {
            return;
        }
        self.changes.review_comments.editor = None;
        let path = "src/another-recorded-file.rs";
        let source = "@@ -1 +1 @@\n-before\n+after\n";
        self.outputs.push(RecordedFile {
            path: path.into(),
            diff: source.into(),
            ..Default::default()
        });
        let owner = self.review_comment_owner();
        self.changes.open_review(
            &self.ctx,
            (path.into(), source.into()).into(),
            true,
            &self.prefs.cwd,
            &owner,
        );
        self.changes.review_comments.expanded = true;
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
