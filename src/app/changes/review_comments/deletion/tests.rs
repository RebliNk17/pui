use super::*;
use crate::app::changes::review_comments::ui_tests::{click, key, pointer, select, setup};
use crate::app::files::tests::frame;
const DIFF: &str = "@@ -1 +1 @@\n-old\n+new\n";
fn add(notes: &mut Comments, text: &str) {
    assert!(notes.begin(
        "file.rs",
        DIFF,
        true,
        Some([7; 32]),
        crate::app::changes::review_comments::tests::range(DIFF, "+new")
    ));
    notes.editor.as_mut().unwrap().text = text.into();
    assert!(notes.save());
}
fn saved() -> Comments {
    let mut notes = Comments::default();
    notes.bind(Path::new("/repo"), "owner", 1);
    notes
}
#[test]
fn exact_delete_survives_index_shift_preserves_editor_other_owners_and_frees_payload() {
    let mut notes = saved();
    add(&mut notes, "first");
    add(&mut notes, "target");
    let target = notes.items[1].id;
    notes.bind(Path::new("/repo"), "other", 1);
    add(&mut notes, "other owner");
    notes.bind(Path::new("/repo"), "owner", 1);
    assert!(notes.begin(
        "file.rs",
        DIFF,
        true,
        Some([7; 32]),
        crate::app::changes::review_comments::tests::range(DIFF, "+new")
    ));
    notes.editor.as_mut().unwrap().text = "unfinished stays".into();
    let bytes = notes.bytes();
    assert!(notes.request_delete(target));
    notes.items.remove(0);
    assert!(notes.confirm_delete());
    assert_eq!(notes.items.len(), 1);
    assert_eq!(notes.items[0].text, "other owner");
    assert_eq!(notes.editor.as_ref().unwrap().text, "unfinished stays");
    assert!(notes.bytes() < bytes);
    assert!(!notes.confirm_delete());
    assert!(notes.delete_requested.is_none());
}
#[test]
fn delete_never_retargets_disappeared_id_reused_position_or_foreign_owner() {
    let mut notes = saved();
    add(&mut notes, "old");
    let old = notes.items[0].id;
    assert!(notes.request_delete(old));
    notes.items.clear();
    add(&mut notes, "replacement");
    assert_ne!(old, notes.items[0].id);
    assert!(!notes.confirm_delete());
    assert_eq!(notes.items[0].text, "replacement");
    let own = notes.items[0].id;
    notes.bind(Path::new("/repo"), "foreign", 1);
    assert!(!notes.request_delete(own));
    notes.delete_requested = Some(own);
    assert!(!notes.confirm_delete());
    assert_eq!(notes.items.len(), 1);
    notes.bind(Path::new("/repo"), "owner", 1);
    assert!(notes.request_delete(own));
    notes.bind(Path::new("/changed-root"), "owner", 1);
    assert!(!notes.confirming());
    assert_eq!(notes.items.len(), 1);
}
#[test]
fn delete_refusal_and_identity_exhaustion_preserve_existing_notes() {
    let mut notes = saved();
    for _ in 0..MAX_COMMENTS {
        add(&mut notes, "bounded");
    }
    let target = notes.items[0].id;
    assert!(notes.request_delete(target));
    assert!(!notes.request_delete(notes.items[1].id));
    notes.cancel_confirmation();
    assert_eq!(notes.items.len(), MAX_COMMENTS);
    assert!(notes.request_delete(target));
    assert!(notes.confirm_delete());
    add(&mut notes, "replacement");
    assert_eq!(notes.items.len(), MAX_COMMENTS);
    assert!(notes.items.last().unwrap().id > target);
    let mut notes = saved();
    notes.next_id = u64::MAX;
    assert!(notes.begin(
        "file.rs",
        DIFF,
        true,
        Some([7; 32]),
        crate::app::changes::review_comments::tests::range(DIFF, "+new")
    ));
    notes.editor.as_mut().unwrap().text = "kept".into();
    assert!(!notes.save());
    assert!(notes.items.is_empty());
    assert_eq!(notes.editor.as_ref().unwrap().text, "kept");
}
fn seed_ui(ctx: &egui::Context, app: &mut Pui) {
    select(ctx, app, "chosen");
    click(ctx, app, "review-comment-add");
    frame(ctx, app, vec![egui::Event::Text("target note".into())]);
    click(ctx, app, "review-comment-save");
    frame(ctx, app, vec![]);
    let owner = app.changes.review_comments.owner.clone().unwrap();
    add(&mut app.changes.review_comments, "second saved note");
    app.changes
        .review_comments
        .bind(&owner.root, &owner.session, app.output_revision);
    frame(ctx, app, vec![]);
}
fn delete_click(ctx: &egui::Context, app: &mut Pui) {
    let pos = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("review-comment-delete", 0usize))))
        .unwrap()
        .center();
    for pressed in [true, false] {
        frame(ctx, app, pointer(pos, pressed));
    }
}
#[test]
fn ui_delete_cancel_safe_default_confirm_and_pending_approval_preserve_everything_else() {
    let (ctx, mut app) = setup();
    seed_ui(&ctx, &mut app);
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.dialogs.push_back(json!({"id":"delete-note-approval","method":"confirm","title":"Allow bash?","message":"Controlled permission fixture"}));
    let id = app.changes.review_comments.items[0].id;
    delete_click(&ctx, &mut app);
    assert_eq!(app.changes.review_comments.delete_requested, Some(id));
    assert_eq!(app.changes.review_comments.items.len(), 2);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!app.changes.review_comments.confirming());
    assert_eq!(app.changes.review_comments.items.len(), 2);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    delete_click(&ctx, &mut app);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    // Native first focus is Cancel, never the destructive action.
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(app.changes.review_comments.items.len(), 2);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    delete_click(&ctx, &mut app);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let out = frame(&ctx, &mut app, vec![]);
    let confirm = out
        .shapes
        .iter()
        .rev()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Delete note" => {
                Some(text.pos + text.galley.size() / 2.0)
            }
            _ => None,
        })
        .unwrap();
    for pressed in [true, false] {
        frame(&ctx, &mut app, pointer(confirm, pressed));
    }
    assert_eq!(app.changes.review_comments.items.len(), 1);
    assert_eq!(
        app.changes.review_comments.items[0].text,
        "second saved note"
    );
    assert!(app.changes.review.is_some());
    assert!(app.state.busy);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.pending_dialog.is_none());
    assert_eq!(app.draft, "Kept draft");
    assert!(commands.try_recv().is_err());
}
#[test]
fn delete_control_pointer_phases_chords_and_arriving_question_never_answer_or_delete() {
    for phase in [true, false] {
        for chord in [egui::Key::Enter, egui::Key::Backspace] {
            let (ctx, mut app) = setup();
            seed_ui(&ctx, &mut app);
            let (client, _, commands) = rpc::Client::stub_with_records();
            app.client = Some(client);
            let mut question = questions::demo_dialog();
            let mut payload: Value =
                serde_json::from_str(question["placeholder"].as_str().unwrap()).unwrap();
            payload["questions"] = json!([payload["questions"][0].clone()]);
            question["placeholder"] = json!(payload.to_string());
            app.state.dialogs.push_back(question);
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
            let pos = ctx
                .data(|data| {
                    data.get_temp::<egui::Rect>(egui::Id::new(("review-comment-delete", 0usize)))
                })
                .unwrap()
                .center();
            for pressed in [true, false] {
                let mut events = pointer(pos, pressed);
                if pressed == phase {
                    events.extend([
                        key(chord, egui::Modifiers::COMMAND),
                        key(egui::Key::Space, egui::Modifiers::NONE),
                        egui::Event::Text(" ".into()),
                    ]);
                }
                frame(&ctx, &mut app, events);
            }
            assert!(app.changes.review_comments.delete_requested.is_some());
            assert_eq!(app.changes.review_comments.items.len(), 2);
            assert_eq!(app.state.dialogs.len(), 1);
            assert!(app.pending_dialog.is_none());
            assert!(app.state.busy);
            assert_eq!(app.draft, "Kept draft");
            assert!(commands.try_recv().is_err());
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
            );
            assert!(!app.changes.review_comments.confirming());
            assert_eq!(app.state.dialogs.len(), 1);
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn delete_modal_theme_zoom_narrow_backdrop_and_disabled_control_preserve_notes() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app) = setup();
            seed_ui(&ctx, &mut app);
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            let id = app.changes.review_comments.items[0].id;
            assert!(app.changes.review_comments.request_delete(id));
            let mut draw = |events| {
                let mut native = eframe::Frame::_new_kittest();
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 780.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut native),
                )
            };
            for _ in 0..4 {
                draw(vec![]);
            }
            let output = draw(vec![]);
            let title = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text)
                        if text.galley.job.text == "Delete local review note?" =>
                    {
                        Some(text)
                    }
                    _ => None,
                })
                .unwrap();
            assert!(
                title.pos.x >= 0.0
                    && title.pos.x + title.galley.size().x <= ctx.screen_rect().right()
            );
            // The outside backdrop is always Cancel, never destructive consent.
            for pressed in [true, false] {
                draw(pointer(egui::pos2(2.0, 2.0), pressed));
            }
            assert!(!app.changes.review_comments.confirming());
            assert_eq!(app.changes.review_comments.items.len(), 2);
            assert!(app.changes.review_comments.request_delete(id));
            let document = app.changes.review.as_ref().unwrap();
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.disable();
                    app.changes.review_comments.show(
                        ui,
                        &document.0,
                        &document.1,
                        document.3,
                        &mut diff_viewer::Navigation::new(&document.1),
                    );
                });
            });
            assert_eq!(app.changes.review_comments.items.len(), 2);
        }
    }
}

#[test]
fn foreign_popup_or_modal_gets_first_refusal_and_modified_confirm_is_not_consent() {
    let (ctx, mut app) = setup();
    seed_ui(&ctx, &mut app);
    let id = app.changes.review_comments.items[0].id;
    assert!(app.changes.review_comments.request_delete(id));
    for popup in [true, false] {
        let _ = ctx.run(
            egui::RawInput {
                events: vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
                ..Default::default()
            },
            |ctx| {
                if popup {
                    egui::Popup::open_id(ctx, egui::Id::new("foreign-note-delete"));
                } else {
                    prompt_ui::modal(ctx, egui::Id::new("foreign-note-delete"), 220.0, |ui| {
                        prompt_ui::heading(ui, "Foreign form", "Owns this input")
                    });
                }
                let before = ctx.input(|input| input.events.clone());
                app.changes
                    .review_comments
                    .deletion_dialog(ctx, &app.changes.review.as_ref().unwrap().2);
                assert_eq!(ctx.input(|input| input.events.clone()), before);
                assert_eq!(app.changes.review_comments.items.len(), 2);
            },
        );
        egui::Popup::close_all(&ctx);
    }
    for _ in 0..5 {
        frame(&ctx, &mut app, vec![]);
    } // foreign modal is no longer rendered
    let output = frame(&ctx, &mut app, vec![]);
    let confirm = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Delete note" => {
                Some(text.pos + text.galley.size() / 2.0)
            }
            _ => None,
        })
        .unwrap();
    // Native Cancel→reverseTab moves to the actual destructive action.
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)],
    );
    frame(&ctx, &mut app, vec![]);
    let confirm_id = ctx.memory(|memory| memory.focused()).unwrap();
    assert!(
        ctx.read_response(confirm_id)
            .unwrap()
            .rect
            .contains(confirm)
    );
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
    );
    assert_eq!(app.changes.review_comments.items.len(), 2);
    assert!(app.changes.review_comments.confirming());
}

#[test]
fn native_delete_activation_and_confirmation_leave_unfinished_editor_and_resolved_notes_intact() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app) = setup();
        seed_ui(&ctx, &mut app);
        app.changes.review_comments.items[0].resolved = true;
        app.changes.review_comments.items[0].stale = true;
        select(&ctx, &mut app, "chosen");
        click(&ctx, &mut app, "review-comment-add");
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::Text("unfinished local text".into())],
        );
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let rect = ctx
            .data(|data| {
                data.get_temp::<egui::Rect>(egui::Id::new(("review-comment-delete", 0usize)))
            })
            .unwrap();
        let delete = app
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
        ctx.memory_mut(|memory| memory.request_focus(delete));
        let mut events = vec![key(activation, egui::Modifiers::NONE)];
        if activation == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        frame(&ctx, &mut app, events);
        assert!(app.changes.review_comments.confirming());
        assert_eq!(app.changes.review_comments.items.len(), 2);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let output = frame(&ctx, &mut app, vec![]);
        let confirm = output
            .shapes
            .iter()
            .rev()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Delete note" => {
                    Some(text.pos + text.galley.size() / 2.0)
                }
                _ => None,
            })
            .unwrap();
        for pressed in [true, false] {
            frame(&ctx, &mut app, pointer(confirm, pressed));
        }
        assert_eq!(app.changes.review_comments.items.len(), 1);
        assert_eq!(
            app.changes.review_comments.items[0].text,
            "second saved note"
        );
        assert_eq!(
            app.changes.review_comments.editor.as_ref().unwrap().text,
            "unfinished local text"
        );
        assert_eq!(app.draft, "Kept draft");
    }
}

#[test]
fn deletion_resource_cost_and_settled_surface_are_bounded() {
    let mut notes = saved();
    for _ in 0..MAX_COMMENTS {
        add(&mut notes, &"界".repeat(MAX_TEXT / 3));
    }
    let start = std::time::Instant::now();
    let bytes = notes.bytes();
    let count = notes.items.len();
    while let Some(id) = notes.items.first().map(|item| item.id) {
        assert!(notes.request_delete(id));
        assert!(notes.confirm_delete());
    }
    assert_eq!(notes.bytes(), 0);
    assert!(notes.editor.is_none());
    eprintln!(
        "deleted {count} local notes / {bytes} retained payload bytes in {:?}; pending request is one u64 ID",
        start.elapsed()
    );
    let (ctx, mut app) = setup();
    seed_ui(&ctx, &mut app);
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
    for _ in 0..25 {
        draw(vec![]);
    }
    let output = draw(vec![]);
    assert!(output.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(1));
}

#[test]
fn pending_delete_cancels_on_hidden_review_and_layout_park_not_execution_restore() {
    let (ctx, mut app) = setup();
    seed_ui(&ctx, &mut app);
    let id = app.changes.review_comments.items[0].id;
    assert!(app.changes.review_comments.request_delete(id));
    app.changes.review_inputs(
        &ctx,
        false,
        &app.outputs,
        app.output_revision,
        &app.workspace,
    );
    assert!(!app.changes.review_comments.confirming());
    assert_eq!(app.changes.review_comments.items.len(), 2);
    assert!(app.changes.review_comments.request_delete(id));
    app.leave_session_layout();
    assert!(!app.changes.review_comments.confirming());
    assert_eq!(app.changes.review_comments.items.len(), 2);
    app.enter_session_layout();
    assert!(!app.changes.review_comments.confirming());
}
