//! Explicit context actions append bounded excerpts to the draft, never directly to Pi.
use super::*;
mod chips;
mod diff;
pub(super) use chips::Draft;
mod rendered;
mod source;
pub(super) use diff::{demo as demo_diff, offer as offer_diff_selection};
pub(super) use rendered::demo as demo_rendered;
pub(crate) use rendered::offer as offer_rendered_selection;
pub(in crate::app) use source::console_menu as offer_console_selection;
pub(super) use source::{demo as demo_source, offer as offer_source_selection};

pub(super) fn clear_source(ctx: &egui::Context) {
    source::clear(ctx);
    ctx.data_mut(|data| {
        data.remove::<Snippet>(egui::Id::new("pui-context-capture"));
        data.remove::<(PathBuf, String)>(egui::Id::new("pui-review-capture-owner"));
    });
}

pub(super) fn inputs(ctx: &egui::Context) {
    source::inputs(ctx);
}

#[derive(Clone, Default)]
struct Snippet {
    label: String,
    text: String,
}
const MAX_BYTES: usize = 24 * 1024;

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
    /// Runtime owner for local review: an unsaved chat is not the shared string "new".
    pub(super) fn review_comment_owner(&mut self) -> String {
        self.state
            .session_file
            .clone()
            .unwrap_or_else(|| format!("agent:{}", self.ensure_agent()))
    }
    pub(super) fn collect_context_capture(&mut self) {
        source::finish(&self.ctx);
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
