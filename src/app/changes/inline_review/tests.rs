use super::*;
use crate::app::files::tests::frame;
pub(super) const PATH: &str = "src/inline.rs";
pub(super) const DIFF: &str = "@@ -10,2 +10,2 @@ first\n-old first\n+chosen first\n context\n@@ -50 +50 @@ second\n-old second\n+chosen second\n";
pub(super) fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
pub(super) fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
pub(super) fn setup(git: bool, source: &str) -> (egui::Context, Pui) {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.prefs.hide_sidebar = true;
    app.prefs.hide_inspector = true;
    app.inspector_panel = true;
    app.draft = "Kept draft".into();
    app.state.busy = true;
    // Prevent the demo transcript projection from replacing this explicit fixture.
    app.output_revision = app.state.outputs_revision;
    app.outputs = vec![RecordedFile {
        path: PATH.into(),
        diff: source.into(),
        ..Default::default()
    }];
    app.workspace = Workspace {
        root: app.prefs.cwd.clone(),
        is_git: true,
        files: vec![workspace::FileChange {
            path: PATH.into(),
            status: " M".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let owner = app.review_comment_owner();
    app.changes.bind(&app.prefs.cwd, &owner);
    app.changes.source = if git { Source::Git } else { Source::Recorded };
    app.changes.open_rows.insert((git, PATH.into()));
    if git {
        app.changes.test_diff(PATH, source);
    }
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app)
}
pub(super) fn nav(app: &Pui, git: bool) -> &diff_viewer::Navigation {
    app.changes.inline.rows[&(git, PATH.into())]
        .navigation
        .as_ref()
        .unwrap()
}
pub(super) fn control(ctx: &egui::Context, name: &str) -> egui::Id {
    ctx.data(|data| data.get_temp::<egui::Id>(egui::Id::new(name)))
        .unwrap()
}
pub(super) fn click_control(ctx: &egui::Context, app: &mut Pui, name: &str) {
    let rect = ctx.read_response(control(ctx, name)).unwrap().rect;
    for pressed in [true, false] {
        frame(ctx, app, pointer(rect.center(), pressed));
    }
}
pub(super) fn approval(app: &mut Pui) -> mpsc::Receiver<Value> {
    app.state.dialogs.push_back(json!({"id":"inline-approval", "method":"confirm", "title":"Allow bash?", "message":"Run\n{\"command\":\"echo fixture\"}\n\nApprove this call only."}));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    commands
}
#[test]
fn production_git_and_recorded_rows_reuse_navigation_wrap_and_no_digest_or_popup() {
    for git in [false, true] {
        let (ctx, mut app) = setup(git, DIFF);
        assert!(app.changes.review.is_none());
        assert!(!egui::Popup::is_any_open(&ctx));
        assert!(nav(&app, git).comment_digest().is_none());
        let owner = nav(&app, git).focus_id();
        click_control(&ctx, &mut app, "diff-next-hunk-focus");
        let output = frame(&ctx, &mut app, vec![]);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.job.text == "Hunk 1 / 2")));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::End, egui::Modifiers::ALT)],
        );
        assert!(ctx.data(|data| data.get_temp::<(usize, egui::Rect)>(egui::Id::new("diff-hunk-target"))).is_some_and(|(row,_)| row == 4));
        click_control(&ctx, &mut app, "diff-wrap-lines-focus");
        assert!(nav(&app, git).wrap());
        assert_eq!(nav(&app, git).focus_id(), owner);
        assert_eq!(app.outputs[0].diff, DIFF);
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(!app.changes.open_rows.contains(&(git, PATH.into())));
        assert!(app.changes.inline.rows.is_empty());
        let pos = ctx
            .read_response(app.changes.inline_header_id(git, PATH))
            .unwrap()
            .rect
            .center();
        if git {
            // Seed before expansion so this UI fixture never starts a Git read.
            app.changes.test_diff(PATH, DIFF);
        }
        for pressed in [true, false] {
            frame(&ctx, &mut app, pointer(pos, pressed));
        }
        frame(&ctx, &mut app, vec![]);
        assert!(
            nav(&app, git).wrap(),
            "reopened rows inherit the explicit window preference"
        );
    }
}
#[test]
fn native_enter_space_first_tab_and_reverse_tab_escape_do_not_approve_send_or_stop() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app) = setup(false, DIFF);
        let commands = approval(&mut app);
        let header = app.changes.inline_header_id(false, PATH);
        ctx.memory_mut(|memory| memory.request_focus(header));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
        );
        // First hunk has no Previous action. Native Tab lands on enabled Next.
        let next = control(&ctx, "diff-next-hunk-focus");
        assert_eq!(ctx.memory(|memory| memory.focused()), Some(next));
        frame(&ctx, &mut app, vec![key(activation, egui::Modifiers::NONE)]);
        assert!(nav(&app, false).owns_focus(&ctx));
        let wrap = control(&ctx, "diff-wrap-lines-focus");
        ctx.memory_mut(|memory| memory.request_focus(wrap));
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
        assert!(app.changes.open_rows.is_empty());
        assert_eq!(ctx.memory(|memory| memory.focused()), Some(header));
        // The collapse atomically restores an Escape-locking focus filter.
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert_eq!(app.draft, "Kept draft");
        assert!(app.state.busy);
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
    }
}
#[test]
fn pointer_press_and_release_command_chords_never_activate_global_or_native_control() {
    for chord in [egui::Key::Enter, egui::Key::Backspace] {
        for on_press in [false, true] {
            let (ctx, mut app) = setup(false, DIFF);
            let commands = approval(&mut app);
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
            let pos = ctx
                .read_response(control(&ctx, "diff-wrap-lines-focus"))
                .unwrap()
                .rect
                .center();
            for pressed in [true, false] {
                let mut events = pointer(pos, pressed);
                if pressed == on_press {
                    events.push(key(chord, egui::Modifiers::COMMAND));
                }
                frame(&ctx, &mut app, events);
            }
            assert!(nav(&app, false).wrap());
            assert_eq!(app.draft, "Kept draft");
            assert!(app.state.busy);
            assert_eq!(app.state.dialogs.len(), 1);
            assert!(commands.try_recv().is_err());
        }
    }
}
#[test]
fn newly_arriving_structured_question_cannot_reuse_control_activation_or_take_focus() {
    for activation in [egui::Key::Enter, egui::Key::Space, egui::Key::Num1] {
        let (ctx, mut app) = setup(false, DIFF);
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let next = control(&ctx, "diff-next-hunk-focus");
        ctx.memory_mut(|memory| memory.request_focus(next));
        let mut dialog = questions::demo_dialog();
        let mut payload: Value =
            serde_json::from_str(dialog["placeholder"].as_str().unwrap()).unwrap();
        let first = payload["questions"][0].clone();
        payload["questions"] = json!([first]);
        dialog["placeholder"] = json!(payload.to_string());
        app.state.dialogs.push_back(dialog);
        frame(&ctx, &mut app, vec![key(activation, egui::Modifiers::NONE)]);
        assert!(nav(&app, false).owns_focus(&ctx));
        assert!(app.pending_dialog.is_none());
        // A one-question fixture catches otherwise invisible page advancement:
        // any reused option activation would complete/remove the dialog.
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
        assert_eq!(app.draft, "Kept draft");
    }
}
#[test]
fn foreign_focus_popup_modal_hidden_query_and_session_do_not_claim_hunk_keys() {
    let (ctx, mut app) = setup(false, DIFF);
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::End, egui::Modifiers::ALT)],
    );
    assert!(
        ctx.data(|data| data.get_temp::<(usize, egui::Rect)>(egui::Id::new("diff-hunk-target")))
            .is_none()
    );
    for blocked in [0, 1, 2] {
        nav(&app, false).focus(&ctx);
        if blocked == 0 {
            egui::Popup::open_id(&ctx, egui::Id::new("foreign-inline-menu"));
        }
        if blocked == 1 {
            app.changes.query = "not-visible".into();
        }
        if blocked == 2 {
            app.settings_open = true;
        }
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::End, egui::Modifiers::ALT)],
        );
        assert!(ctx.data(|data| data.get_temp::<(usize, egui::Rect)>(egui::Id::new("diff-hunk-target"))).is_none());
        egui::Popup::close_all(&ctx);
        app.changes.query.clear();
        app.settings_open = false;
    }
    app.state.session_file = Some("new-inline-session".into());
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::End, egui::Modifiers::ALT)],
    );
    assert!(app.changes.inline.rows.is_empty());
    assert!(app.changes.open_rows.is_empty());
}
#[test]
fn foreign_modal_gets_first_refusal_for_escape_and_hunk_input() {
    let (ctx, mut app) = setup(false, DIFF);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |_| {});
        prompt_ui::modal(ctx, egui::Id::new("inline-foreign-modal"), 280.0, |ui| {
            prompt_ui::heading(ui, "Information", "Not inline review");
        });
    });
    nav(&app, false).focus(&ctx);
    let _ = ctx.run(
        egui::RawInput {
            events: vec![
                key(egui::Key::End, egui::Modifiers::ALT),
                key(egui::Key::Escape, egui::Modifiers::NONE),
            ],
            ..Default::default()
        },
        |ctx| {
            app.changes
                .review_inputs(ctx, true, &app.outputs, app.output_revision, &app.workspace);
            assert_eq!(ctx.input(|input| input.events.len()), 2);
        },
    );
    assert!(app.changes.open_rows.contains(&(false, PATH.into())));
}
#[test]
fn modified_keyboard_activation_cannot_toggle_native_wrap_or_global_actions() {
    let (ctx, mut app) = setup(false, DIFF);
    let commands = approval(&mut app);
    let wrap = control(&ctx, "diff-wrap-lines-focus");
    ctx.memory_mut(|memory| memory.request_focus(wrap));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
    );
    assert!(!nav(&app, false).wrap());
    assert_eq!(app.draft, "Kept draft");
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(commands.try_recv().is_err());
}
#[test]
fn cap_disables_extra_expansion_without_eviction_or_worker_and_reset_is_revision_driven() {
    let (ctx, mut app) = setup(false, DIFF);
    click_control(&ctx, &mut app, "diff-wrap-lines-focus");
    for i in 1..MAX_OPEN_ROWS {
        app.changes.open_rows.insert((false, format!("file-{i}")));
    }
    assert!(!app.changes.can_expand_inline("extra.rs"));
    let before = app.changes.open_rows.clone();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            assert!(!app.changes.header(ui, "extra.rs", "0", "0", true));
        });
    });
    assert_eq!(app.changes.open_rows, before);
    assert!(app.changes.diffs.is_empty());
    // No body hashing/comparison: only explicit projection revision resets state.
    app.output_revision = app.output_revision.wrapping_add(1);
    app.changes
        .bind_inline_revision(&ctx, &app.outputs, app.output_revision);
    assert!(app.changes.inline.rows.is_empty());
    assert_eq!(app.changes.open_rows.len(), 1);
    app.changes.invalidate();
    assert!(app.changes.inline.rows.is_empty());
    assert!(
        !app.changes
            .can_expand_inline(&"x".repeat(MAX_INLINE_PATH + 1))
    );
}
#[test]
fn untracked_is_neutral_without_worker_or_empty_tinted_selectable_code() {
    let (ctx, mut app) = setup(true, DIFF);
    app.workspace.files[0].status = "??".into();
    app.changes.diffs.clear();
    app.changes.inline.clear();
    let output = frame(&ctx, &mut app, vec![]);
    assert!(app.changes.diffs.is_empty());
    assert!(app.changes.inline.rows.is_empty());
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.job.text == "Untracked file")));
    assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.job.text.contains("+chosen"))));
}
