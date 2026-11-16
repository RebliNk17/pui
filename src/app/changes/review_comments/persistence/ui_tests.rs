use super::tests::{Storage, notes, restore};
use super::*;
use crate::app::files::tests::frame;
const DIFF: &str = "@@ -1 +1 @@\n-old\n+new 世界\n";
fn seeded(ctx: &egui::Context) -> Pui {
    let mut app = crate::app::files::tests::app(ctx);
    let session = app.prefs.cwd.join("persisted-owner.jsonl");
    app.state.session_file = Some(session.display().to_string());
    let owner = app.review_comment_owner();
    app.open_diff_review(("file.rs".into(), DIFF.into()).into(), true);
    app.changes.set_expanded(true);
    app.inspector_panel = true;
    let comments = &mut app.changes.review_comments;
    comments.bind(&app.prefs.cwd, &owner, app.output_revision);
    comments.source_root = Some(app.prefs.cwd.clone());
    let digest = app.changes.review.as_ref().unwrap().2.comment_digest();
    assert!(comments.begin(
        "file.rs",
        DIFF,
        true,
        digest,
        crate::app::changes::review_comments::tests::range(DIFF, "+new 世界")
    ));
    comments.editor.as_mut().unwrap().text = "Checkpointed note 世界".into();
    assert!(comments.save());
    app
}
#[test]
fn actual_app_save_and_reopen_restore_no_review_editor_open_request_or_execution() {
    let ctx = egui::Context::default();
    let mut app = seeded(&ctx);
    let root = app.prefs.cwd.clone();
    let mut storage = Storage::default();
    app.demo = false;
    eframe::App::save(&mut app, &mut storage);
    app.demo = true;
    assert!(storage.0.contains_key(KEY));
    // Use normal stored preferences without launching real providers: demo skips
    // startup/session discovery, then exercise the exact dedicated restore hook.
    let ctx = egui::Context::default();
    let mut cc = eframe::CreationContext::_new_kittest(ctx.clone());
    cc.storage = Some(&storage);
    let mut restored = Pui::new(
        &cc,
        Options {
            demo: true,
            cwd: Some(root),
            ..Default::default()
        },
    );
    restored
        .changes
        .review_comments
        .restore_checkpoint(Some(&storage));
    assert_eq!(restored.changes.review_comments.items.len(), 1);
    assert!(restored.changes.review_comments.items[0].stale);
    assert!(restored.changes.review.is_none());
    assert!(restored.changes.review_comments.editor.is_none());
    assert!(!restored.changes.review_comments.expanded);
    assert!(restored.client.is_none());
    assert!(restored.workspace_rx.is_none() && restored.diff_rx.is_none());
    assert!(restored.state.dialogs.is_empty());
    assert!(restored.draft.is_empty());
    // Inactive windows/restored read-only snapshots do not periodically replace
    // another window's checkpoint when no local saved-note mutation occurred.
    let before = storage.0[KEY].clone();
    restored
        .changes
        .review_comments
        .save_checkpoint(&mut storage);
    assert_eq!(storage.0[KEY], before);
}
#[test]
fn restored_ui_light_dark_zoom_has_stale_note_disabled_jump_no_marker_and_explicit_batch_only() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            let mut app = seeded(&ctx);
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            let mut storage = Storage::default();
            app.changes.review_comments.save_checkpoint(&mut storage);
            app.changes.review_comments = restore(&storage);
            app.draft = "Unsent stays".into();
            app.state.busy = true;
            let (client, _, commands) = rpc::Client::stub_with_records();
            app.client = Some(client);
            let owner = app.review_comment_owner();
            app.changes
                .review_comments
                .bind(&app.prefs.cwd, &owner, app.output_revision);
            app.changes.review_comments.expanded = true;
            let mut found = false;
            for _ in 0..5 {
                let output = frame(&ctx, &mut app, vec![]);
                for shape in output.shapes {
                    if let egui::Shape::Text(text) = shape.shape {
                        found |= text.galley.job.text.contains("Checkpointed note");
                    }
                }
            }
            let notes = &app.changes.review_comments;
            assert!(found);
            let id = notes.items[0].id;
            let document = app.changes.review.as_ref().unwrap();
            assert!(!notes.jump_available(id, "file.rs", true, &document.2));
            assert!(notes.marker_controls.is_empty());
            assert!(notes.items[0].stale);
            assert_eq!(app.draft, "Unsent stays");
            assert!(commands.try_recv().is_err());
            let (jump, _) = ctx
                .data(|data| {
                    data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new((
                        "review-comment-jump",
                        id,
                    )))
                })
                .unwrap();
            assert!(!ctx.read_response(jump).unwrap().enabled());
            let batch = notes.batch().unwrap();
            assert!(batch.contains("STALE") && batch.contains("Checkpointed note"));
            assert_eq!(app.draft, "Unsent stays");
            assert!(app.state.busy);
            let resolve = ctx
                .data(|data| {
                    data.get_temp::<egui::Rect>(egui::Id::new(("review-comment-resolve", 0usize)))
                })
                .unwrap();
            crate::app::files::tests::click(&ctx, &mut app, resolve.center());
            assert!(app.changes.review_comments.items[0].resolved);
            assert!(app.changes.review_comments.storage_dirty);
            app.changes.review_comments.save_checkpoint(&mut storage);
            assert!(restore(&storage).items[0].resolved);
            assert!(commands.try_recv().is_err());
            app.state.busy = false;
            app.client = None;
            let mut output = None;
            for _ in 0..80 {
                output = Some(frame(&ctx, &mut app, vec![]));
            }
            assert!(!app.changes.review_comments.storage_dirty);
            assert!(app.workspace_rx.is_none() && app.diff_rx.is_none());
            assert!(
                !output.unwrap().viewport_output[&egui::ViewportId::ROOT]
                    .repaint_delay
                    .is_zero()
            );
        }
    }
}
#[test]
fn invalid_storage_notice_is_neutral_and_unchanged_normal_saves_preserve_corrupt_evidence() {
    let ctx = egui::Context::default();
    let mut app = seeded(&ctx);
    let mut storage = Storage::default();
    storage.0.insert(KEY.into(), "broken checkpoint".into());
    app.changes.review_comments = restore(&storage);
    let owner = app.review_comment_owner();
    app.changes
        .review_comments
        .bind(&app.prefs.cwd, &owner, app.output_revision);
    let mut visible = false;
    for _ in 0..4 {
        let output = frame(&ctx, &mut app, vec![]);
        for shape in output.shapes {
            if let egui::Shape::Text(text) = shape.shape {
                if text.galley.job.text == INVALID {
                    visible = true;
                    assert_ne!(
                        text.galley.job.sections[0].format.color,
                        theme::palette(&ctx).red
                    );
                }
            }
        }
    }
    assert!(visible);
    app.changes.review_comments.save_checkpoint(&mut storage);
    assert_eq!(storage.0[KEY], "broken checkpoint");
    assert!(app.changes.review_comments.items.is_empty());
}
#[test]
fn explicit_edit_delete_and_resolve_changes_update_checkpoint_not_stale_passive_observations() {
    let mut notes = notes();
    let mut storage = Storage::default();
    notes.save_checkpoint(&mut storage);
    assert!(!notes.storage_dirty);
    notes.stale(None);
    notes.save_checkpoint(&mut storage);
    assert!(!notes.storage_dirty);
    let id = notes.items[0].id;
    assert!(notes.begin_edit(id));
    notes.editor.as_mut().unwrap().text = "Saved replacement".into();
    assert!(notes.save());
    assert!(notes.storage_dirty);
    notes.save_checkpoint(&mut storage);
    assert_eq!(restore(&storage).items[0].text, "Saved replacement");
    assert!(notes.request_delete(id));
    assert!(notes.confirm_delete());
    assert!(notes.storage_dirty);
    notes.save_checkpoint(&mut storage);
    assert!(restore(&storage).items.is_empty());
}
