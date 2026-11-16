//! Bounded local review notes. Saved-chat checkpoints are inert, never remote reviews.
use super::*;
use std::path::Path;
mod anchor;
mod cross_file;
mod deletion;
mod demo;
mod editing;
#[cfg(test)]
mod filter_tests;
mod jump;
mod markers;
mod persistence;
mod recheck;
#[cfg(test)]
mod search_tests;
#[cfg(test)]
mod single_tests;
mod ui;
use anchor::Selection;

pub(in crate::app) const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;
const MAX_COMMENTS: usize = 64;
const MAX_TEXT: usize = 2048;
const MAX_TOTAL: usize = 256 * 1024;
const MAX_METADATA: usize = 8192;
const MAX_BATCH: usize = 24 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Owner {
    root: PathBuf,
    session: String,
}
impl Owner {
    fn new(root: &Path, session: &str) -> Option<Self> {
        (root.as_os_str().len() <= 4096 && session.len() <= 2048).then(|| Self {
            root: root.to_owned(),
            session: session.to_owned(),
        })
    }
    fn bytes(&self) -> usize {
        self.root.capacity() + self.session.capacity()
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Anchor {
    owner: Owner,
    path: String,
    root: PathBuf,
    recorded: bool,
    digest: [u8; 32],
    selection: Selection,
    // No base OID: the built-in document currently provides no actual OID evidence.
}
impl Anchor {
    fn bytes(&self) -> usize {
        self.owner.bytes() + self.path.capacity() + self.root.capacity() + 256
    }
    fn source(&self) -> &'static str {
        if self.recorded {
            "Recorded edits (not net Git)"
        } else {
            "Working tree vs HEAD"
        }
    }
}
struct Comment {
    id: u64, // window-store identity; never a positional index or reused after Clear
    anchor: Anchor,
    text: String,
    resolved: bool,
    stale: bool,
}
struct Editor {
    editing: Option<u64>, // Some updates the exact saved note; never creates/reanchors
    anchor: Anchor,
    text: String,
    first: bool,
    stale: bool,
}

#[derive(Default)]
pub(in crate::app) struct Comments {
    items: Vec<Comment>,
    editor: Option<Editor>,
    owner: Option<Owner>,
    revision: Option<u64>,
    expanded: bool,
    unresolved_only: bool, // window-local presentation; never checkpointed
    query: String,         // ≤128 Unicode characters, presentation only
    clear_requested: bool,
    delete_requested: Option<u64>,
    recheck_requested: Option<(u64, egui::Id)>,
    next_id: u64,
    confirmation_pass: Option<u64>,
    error: Option<&'static str>,
    storage_notice: Option<&'static str>,
    storage_loaded: bool,
    storage_dirty: bool,
    request_add: bool,
    controls: Vec<egui::Id>,
    editor_controls: [Option<egui::Id>; 3],
    after_editor: Option<egui::Id>,
    control_keyboard_pass: Option<u64>,
    pub(super) source_root: Option<PathBuf>,
    bound_document: Option<egui::Id>,
    marker_controls: Vec<(u64, egui::Id)>,
    marker_document: Option<egui::Id>,
    reveal_note: Option<(u64, egui::Id, u64)>,
    recorded_targets: Vec<(u64, usize)>,
    recorded_binding: Option<(egui::Id, u64)>,
    recorded_open: Option<cross_file::Open>,
}
impl Comments {
    #[cfg(test)]
    pub(in crate::app) fn test_seed(&mut self, root: &Path, session: &str) {
        self.bind(root, session, 1);
        let diff = "@@ -1 +1 @@\n-old\n+new\n";
        assert!(self.begin("file.rs", diff, true, Some([1; 32]), 31..34));
        self.editor.as_mut().unwrap().text = "kept local note".into();
        assert!(self.save());
    }
    #[cfg(test)]
    pub(in crate::app) fn test_count(&self) -> usize {
        self.items.len()
    }
    pub(in crate::app) fn editor_owns_keyboard(&self, ctx: &egui::Context) -> bool {
        self.editor
            .as_ref()
            .is_some_and(|editor| Some(&editor.anchor.owner) == self.owner.as_ref())
            && ctx.memory(|memory| {
                memory
                    .focused()
                    .is_some_and(|id| self.editor_controls.contains(&Some(id)))
            })
    }
    /// Root/session changes never move or discard notes. They remain counted until explicit Clear.
    pub(super) fn bind(&mut self, root: &Path, session: &str, revision: u64) -> bool {
        if self
            .owner
            .as_ref()
            .is_none_or(|owner| owner.root != root || owner.session != session)
        {
            self.stale(None);
            self.cancel_confirmation();
            self.owner = Owner::new(root, session);
            self.revision = Some(revision);
            self.expanded = false;
            self.unresolved_only = false;
            self.query = String::new();
            self.request_add = false;
            self.controls.clear();
            self.marker_controls.clear();
            self.marker_document = None;
            self.reveal_note = None;
            self.clear_recorded_targets();
        } else if self.revision != Some(revision) {
            self.stale(Some(true));
            self.revision = Some(revision);
            return true;
        }
        false
    }
    pub(super) fn stale(&mut self, recorded: Option<bool>) {
        self.clear_recorded_targets();
        for comment in &mut self.items {
            if recorded.is_none_or(|source| comment.anchor.recorded == source) {
                comment.stale = true;
            }
        }
        if let Some(editor) = &mut self.editor
            && recorded.is_none_or(|source| editor.anchor.recorded == source)
        {
            editor.stale = true;
        }
    }
    fn bytes(&self) -> usize {
        self.items
            .iter()
            .map(|item| item.anchor.bytes() + item.text.capacity())
            .sum::<usize>()
            + self
                .editor
                .as_ref()
                .map_or(0, |editor| editor.anchor.bytes() + editor.text.capacity())
    }
    fn bind_document(&mut self, path: &str, recorded: bool, navigation: &diff_viewer::Navigation) {
        if self.bound_document != Some(navigation.focus_id()) {
            self.observe(path, recorded, navigation.comment_digest());
            self.bound_document = Some(navigation.focus_id());
        }
    }
    fn observe(&mut self, path: &str, recorded: bool, digest: Option<[u8; 32]>) {
        for item in &mut self.items {
            if Some(&item.anchor.owner) == self.owner.as_ref()
                && item.anchor.path == path
                && item.anchor.recorded == recorded
                && Some(item.anchor.digest) != digest
            {
                item.stale = true;
            }
        }
        if let Some(editor) = &mut self.editor
            && Some(&editor.anchor.owner) == self.owner.as_ref()
            && editor.anchor.path == path
            && editor.anchor.recorded == recorded
            && Some(editor.anchor.digest) != digest
        {
            editor.stale = true;
        }
    }
    fn begin(
        &mut self,
        path: &str,
        diff: &str,
        recorded: bool,
        digest: Option<[u8; 32]>,
        range: std::ops::Range<usize>,
    ) -> bool {
        let Some(owner) = self.owner.clone() else {
            return false;
        };
        if self.editor.is_some() {
            self.error = Some("Finish or cancel the existing local comment first.");
            return false;
        }
        let Some(digest) = digest else {
            self.error = Some("Comments require a diff no larger than 2 MiB.");
            return false;
        };
        let Some(selection) = anchor::selected(diff, range) else {
            self.error = Some(
                "Select source text within one hunk; headers and cross-hunk ranges are unsupported.",
            );
            return false;
        };
        let root = self.source_root.as_ref().unwrap_or(&owner.root);
        if root.as_os_str().len() > 4096
            || owner
                .bytes()
                .saturating_add(path.len())
                .saturating_add(root.as_os_str().len())
                .saturating_add(256)
                > MAX_METADATA
        {
            self.error = Some("Comment metadata exceeds the 8 KiB limit.");
            return false;
        }
        let anchor = Anchor {
            root: root.clone(),
            owner,
            path: path.into(),
            recorded,
            digest,
            selection,
        };
        if anchor.bytes() > MAX_METADATA
            || self.items.len() >= MAX_COMMENTS
            || self.bytes() + anchor.bytes() + MAX_TEXT > MAX_TOTAL
        {
            self.error = Some(
                "Local review limit reached: 64 comments, 2 KiB each, 256 KiB including metadata.",
            );
            return false;
        }
        self.editor = Some(Editor {
            editing: None,
            anchor,
            text: String::new(),
            first: true,
            stale: false,
        });
        self.expanded = true;
        self.error = None;
        true
    }
    fn save(&mut self) -> bool {
        if let Some(editor) = self.editor.as_mut()
            && editor.text.capacity() > MAX_TEXT
        {
            editor.text.shrink_to_fit();
        }
        let Some(editor) = self.editor.as_ref() else {
            return false;
        };
        if editor.text.trim().is_empty() || editor.text.len() > MAX_TEXT || self.bytes() > MAX_TOTAL
        {
            return false;
        }
        if let Some(id) = editor.editing {
            return self.save_edit(id);
        }
        if self.items.len() >= MAX_COMMENTS {
            return false;
        }
        let Some(id) = self.next_id.checked_add(1) else {
            self.error = Some("Local note identity limit reached. Existing notes are unchanged.");
            return false;
        };
        self.next_id = id;
        self.storage_dirty = true;
        let editor = self.editor.take().unwrap();
        self.items.push(Comment {
            id,
            anchor: editor.anchor,
            text: editor.text,
            resolved: false,
            stale: editor.stale,
        });
        true
    }
    fn one_note(&self, id: u64) -> Result<String, &'static str> {
        let item = self
            .items
            .iter()
            .find(|item| item.id == id && Some(&item.anchor.owner) == self.owner.as_ref())
            .ok_or("Local note is unavailable for this repository/session.")?;
        let text = Self::format_note(item);
        if text.len() > MAX_BATCH {
            return Err("Note exceeds 24 KiB; nothing added.");
        }
        Ok(text)
    }
    fn format_note(item: &Comment) -> String {
        let anchor = &item.anchor;
        let digest = anchor
            .digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!(
            "{} · {} · {}{}{}\nRoot: {}\nSession: {}\nSHA-256: {digest}\nSelected display characters: {}–{} · hunk {}\n{}\n\n",
            anchor.path,
            anchor.source(),
            anchor.selection.label(),
            if item.stale {
                " · STALE (original snapshot; not retargeted)"
            } else {
                " · original snapshot"
            },
            if item.resolved { " · RESOLVED" } else { "" },
            anchor.root.display(),
            anchor.owner.session,
            anchor.selection.chars.start,
            anchor.selection.chars.end,
            anchor.selection.hunk,
            item.text
        )
    }
    fn batch(&self) -> Result<String, &'static str> {
        let Some(owner) = self.owner.as_ref() else {
            return Err("Repository/session identity is unavailable.");
        };
        let mut text = String::new();
        for item in self
            .items
            .iter()
            .filter(|item| &item.anchor.owner == owner && !item.resolved)
        {
            let entry = Self::format_note(item);
            if text.len() + entry.len() > MAX_BATCH {
                return Err(
                    "Unresolved batch exceeds 24 KiB. Resolve some notes before adding a complete batch.",
                );
            }
            text.push_str(&entry);
        }
        if text.is_empty() {
            Err("No unresolved comments for this repository/session.")
        } else {
            Ok(text)
        }
    }
}

impl Changes {
    /// Compatibility name for the agent-question gate: includes this pass's
    /// actual inline Changes controls/source owner, not hidden rows or menus.
    pub(in crate::app) fn local_comment_editor_owns_keyboard(&self, ctx: &egui::Context) -> bool {
        self.inline.owns_keyboard(ctx)
            || self.review.is_some()
                && (self.review_comments.editor_owns_keyboard(ctx)
                    || self.review_comments.confirming()
                    || self.review_comments.confirmation_pass == Some(ctx.cumulative_pass_nr())
                    || self.review_comments.control_keyboard_pass == Some(ctx.cumulative_pass_nr()))
    }
    /// Shared freshness hook. Git read freshness/HEAD observations can conservatively call this.
    pub(in crate::app) fn stale_review_comments(&mut self) {
        self.review_comments.stale(None);
        self.review_comments.bound_document = None;
        if let Some(document) = &mut self.review {
            document.2.stale_comments();
        }
    }
}

#[cfg(test)]
mod lifetime_tests;
#[cfg(test)]
mod ownership_tests;
#[cfg(test)]
mod question_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
