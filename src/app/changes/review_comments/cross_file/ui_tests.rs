use super::*;
use crate::app::changes::review_comments::ui_tests::{key, pointer};
use crate::app::files::tests::frame;
fn fixture() -> (egui::Context, Pui, u64) {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.prefs.hide_sidebar = true;
    app.focus_composer = false;
    app.draft = "Kept draft".into();
    app.state.busy = true;
    let target = "@@ -1 +1 @@\n-old\n+chosen 世界\n";
    let current = "@@ -1 +1 @@\n-before\n+after\n";
    let owner = app.review_comment_owner();
    app.outputs = vec![
        RecordedFile {
            path: "target.rs".into(),
            diff: target.into(),
            ..Default::default()
        },
        RecordedFile {
            path: "current.rs".into(),
            diff: current.into(),
            ..Default::default()
        },
    ];
    app.output_revision = app.state.outputs_revision;
    app.open_diff_review(("target.rs".into(), target.into()).into(), true);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let digest = app.changes.review.as_ref().unwrap().2.comment_digest();
    let notes = &mut app.changes.review_comments;
    assert!(notes.begin(
        "target.rs",
        target,
        true,
        digest,
        crate::app::changes::review_comments::tests::range(target, "+chosen 世界")
    ));
    notes.editor.as_mut().unwrap().text = "Open original target".into();
    assert!(notes.save());
    let id = notes.items[0].id;
    app.changes.open_review(
        &ctx,
        ("current.rs".into(), current.into()).into(),
        true,
        &app.prefs.cwd,
        &owner,
    );
    app.changes.set_expanded(true);
    app.changes.review_comments.expanded = true;
    for _ in 0..5 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(!app.changes.review_comments.items[0].stale);
    (ctx, app, id)
}
fn control(ctx: &egui::Context, note: u64) -> (egui::Id, egui::Rect) {
    ctx.data(|data| {
        data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new(("review-note-open-recorded", note)))
    })
    .unwrap()
}
#[test]
fn real_open_click_reveals_exact_other_recorded_file_preserving_editor_draft_and_no_rpc() {
    let (ctx, mut app, id) = fixture();
    assert!(app.changes.review_comments.begin_edit(id));
    app.changes.review_comments.editor.as_mut().unwrap().text = "Unfinished stays".into();
    app.changes.review_comments.editor.as_mut().unwrap().first = false;
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let pos = control(&ctx, id).1.center();
    for pressed in [true, false] {
        frame(&ctx, &mut app, pointer(pos, pressed));
    }
    assert_eq!(app.changes.review.as_ref().unwrap().0, "target.rs");
    assert_eq!(
        app.changes.review_comments.editor.as_ref().unwrap().text,
        "Unfinished stays"
    );
    assert_eq!(app.draft, "Kept draft");
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
    assert!(app.workspace_rx.is_none() && app.diff_rx.is_none());
    assert!(app.changes.expanded());
}
#[test]
fn stale_note_pointer_inspects_other_exact_file_without_marking_fresh_or_sending() {
    let (ctx, mut app, id) = fixture();
    app.changes.review_comments.items[0].stale = true;
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let pos = control(&ctx, id).1.center();
    for pressed in [true, false] {
        frame(&ctx, &mut app, pointer(pos, pressed));
    }
    assert_eq!(app.changes.review.as_ref().unwrap().0, "target.rs");
    assert!(app.changes.review_comments.items[0].stale);
    assert_eq!(app.draft, "Kept draft");
    assert!(commands.try_recv().is_err());
    assert!(app.workspace_rx.is_none() && app.diff_rx.is_none());
}
#[test]
fn actual_native_tab_activation_pointer_chords_and_new_question_keep_permission_scope() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app, id) = fixture();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let target = control(&ctx, id).0;
        for _ in 0..100 {
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
            );
            if ctx.memory(|memory| memory.has_focus(target)) {
                break;
            }
        }
        assert!(ctx.memory(|memory| memory.has_focus(target)));
        app.state.dialogs.push_back(questions::demo_dialog());
        let mut events = vec![key(activation, egui::Modifiers::NONE)];
        if activation == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        frame(&ctx, &mut app, events);
        assert_eq!(app.changes.review.as_ref().unwrap().0, "target.rs");
        assert_eq!(app.state.dialogs.len(), 1);
        assert_eq!(app.draft, "Kept draft");
        assert!(app.state.busy);
        assert!(commands.try_recv().is_err());
    }
    for phase in [true, false] {
        let (ctx, mut app, id) = fixture();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.state.dialogs.push_back(questions::demo_dialog());
        let pos = control(&ctx, id).1.center();
        for pressed in [true, false] {
            let mut events = pointer(pos, pressed);
            if pressed == phase {
                events.extend([
                    key(egui::Key::Enter, egui::Modifiers::COMMAND),
                    key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                    egui::Event::Text(" ".into()),
                    egui::Event::Paste("no answer".into()),
                ]);
            }
            frame(&ctx, &mut app, events);
        }
        assert_eq!(app.state.dialogs.len(), 1);
        assert_eq!(app.draft, "Kept draft");
        assert!(app.state.busy);
        assert!(commands.try_recv().is_err());
    }
}
#[test]
fn reverse_native_tab_escape_and_changed_document_cannot_replay_old_open_control() {
    let (ctx, mut app, id) = fixture();
    let (old, _) = control(&ctx, id);
    let resolve = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("review-comment-resolve", 0usize))))
        .unwrap();
    // Native traversal, not a guessed predecessor: Open follows the disabled range
    // and precedes Resolve for this exact note row.
    ctx.memory_mut(|memory| memory.request_focus(old));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
    );
    let focused = ctx.memory(|memory| memory.focused()).unwrap();
    assert_eq!(ctx.read_response(focused).unwrap().rect, resolve);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)],
    );
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Escape, egui::Modifiers::NONE),
            egui::Event::Text(" ".into()),
        ],
    );
    assert_eq!(app.changes.review.as_ref().unwrap().0, "current.rs");
    assert!(app.state.busy);
    assert_eq!(app.draft, "Kept draft");
    app.changes.review_comments.expanded = true;
    let owner = app.review_comment_owner();
    let source = "@@ -1 +1 @@\n-x\n+y\n";
    app.changes.open_review(
        &ctx,
        ("third.rs".into(), source.into()).into(),
        true,
        &app.prefs.cwd,
        &owner,
    );
    app.outputs.push(RecordedFile {
        path: "third.rs".into(),
        diff: source.into(),
        ..Default::default()
    });
    app.changes.review_comments.expanded = true;
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let (new, _) = control(&ctx, id);
    assert_ne!(old, new);
    ctx.memory_mut(|memory| memory.request_focus(old));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(app.changes.review.as_ref().unwrap().0, "third.rs");
}
#[test]
fn explicit_native_demo_is_provider_free_and_does_not_hash_copy_or_open_target_while_settled() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_review_comments: true,
            demo_review_note_files: true,
            demo_review_files: true,
            demo_changes: true,
            ..Default::default()
        },
    );
    let mut found = false;
    for _ in 0..5 {
        let output = frame(&ctx, &mut app, vec![]);
        found |= output.shapes.iter().any(
            |shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="Open"),
        );
    }
    assert!(found);
    assert_eq!(
        app.changes.review.as_ref().unwrap().0,
        "src/another-recorded-file.rs"
    );
    assert!(app.changes.review_comments.editor.is_none());
    let note = app.changes.review_comments.items[0].id;
    assert_eq!(app.changes.review_comments.recorded_targets.len(), 1);
    assert!(app.client.is_none());
    assert!(app.workspace_rx.is_none() && app.diff_rx.is_none());
    app.state.busy = false;
    for _ in 0..80 {
        let output = frame(&ctx, &mut app, vec![]);
        assert!(output.platform_output.commands.is_empty());
    }
    assert!(app.changes.review_comments.recorded_open.is_none());
    assert_eq!(app.changes.review_comments.items[0].id, note);
}
#[test]
fn light_dark_narrow_zoom_foreign_popup_stale_and_list_change_refuse_old_action() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app, id) = fixture();
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            let mut native = eframe::Frame::_new_kittest();
            for _ in 0..4 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(620.0, 760.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut native),
                );
            }
            let (control, rect) = control(&ctx, id);
            assert!(ctx.screen_rect().contains_rect(rect));
            egui::Popup::open_id(&ctx, egui::Id::new("foreign-note-file"));
            ctx.memory_mut(|memory| memory.request_focus(control));
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
            );
            assert_eq!(app.changes.review.as_ref().unwrap().0, "current.rs");
            egui::Popup::close_all(&ctx);
            app.changes.review_comments.stale(None);
            ctx.memory_mut(|memory| memory.request_focus(control));
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
            );
            assert_eq!(app.changes.review.as_ref().unwrap().0, "current.rs");
            assert_eq!(app.changes.review_comments.recorded_targets.len(), 1);
            assert!(app.changes.review_comments.items[0].stale);
        }
    }
    let (ctx, mut app, id) = fixture();
    let (old, _) = control(&ctx, id);
    app.state.outputs_revision = app.state.outputs_revision.wrapping_add(1);
    ctx.memory_mut(|memory| memory.request_focus(old));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(app.changes.review.as_ref().unwrap().0, "current.rs");
    assert!(app.changes.review_comments.recorded_targets.is_empty());
    assert!(app.changes.review_comments.items[0].stale);
}
