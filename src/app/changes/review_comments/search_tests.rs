use super::ui_tests::{key, setup};
use super::*;
use crate::app::files::tests::frame;
fn seeded() -> (egui::Context, Pui) {
    let (ctx, mut app) = setup();
    let document = app.changes.review.as_ref().unwrap();
    let (path, diff, digest) = (
        document.0.clone(),
        document.1.clone(),
        document.2.comment_digest(),
    );
    let start = diff[..diff.find("+chosen").unwrap()].chars().count();
    let notes = &mut app.changes.review_comments;
    for text in ["needle 世界", "other committed"] {
        assert!(notes.begin(&path, &diff, true, digest, start..start + 7));
        notes.editor.as_mut().unwrap().text = text.into();
        assert!(notes.save());
    }
    notes.expanded = true;
    notes.storage_dirty = false;
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app)
}
fn search(ctx: &egui::Context) -> egui::Id {
    ctx.data(|data| data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("review-note-search")))
        .unwrap()
        .0
}
#[test]
fn actual_note_search_text_paste_never_reaches_draft_approval_or_rpc() {
    let (ctx, mut app) = seeded();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let id = search(&ctx);
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::Text("needle".into()),
            egui::Event::Paste(" 世界".into()),
        ],
    );
    assert_eq!(app.changes.review_comments.query, "needle 世界");
    assert_eq!(app.draft, "Kept draft");
    assert!(!app.changes.review_comments.storage_dirty);
    app.state.dialogs.push_back(questions::demo_dialog());
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
        ],
    );
    assert!(commands.try_recv().is_err());
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.state.busy);
}
#[test]
fn hidden_old_note_action_never_retargets_and_filter_does_not_change_batch_or_editor() {
    let (ctx, mut app) = seeded();
    let note = app.changes.review_comments.items[0].id;
    let old = ctx
        .data(|data| {
            data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new(("review-note-add-draft", note)))
        })
        .unwrap()
        .0;
    let notes = &mut app.changes.review_comments;
    let batch = notes.batch().unwrap();
    assert!(notes.begin_edit(note));
    notes.editor.as_mut().unwrap().text = "unfinished".into();
    notes.query = "other committed".into();
    ctx.memory_mut(|memory| memory.request_focus(old));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(app.draft, "Kept draft");
    let notes = &app.changes.review_comments;
    assert_eq!(notes.batch().unwrap(), batch);
    assert_eq!(notes.editor.as_ref().unwrap().text, "unfinished");
    assert_eq!(notes.items.len(), 2);
}
#[test]
fn marker_reveal_clears_search_and_owner_change_discards_query_without_dirty_checkpoint() {
    let (ctx, mut app) = seeded();
    let notes = &mut app.changes.review_comments;
    notes.query = "no match".into();
    let id = notes.items[0].id;
    notes.reveal_note = Some((
        id,
        app.changes.review.as_ref().unwrap().2.focus_id(),
        ctx.cumulative_pass_nr(),
    ));
    frame(&ctx, &mut app, vec![]);
    assert!(app.changes.review_comments.query.is_empty());
    let notes = &mut app.changes.review_comments;
    notes.query = "needle".into();
    notes.bind(Path::new("/other"), "other", 1);
    assert!(notes.query.is_empty());
    assert!(!notes.storage_dirty);
}
#[test]
fn text_path_case_sensitive_matching_and_native_escape_foreign_modal_are_safe() {
    let (ctx, mut app) = seeded();
    let visible = |output: &egui::FullOutput, needle: &str| {
        output.shapes.iter().any(
            |shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text==needle),
        )
    };
    app.changes.review_comments.query = "needle".into();
    let output = frame(&ctx, &mut app, vec![]);
    assert!(visible(&output, "needle 世界"));
    assert!(!visible(&output, "other committed"));
    app.changes.review_comments.query = "NEEDLE".into();
    let output = frame(&ctx, &mut app, vec![]);
    assert!(!visible(&output, "needle 世界"));
    app.changes.review_comments.query = "src/local-review.rs".into();
    let output = frame(&ctx, &mut app, vec![]);
    assert!(visible(&output, "needle 世界"));
    assert!(visible(&output, "other committed"));
    let id = search(&ctx);
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)],
    );
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert_eq!(app.draft, "Kept draft");
    app.changes.review_comments.expanded = true;
    frame(&ctx, &mut app, vec![]);
    let id = search(&ctx);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        prompt_ui::modal(ctx, egui::Id::new("foreign-note-search"), 220.0, |ui| {
            prompt_ui::heading(ui, "Information", "Foreign")
        });
    });
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("must not append".into())],
    );
    assert_eq!(app.changes.review_comments.query, "src/local-review.rs");
}
#[test]
fn bounded_unicode_search_and_settled_frames_keep_note_payload() {
    let (ctx, mut app) = seeded();
    let id = search(&ctx);
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(&ctx, &mut app, vec![egui::Event::Paste("界".repeat(300))]);
    assert_eq!(app.changes.review_comments.query.chars().count(), 128);
    assert_eq!(app.changes.review_comments.query.capacity(), 384);
    let bytes = app.changes.review_comments.bytes();
    let start = std::time::Instant::now();
    for _ in 0..30 {
        let output = frame(&ctx, &mut app, vec![]);
        assert!(output.platform_output.commands.is_empty());
    }
    assert_eq!(app.changes.review_comments.bytes(), bytes);
    assert!(!app.changes.review_comments.storage_dirty);
    eprintln!(
        "30notefilterframes {:?}; payload{}B query384B",
        start.elapsed(),
        bytes
    );
}
