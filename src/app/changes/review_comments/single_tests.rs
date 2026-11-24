use super::ui_tests::{key, pointer, setup};
use super::*;
use crate::app::files::tests::frame;
fn seeded() -> (egui::Context, Pui, u64) {
    let (ctx, mut app) = setup();
    let document = app.changes.review.as_ref().unwrap();
    let (path, diff, digest) = (
        document.0.clone(),
        document.1.clone(),
        document.2.comment_digest(),
    );
    let start = diff[..diff.find("+chosen").unwrap()].chars().count();
    let notes = &mut app.changes.review_comments;
    for text in ["first committed note", "other note"] {
        assert!(notes.begin(&path, &diff, true, digest, start..start + 7));
        notes.editor.as_mut().unwrap().text = text.into();
        assert!(notes.save());
    }
    notes.expanded = true;
    notes.storage_dirty = false;
    let id = notes.items[0].id;
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app, id)
}
fn control(ctx: &egui::Context, id: u64) -> (egui::Id, egui::Rect) {
    ctx.data(|data| data.get_temp(egui::Id::new(("review-note-add-draft", id))))
        .unwrap()
}
#[test]
fn exact_single_note_has_original_status_and_saved_text_only() {
    let (_, mut app, id) = seeded();
    let notes = &mut app.changes.review_comments;
    notes.items[0].stale = true;
    notes.items[0].resolved = true;
    assert!(notes.begin_edit(id));
    notes.editor.as_mut().unwrap().text = "unfinished replacement".into();
    let text = notes.one_note(id).unwrap();
    assert!(text.contains("first committed note"));
    assert!(!text.contains("other note") && !text.contains("unfinished replacement"));
    assert!(text.contains("STALE") && text.contains("RESOLVED") && text.contains("SHA-256:"));
    assert!(!notes.storage_dirty);
    assert!(notes.one_note(u64::MAX).is_err());
    notes.bind(Path::new("/foreign"), "foreign", 1);
    assert!(notes.one_note(id).is_err());
}
#[test]
fn pointer_single_note_appends_once_without_rpc_or_note_mutation() {
    let (ctx, mut app, id) = seeded();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let response = ctx.read_response(control(&ctx, id).0).unwrap();
    let pos = response.interact_rect.center();
    for pressed in [true, false] {
        let mut events = pointer(pos, pressed);
        events.extend([
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
        ]);
        frame(&ctx, &mut app, events);
    }
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    assert_eq!(
        app.outgoing_text().matches("first committed note").count(),
        1
    );
    assert!(!app.outgoing_text().contains("other note"));
    assert!(app.outgoing_text().starts_with("Kept draft"));
    assert!(commands.try_recv().is_err());
    assert!(app.state.busy);
    assert_eq!(app.changes.review_comments.items.len(), 2);
    assert!(!app.changes.review_comments.storage_dirty);
}
#[test]
fn native_single_note_activation_with_new_question_and_stale_deleted_control_is_safe() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app, id) = seeded();
        let target = control(&ctx, id).0;
        ctx.memory_mut(|memory| memory.request_focus(target));
        app.state.dialogs.push_back(questions::demo_dialog());
        let mut events = vec![key(activation, egui::Modifiers::NONE)];
        if activation == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        frame(&ctx, &mut app, events);
        frame(&ctx, &mut app, vec![]);
        assert!(app.outgoing_text().contains("first committed note"));
        assert_eq!(app.state.dialogs.len(), 1);
        let draft = app.outgoing_text().clone();
        app.changes.review_comments.items.remove(0);
        ctx.memory_mut(|memory| memory.request_focus(target));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert_eq!(app.outgoing_text(), draft);
    }
}
#[test]
fn native_tab_from_resolve_then_reverse_escape_and_foreign_modal_preserve_draft() {
    let (ctx, mut app, id) = seeded();
    let target = control(&ctx, id).0;
    let resolve_rect = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("review-comment-resolve", 0usize))))
        .unwrap();
    let resolve = app
        .changes
        .review_comments
        .controls
        .iter()
        .find(|control| {
            ctx.read_response(**control)
                .is_some_and(|response| response.rect == resolve_rect)
        })
        .copied()
        .unwrap();
    ctx.memory_mut(|memory| memory.request_focus(resolve));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
    );
    assert!(ctx.memory(|memory| memory.has_focus(target)));
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
    assert_eq!(app.outgoing_text(), "Kept draft");
    app.changes.review_comments.expanded = true;
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let target = control(&ctx, id).0;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        prompt_ui::modal(ctx, egui::Id::new("foreign-single-note"), 220.0, |ui| {
            prompt_ui::heading(ui, "Information", "Foreign")
        });
    });
    ctx.memory_mut(|memory| memory.request_focus(target));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(app.outgoing_text(), "Kept draft");
    assert!(!app.changes.review_comments.storage_dirty);
}
#[test]
fn single_note_formatting_stays_bounded_and_does_not_mutate_payload() {
    let (_, mut app, id) = seeded();
    let notes = &mut app.changes.review_comments;
    notes.items[0].text = "界".repeat(682);
    let bytes = notes.bytes();
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        let text = notes.one_note(id).unwrap();
        assert!(text.len() < MAX_BATCH);
    }
    assert_eq!(notes.bytes(), bytes);
    eprintln!(
        "1000single-noteformats {:?}; payload{}B",
        start.elapsed(),
        bytes
    );
}
