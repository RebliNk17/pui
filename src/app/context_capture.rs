//! Explicit context actions append bounded excerpts to the draft, never directly to Pi.
use super::*;
pub(in crate::app) mod browser;
mod chips;
mod diff;
mod file_blocks;
pub(super) use chips::Draft;
pub(crate) use file_blocks::offer as offer_file_block;
pub(super) use file_blocks::show as show_file_blocks;
mod rendered;
mod source;
pub(super) use diff::{demo as demo_diff, offer as offer_diff_selection};
pub(super) use rendered::demo as demo_rendered;
pub(crate) use rendered::offer as offer_rendered_selection;
pub(in crate::app) use source::console_menu as offer_console_selection;
pub(super) use source::{demo as demo_source, offer as offer_source_selection};

pub(super) fn clear_source(ctx: &egui::Context) {
    source::clear(ctx);
    file_blocks::clear(ctx);
    browser::clear(ctx);
    ctx.data_mut(|data| {
        data.remove::<Snippet>(egui::Id::new("pui-context-capture"));
        data.remove::<(PathBuf, String)>(egui::Id::new("pui-review-capture-owner"));
    });
}

pub(super) fn inputs(ctx: &egui::Context) {
    file_blocks::inputs(ctx);
    source::inputs(ctx);
}

pub(in crate::app) struct ParkedCapture {
    snippet: Option<Snippet>,
    owner: Option<(PathBuf, String)>,
}
pub(in crate::app) fn park_capture(ctx: &egui::Context) -> ParkedCapture {
    ctx.data_mut(|data| ParkedCapture {
        snippet: data.remove_temp(egui::Id::new("pui-context-capture")),
        owner: data.remove_temp(egui::Id::new("pui-review-capture-owner")),
    })
}
pub(in crate::app) fn restore_capture(ctx: &egui::Context, capture: ParkedCapture) {
    ctx.data_mut(|data| {
        if let Some(snippet) = capture.snippet {
            data.insert_temp(egui::Id::new("pui-context-capture"), snippet);
        }
        if let Some(owner) = capture.owner {
            data.insert_temp(egui::Id::new("pui-review-capture-owner"), owner);
        }
    });
}

#[derive(Clone, Default)]
struct Snippet {
    label: String,
    text: String,
}
const MAX_BYTES: usize = 24 * 1024;

/// Bind file-render captures to the exact chat before any later conversation input.
pub(in crate::app) fn bind_file_owner(ctx: &egui::Context, root: &std::path::Path, session: &str) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("pui-file-capture-owner"),
            (root.to_path_buf(), session.to_owned(), pass),
        )
    });
}
pub(in crate::app) fn queue_file_selection(ctx: &egui::Context, label: &str, source: &str) -> bool {
    let pass = ctx.cumulative_pass_nr();
    let owner = ctx.data(|data| {
        data.get_temp::<(PathBuf, String, u64)>(egui::Id::new("pui-file-capture-owner"))
    });
    let Some((root, session, _)) = owner.filter(|(_, _, bound)| *bound == pass) else {
        return false;
    };
    queue_review(ctx, &root, &session, label, source);
    true
}
pub(crate) fn queue(ctx: &egui::Context, label: &str, source: &str) {
    queue_parts(ctx, label, std::iter::once(source));
}

/// Local-review batching retains exact draft ownership across the deferred collection step.
/// This is still the same bounded context queue, never a send/execute/approval action.
pub(in crate::app) fn queue_review(
    ctx: &egui::Context,
    root: &std::path::Path,
    session: &str,
    label: &str,
    source: &str,
) {
    queue(ctx, label, source);
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("pui-review-capture-owner"),
            (root.to_path_buf(), session.to_owned()),
        )
    });
}

pub(crate) fn queue_parts<'a>(
    ctx: &egui::Context,
    label: &str,
    parts: impl IntoIterator<Item = &'a str>,
) {
    ctx.data_mut(|data| {
        data.remove::<(PathBuf, String)>(egui::Id::new("pui-review-capture-owner"))
    });
    let mut text = String::new();
    for part in parts {
        if part.is_empty() {
            continue;
        }
        if !text.is_empty() {
            if text.len() + 2 >= MAX_BYTES {
                text.push_str("\n[Context excerpt limited to 24 KiB]");
                break;
            }
            text.push_str("\n\n");
        }
        let mut end = part.len().min(MAX_BYTES.saturating_sub(text.len()));
        while !part.is_char_boundary(end) {
            end -= 1;
        }
        text.push_str(&part[..end]);
        if end < part.len() || text.len() >= MAX_BYTES {
            text.push_str("\n[Context excerpt limited to 24 KiB]");
            break;
        }
    }
    if text.is_empty() {
        return;
    }
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("pui-context-capture"),
            Snippet {
                label: label
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(512)
                    .collect(),
                text,
            },
        )
    });
}
impl Pui {
    pub(super) fn prepare_file_block_capture(&mut self, ctx: &egui::Context) {
        file_blocks::prepare(self, ctx);
        browser::prepare(self, ctx);
    }
    /// Runtime owner for local review: an unsaved chat is not the shared string "new".
    pub(super) fn review_comment_owner(&mut self) -> String {
        self.state
            .session_file
            .clone()
            .unwrap_or_else(|| format!("agent:{}", self.ensure_agent()))
    }
    pub(super) fn collect_context_capture(&mut self) {
        file_blocks::collect(self);
        browser::collect(self);
        source::finish(&self.ctx);
        self.collect_queued_context_capture();
    }
    /// Drain only this transcript's queued excerpt; global file/selection
    /// lifecycle collection runs once after every pane has rendered.
    pub(in crate::app) fn collect_queued_context_capture(&mut self) {
        let snippet = self
            .ctx
            .data_mut(|data| data.remove_temp::<Snippet>(egui::Id::new("pui-context-capture")));
        let owner = self.ctx.data_mut(|data| {
            data.remove_temp::<(PathBuf, String)>(egui::Id::new("pui-review-capture-owner"))
        });
        let Some(snippet) = snippet else {
            return;
        };
        if owner.is_some_and(|(root, session)| {
            root != self.prefs.cwd || session != self.review_comment_owner()
        }) {
            self.state.error = Some("Local review context was not added: repository/session changed. Original notes remain in Changes.".into());
            return;
        }
        if let Err(error) = self.context_chips.append(&mut self.draft, snippet) {
            self.state.error = Some(error.into());
            return;
        }
        self.focus_composer = true;
        self.ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captured_context_is_bounded_and_unicode_safe() {
        let ctx = egui::Context::default();
        queue(&ctx, "file.rs:4–8", &"שלום".repeat(20000));
        let snippet = ctx
            .data_mut(|d| d.remove_temp::<Snippet>(egui::Id::new("pui-context-capture")))
            .unwrap();
        assert!(snippet.text.len() < MAX_BYTES + 100);
        assert!(snippet.text.contains("limited"));
    }

    #[test]
    fn capture_appends_to_draft_preserves_chat_and_never_auto_sends() {
        let ctx = egui::Context::default();
        let mut app = crate::app::files::tests::app(&ctx);
        app.draft = "Keep this draft".into();
        app.state.busy = true;
        let messages = app.state.messages.len();
        queue(&ctx, "src/file.rs:4–8", "Selected text");
        app.collect_context_capture();
        assert!(app.draft.starts_with("Keep this draft\n\n"));
        assert!(app.draft.contains("src/file.rs:4–8"));
        assert!(app.draft.contains("Selected text"));
        assert_eq!(app.state.messages.len(), messages);
        assert!(app.state.busy);
        let draft = app.draft.clone();
        app.collect_context_capture();
        assert_eq!(app.draft, draft, "Context is consumed exactly once");
        app.draft = "x".repeat(256 * 1024);
        queue(&ctx, "other", "text");
        app.collect_context_capture();
        assert_eq!(app.draft.len(), 256 * 1024);
        assert!(app.state.error.as_deref().unwrap().contains("too large"));
    }
}
