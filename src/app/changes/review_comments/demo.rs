//! Explicit, synthetic presentation fixture; uses the actual open recorded document.
use super::*;

impl Changes {
    pub(in crate::app) fn demo_review_comments(&mut self, revision: u64) {
        let Some(document) = self.review.as_ref().filter(|document| document.3) else {
            return;
        };
        // Derive a real display-character range from this exact document, not
        // invented coordinates or a second body. Only the first source row is used.
        let mut offset = 0usize;
        let mut selected = None;
        for row in lines::rows(&document.1) {
            let count = row.text.chars().count();
            if row.selectable() && !row.header && count > 1 {
                let start = offset + row.gutter().chars().count() + 1;
                selected = Some(start..start + count - 1);
                break;
            }
            offset += if row.header {
                lines::HUNK_CAPTION.chars().count()
            } else {
                row.gutter().chars().count() + count + 1
            };
        }
        let Some(range) = selected else {
            return;
        };
        let comments = &mut self.review_comments;
        comments.bind(&self.identity.0, &self.identity.1, revision);
        comments.source_root = Some(self.identity.0.clone());
        if comments.begin(
            &document.0,
            &document.1,
            true,
            document.2.comment_digest(),
            range.clone(),
        ) {
            comments.editor.as_mut().unwrap().text =
                "Synthetic demo note · original recorded snapshot; never sent automatically."
                    .into();
            comments.save();
        }
        if comments.begin(
            &document.0,
            &document.1,
            true,
            document.2.comment_digest(),
            range,
        ) {
            comments.editor.as_mut().unwrap().text =
                "Synthetic demo draft · can we make this case clearer?".into();
        }
        // begin/save are local state only: no context queue, provider or Git read.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::files::tests::frame;
    #[test]
    fn explicit_demo_seeds_real_recorded_review_and_inline_editor_without_provider_or_git_read() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                demo_review_comments: true,
                ..Default::default()
            },
        );
        let notes = &app.changes.review_comments;
        assert_eq!(notes.items.len(), 1);
        assert!(notes.expanded);
        assert!(
            notes
                .editor
                .as_ref()
                .unwrap()
                .text
                .starts_with("Synthetic demo draft")
        );
        let document = app.changes.review.as_ref().unwrap();
        assert!(document.3);
        assert!(
            app.outputs
                .iter()
                .any(|file| file.path == document.0 && file.diff == document.1)
        );
        assert_eq!(
            notes.items[0].anchor.digest,
            document.2.comment_digest().unwrap()
        );
        assert_eq!(
            notes.items[0].anchor.selection,
            anchor::selected(&document.1, notes.items[0].anchor.selection.chars.clone()).unwrap()
        );
        let draft = app.draft.clone();
        let mut visible_editor = false;
        let mut visible_demo_note = false;
        for _ in 0..4 {
            let output = frame(&ctx, &mut app, vec![]);
            for shape in output.shapes {
                if let egui::Shape::Text(text) = shape.shape {
                    visible_editor |= text.galley.job.text == "Local comment";
                    visible_demo_note |= text.galley.job.text.starts_with("Synthetic demo note");
                }
            }
        }
        assert!(visible_editor && visible_demo_note);
        assert!(app.client.is_none());
        assert!(app.workspace_rx.is_none() && app.diff_rx.is_none());
        assert_eq!(app.draft, draft, "The demo never batches or sends");
        assert!(!egui::Popup::is_any_open(&ctx));
        assert!(ctx.memory(|memory| memory.top_modal_layer().is_none()));
        assert_eq!(
            app.changes.review_comments.items.len(),
            1,
            "Seeded once, not every frame"
        );
    }
}
