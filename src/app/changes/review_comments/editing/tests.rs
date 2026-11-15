use super::*;
use crate::app::changes::review_comments::ui_tests::{click, key, pointer, select, setup};
use crate::app::files::tests::frame;
const DIFF: &str = "@@ -1 +1 @@\n-old\n+new\n";
fn add(notes: &mut Comments, text: &str) {
    assert!(notes.begin(
        "original.rs",
        DIFF,
        true,
        Some([7; 32]),
        crate::app::changes::review_comments::tests::range(DIFF, "+new")
    ));
    notes.editor.as_mut().unwrap().text = text.into();
    assert!(notes.save());
}
fn notes() -> Comments {
    let mut notes = Comments::default();
    notes.bind(Path::new("/repo"), "owner", 1);
    notes
}
#[test]
fn save_updates_only_text_same_id_anchor_resolved_stale_and_batch_does_not_see_draft() {
    let mut notes = notes();
    add(&mut notes, "saved text");
    add(&mut notes, "other note");
    let id = notes.items[0].id;
    let anchor = notes.items[0].anchor.clone();
    notes.items[0].resolved = true;
    notes.items[0].stale = true;
    assert!(notes.begin_edit(id));
    notes.editor.as_mut().unwrap().text = "changed 世界".into();
    assert_eq!(notes.items[0].text, "saved text");
    assert!(!notes.batch().unwrap().contains("changed 世界"));
    assert!(notes.save());
    let item = &notes.items[0];
    assert_eq!(item.id, id);
    assert_eq!(item.text, "changed 世界");
    assert!(item.resolved && item.stale);
    assert_eq!(item.anchor.digest, anchor.digest);
    assert_eq!(item.anchor.selection, anchor.selection);
    assert_eq!(item.anchor.owner, anchor.owner);
    assert_eq!(item.anchor.path, anchor.path);
    assert_eq!(notes.items[1].text, "other note");
    assert!(notes.begin_edit(id));
    notes.editor.as_mut().unwrap().text = "cancelled".into();
    notes.editor = None;
    assert_eq!(notes.items[0].text, "changed 世界");
}
#[test]
fn deleted_id_foreign_owner_index_shift_and_pending_editor_never_retarget() {
    let mut notes = notes();
    add(&mut notes, "first");
    add(&mut notes, "target");
    let id = notes.items[1].id;
    assert!(notes.begin_edit(id));
    notes.editor.as_mut().unwrap().text = "updated".into();
    notes.items.remove(0);
    assert!(notes.save());
    assert_eq!(notes.items[0].text, "updated");
    assert!(notes.begin_edit(id));
    notes.editor.as_mut().unwrap().text = "pending".into();
    assert!(!notes.begin_edit(id));
    notes.bind(Path::new("/repo"), "other", 1);
    assert!(!notes.save());
    assert_eq!(notes.items[0].text, "updated");
    assert_eq!(notes.editor.as_ref().unwrap().text, "pending");
    notes.bind(Path::new("/repo"), "owner", 1);
    notes.items.clear();
    assert!(!notes.save());
    assert!(notes.items.is_empty());
    assert!(notes.editor.is_some());
}
#[test]
fn editing_at_count_cap_preserves_original_and_payload_cap_covers_pending_copy() {
    let mut notes = notes();
    for _ in 0..MAX_COMMENTS {
        add(&mut notes, &"界".repeat(MAX_TEXT / 3));
    }
    let id = notes.items[0].id;
    let old = notes.bytes();
    assert!(notes.begin_edit(id));
    assert!(notes.bytes() > old);
    assert!(notes.bytes() <= MAX_TOTAL);
    notes.editor.as_mut().unwrap().text = "short".into();
    assert!(notes.save());
    assert_eq!(notes.items.len(), MAX_COMMENTS);
    let id = notes.items[0].id;
    assert!(notes.begin_edit(id));
    for invalid in [" ".into(), "x".repeat(MAX_TEXT + 1)] {
        notes.editor.as_mut().unwrap().text = invalid;
        assert!(!notes.save());
        assert_eq!(notes.items[0].text, "short");
    }
    notes.editor = None;
    let mut full = Comments::default();
    full.bind(
        Path::new(&format!("/{}", "p".repeat(2500))),
        &"s".repeat(1000),
        1,
    );
    loop {
        if !full.begin(
            "original.rs",
            DIFF,
            true,
            Some([7; 32]),
            crate::app::changes::review_comments::tests::range(DIFF, "+new"),
        ) {
            break;
        }
        full.editor.as_mut().unwrap().text = "x".repeat(MAX_TEXT);
        assert!(full.save());
    }
    let before = full.bytes();
    let id = full.items[0].id;
    assert!(!full.begin_edit(id));
    assert!(full.editor.is_none());
    assert_eq!(full.bytes(), before);
}
fn seeded() -> (egui::Context, Pui) {
    let (ctx, mut app) = setup();
    select(&ctx, &mut app, "chosen");
    click(&ctx, &mut app, "review-comment-add");
    frame(&ctx, &mut app, vec![egui::Event::Text("saved note".into())]);
    click(&ctx, &mut app, "review-comment-save");
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app)
}
fn edit(ctx: &egui::Context, app: &mut Pui) {
    // Cancel changes the preceding form's height; use the newly visible action.
    for _ in 0..3 {
        frame(ctx, app, vec![]);
    }
    let pos = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("review-comment-edit", 0usize))))
        .unwrap()
        .center();
    for pressed in [true, false] {
        frame(ctx, app, pointer(pos, pressed));
    }
    frame(ctx, app, vec![]);
}
#[test]
fn actual_edit_cancel_then_save_preserves_permission_run_draft_and_exact_anchor() {
    let (ctx, mut app) = seeded();
    let id = app.changes.review_comments.items[0].id;
    let anchor = app.changes.review_comments.items[0].anchor.clone();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.dialogs.push_back(json!({"id":"edit-approval","method":"confirm","title":"Allow bash?","message":"Controlled permission fixture"}));
    edit(&ctx, &mut app);
    assert_eq!(
        app.changes.review_comments.editor.as_ref().unwrap().editing,
        Some(id)
    );
    frame(&ctx, &mut app, vec![egui::Event::Text(" pending".into())]);
    click(&ctx, &mut app, "review-comment-cancel");
    assert_eq!(app.changes.review_comments.items[0].text, "saved note");
    edit(&ctx, &mut app);
    frame(&ctx, &mut app, vec![egui::Event::Text(" updated".into())]);
    let pending = app
        .changes
        .review_comments
        .editor
        .as_ref()
        .unwrap()
        .text
        .clone();
    click(&ctx, &mut app, "review-comment-save");
    assert_eq!(app.changes.review_comments.items.len(), 1);
    assert_eq!(app.changes.review_comments.items[0].id, id);
    assert_eq!(app.changes.review_comments.items[0].text, pending);
    assert_eq!(
        app.changes.review_comments.items[0].anchor.selection,
        anchor.selection
    );
    assert_eq!(
        app.changes.review_comments.items[0].anchor.digest,
        anchor.digest
    );
    assert_eq!(app.draft, "Kept draft");
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
}
#[test]
fn edit_pointer_press_release_modified_chords_and_arriving_question_cannot_send_answer_or_save() {
    for phase in [true, false] {
        let (ctx, mut app) = seeded();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let mut question = questions::demo_dialog();
        let mut value: Value =
            serde_json::from_str(question["placeholder"].as_str().unwrap()).unwrap();
        value["questions"] = json!([value["questions"][0].clone()]);
        question["placeholder"] = json!(value.to_string());
        app.state.dialogs.push_back(question);
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
        let pos = ctx
            .data(|data| {
                data.get_temp::<egui::Rect>(egui::Id::new(("review-comment-edit", 0usize)))
            })
            .unwrap()
            .center();
        for pressed in [true, false] {
            let mut events = pointer(pos, pressed);
            if pressed == phase {
                events.extend([
                    key(egui::Key::Enter, egui::Modifiers::COMMAND),
                    key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                    key(egui::Key::Space, egui::Modifiers::NONE),
                    egui::Event::Text(" ".into()),
                ]);
            }
            frame(&ctx, &mut app, events);
        }
        assert!(app.changes.review_comments.editor.is_some());
        assert_eq!(app.changes.review_comments.items[0].text, "saved note");
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(app.pending_dialog.is_none());
        assert_eq!(app.draft, "Kept draft");
        assert!(app.state.busy);
        assert!(commands.try_recv().is_err());
    }
}
#[test]
fn save_pointer_plain_and_modified_activation_cannot_reach_old_composer_or_permission() {
    for phase in [true, false] {
        let (ctx, mut app) = seeded();
        edit(&ctx, &mut app);
        frame(&ctx, &mut app, vec![egui::Event::Text(" changed".into())]);
        let pending = app
            .changes
            .review_comments
            .editor
            .as_ref()
            .unwrap()
            .text
            .clone();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.state.dialogs.push_back(json!({"id":"edit-save-permission","method":"confirm","title":"Allow bash?","message":"controlled"}));
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let pos = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("review-comment-save")))
            .unwrap()
            .center();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
        for pressed in [true, false] {
            let mut events = pointer(pos, pressed);
            if pressed == phase {
                events.extend([
                    key(egui::Key::Enter, egui::Modifiers::COMMAND),
                    key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                    key(egui::Key::Enter, egui::Modifiers::NONE),
                    key(egui::Key::Space, egui::Modifiers::NONE),
                    egui::Event::Text(" ".into()),
                ]);
            }
            frame(&ctx, &mut app, events);
        }
        assert!(app.changes.review_comments.editor.is_none());
        assert_eq!(app.changes.review_comments.items[0].text, pending);
        assert_eq!(app.draft, "Kept draft");
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(app.pending_dialog.is_none());
        assert!(app.state.busy);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn pending_edit_parks_without_mutating_note_and_stales_without_reattaching() {
    let (ctx, mut app) = seeded();
    edit(&ctx, &mut app);
    let id = app.changes.review_comments.items[0].id;
    frame(&ctx, &mut app, vec![egui::Event::Text(" parked".into())]);
    let pending = app
        .changes
        .review_comments
        .editor
        .as_ref()
        .unwrap()
        .text
        .clone();
    let root = app.prefs.cwd.clone();
    let owner = app.review_comment_owner();
    app.leave_session_layout();
    app.changes.review_comments.bind(&root, "foreign", 2);
    assert!(!app.changes.review_comments.save());
    assert_eq!(app.changes.review_comments.items[0].text, "saved note");
    assert_eq!(
        app.changes.review_comments.editor.as_ref().unwrap().text,
        pending
    );
    app.enter_session_layout();
    app.changes.review_comments.bind(&root, &owner, 2);
    assert_eq!(
        app.changes.review_comments.editor.as_ref().unwrap().editing,
        Some(id)
    );
    assert!(app.changes.review_comments.save());
    assert_eq!(app.changes.review_comments.items[0].text, pending);
    assert!(app.changes.review_comments.items[0].stale);
}

#[test]
fn edit_form_narrow_themes_zoom_and_new_question_typing_keep_source_note_unchanged() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app) = seeded();
            edit(&ctx, &mut app);
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            let (client, _, commands) = rpc::Client::stub_with_records();
            app.client = Some(client);
            let mut question = questions::demo_dialog();
            question["placeholder"]=json!(json!({"version":1,"request_id":"edit-text","questions":[{"id":"free","question":"Explain?","options":[]}]}).to_string());
            app.state.dialogs.push_back(question);
            let mut native = eframe::Frame::_new_kittest();
            for pass in 0..5 {
                let events = if pass == 4 {
                    vec![
                        egui::Event::Text(" 世界 edit 1".into()),
                        key(egui::Key::Num1, egui::Modifiers::NONE),
                    ]
                } else {
                    vec![]
                };
                let out = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 780.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut native),
                );
                assert!(out.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="Edit local comment")));
            }
            assert_eq!(app.changes.review_comments.items[0].text, "saved note");
            assert!(
                app.changes
                    .review_comments
                    .editor
                    .as_ref()
                    .unwrap()
                    .text
                    .contains("世界 edit 1")
            );
            assert_eq!(app.state.dialogs.len(), 1);
            assert!(app.pending_dialog.is_none());
            assert!(commands.try_recv().is_err());
            assert_eq!(app.draft, "Kept draft");
        }
    }
}

#[test]
fn native_edit_activation_foreign_popup_and_disabled_action_preserve_saved_text() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app) = seeded();
        let rect = ctx
            .data(|data| {
                data.get_temp::<egui::Rect>(egui::Id::new(("review-comment-edit", 0usize)))
            })
            .unwrap();
        let id = app
            .changes
            .review_comments
            .controls
            .iter()
            .copied()
            .find(|id| {
                ctx.read_response(*id)
                    .is_some_and(|response| response.rect == rect)
            })
            .unwrap();
        ctx.memory_mut(|memory| memory.request_focus(id));
        let mut events = vec![key(activation, egui::Modifiers::NONE)];
        if activation == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        frame(&ctx, &mut app, events);
        assert_eq!(
            app.changes.review_comments.editor.as_ref().unwrap().editing,
            Some(app.changes.review_comments.items[0].id)
        );
        assert_eq!(app.changes.review_comments.items[0].text, "saved note");
    }
    let (ctx, mut app) = seeded();
    let rect = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("review-comment-edit", 0usize))))
        .unwrap();
    let id = app
        .changes
        .review_comments
        .controls
        .iter()
        .copied()
        .find(|id| {
            ctx.read_response(*id)
                .is_some_and(|response| response.rect == rect)
        })
        .unwrap();
    egui::Popup::open_id(&ctx, egui::Id::new("foreign-edit-popup"));
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(app.changes.review_comments.editor.is_none());
    assert_eq!(app.changes.review_comments.items[0].text, "saved note");
    egui::Popup::close_all(&ctx);
    edit(&ctx, &mut app);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let rect = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("review-comment-edit", 0usize))))
        .unwrap();
    let id = app
        .changes
        .review_comments
        .controls
        .iter()
        .copied()
        .find(|id| {
            ctx.read_response(*id)
                .is_some_and(|response| response.rect == rect)
        })
        .unwrap();
    assert!(!ctx.read_response(id).unwrap().enabled());
    let editing = app.changes.review_comments.editor.as_ref().unwrap().editing;
    for pressed in [true, false] {
        frame(&ctx, &mut app, pointer(rect.center(), pressed));
    }
    assert_eq!(
        app.changes.review_comments.editor.as_ref().unwrap().editing,
        editing
    );
}

#[test]
fn editing_payload_and_settled_frame_add_no_idle_work() {
    let mut notes = notes();
    for _ in 0..MAX_COMMENTS {
        add(&mut notes, &"界".repeat(MAX_TEXT / 3));
    }
    let before = notes.bytes();
    let ids = notes.items.iter().map(|item| item.id).collect::<Vec<_>>();
    let start = std::time::Instant::now();
    for id in ids {
        assert!(notes.begin_edit(id));
        notes.editor.as_mut().unwrap().text.push('x');
        assert!(notes.save());
    }
    assert_eq!(notes.items.len(), MAX_COMMENTS);
    assert!(notes.bytes() <= MAX_TOTAL);
    eprintln!(
        "64local-note edits {:?}; retained payload before{before}B/after{}B; pendingclone budget enforced",
        start.elapsed(),
        notes.bytes()
    );
    let (ctx, mut app) = seeded();
    edit(&ctx, &mut app);
    app.state.busy = false;
    let mut time = 0.0;
    let mut draw = |events| {
        time += 0.1;
        let mut native = eframe::Frame::_new_kittest();
        ctx.run(
            egui::RawInput {
                time: Some(time),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1134.0, 900.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut native),
        )
    };
    draw(vec![egui::Event::PointerGone]);
    for _ in 0..20 {
        draw(vec![]);
    }
    let out = draw(vec![]);
    assert!(
        out.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_millis(100)
    );
}

#[test]
fn edit_actual_native_tab_reverse_escape_cancels_without_changing_saved_note() {
    for shift in [false, true] {
        let (ctx, mut app) = seeded();
        edit(&ctx, &mut app);
        frame(&ctx, &mut app, vec![egui::Event::Text(" unsaved".into())]);
        frame(
            &ctx,
            &mut app,
            vec![key(
                egui::Key::Tab,
                if shift {
                    egui::Modifiers::SHIFT
                } else {
                    egui::Modifiers::NONE
                },
            )],
        );
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(
            app.changes.review_comments.editor.is_none(),
            "shift={shift}"
        );
        assert_eq!(app.changes.review_comments.items[0].text, "saved note");
        assert!(app.state.busy);
        assert_eq!(app.draft, "Kept draft");
    }
}
