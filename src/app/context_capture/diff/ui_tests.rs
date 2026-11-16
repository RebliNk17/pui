use super::tests::{display, pointer, pos, range};
use super::*;
use crate::app::files::tests::{app, frame};

const DIFF: &str = "@@ -10,2 +10,2 @@\n-old\n+new selected\n context\n";
fn setup() -> (egui::Context, Pui) {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.prefs.hide_sidebar = true;
    app.prefs.hide_inspector = true;
    app.open_diff_review(("src/selected.rs".into(), DIFF.into()).into(), false);
    app.draft = "Kept draft".into();
    app.state.busy = true;
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app)
}
fn select_menu(ctx: &egui::Context, app: &mut Pui) {
    let output = frame(ctx, app, vec![]);
    let range = range(&display(DIFF), "-old", "+new selected");
    let start = pos(&output, range.start);
    let end = pos(&output, range.end);
    frame(ctx, app, pointer(start, egui::PointerButton::Primary, true));
    frame(ctx, app, vec![egui::Event::PointerMoved(end)]);
    frame(ctx, app, pointer(end, egui::PointerButton::Primary, false));
    for pressed in [true, false] {
        frame(
            ctx,
            app,
            pointer(start, egui::PointerButton::Secondary, pressed),
        );
    }
    for _ in 0..3 {
        frame(ctx, app, vec![]);
    }
}
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn utility_diff_menu_keyboard_add_preserves_live_approval_run_draft_and_source() {
    let (ctx, mut app) = setup();
    let permission = json!({"id":"diff-context-permission", "method":"confirm", "title":"Allow bash?", "message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."});
    assert!(prompts::approval(&permission).is_some());
    app.state.dialogs.push_back(permission);
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let count = app.state.messages.len();
    select_menu(&ctx, &mut app);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
    );
    assert_eq!(app.draft, "Kept draft");
    assert!(app.pending_dialog.is_none());
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(
        app.draft
            .starts_with("Kept draft\n\n[Context: src/selected.rs · Working tree vs HEAD")
    );
    assert!(app.draft.ends_with("\n-old\n+new selected\n[/Context]"));
    assert!(app.state.busy);
    assert_eq!(app.state.messages.len(), count);
    assert!(commands.try_recv().is_err());
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.pending_dialog.is_none());
    assert_eq!(app.changes.review.as_ref().unwrap().1, DIFF);
}

#[test]
fn hunk_shortcuts_leave_real_frozen_selection_menu_and_draft_untouched() {
    let (ctx, mut app) = setup();
    select_menu(&ctx, &mut app);
    let source = app.changes.review.as_ref().unwrap().1.clone();
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::ALT)],
    );
    assert!(egui::Popup::is_any_open(&ctx));
    assert_eq!(app.changes.review.as_ref().unwrap().1, source);
    assert_eq!(app.draft, "Kept draft");
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!egui::Popup::is_any_open(&ctx));
    assert!(app.changes.review.is_some() && app.state.busy);
}

#[test]
fn demo_snapshot_is_explicitly_recorded_and_uses_no_provider() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_diff_context: true,
            ..Default::default()
        },
    );
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let document = app.changes.review.as_ref().unwrap();
    let (path, diff) = (&document.0, &document.1);
    assert!(
        app.outputs
            .iter()
            .any(|file| &file.path == path && &file.diff == diff)
    );
    let output = frame(&ctx, &mut app, vec![]);
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Recorded edits · read-only")));
    assert!(app.client.is_none());
}

#[test]
fn utility_diff_escape_dismisses_menu_before_window_without_stopping_chat() {
    let (ctx, mut app) = setup();
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    select_menu(&ctx, &mut app);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!egui::Popup::is_any_open(&ctx));
    assert!(app.changes.review.is_some());
    assert!(
        app.changes.review.as_ref().unwrap().2.owns_focus(&ctx),
        "focus after dismiss: {:?}",
        ctx.memory(|m| m.focused())
    );
    assert_eq!(app.draft, "Kept draft");
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(app.changes.review.is_none());
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
}

#[test]
fn closing_diff_or_switching_source_never_applies_an_unconfirmed_snapshot() {
    let (ctx, mut app) = setup();
    select_menu(&ctx, &mut app);
    app.changes.review = None;
    frame(&ctx, &mut app, vec![]);
    assert!(!egui::Popup::is_any_open(&ctx));
    assert_eq!(app.draft, "Kept draft");
    app.open_diff_review(("src/selected.rs".into(), DIFF.into()).into(), false);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    select_menu(&ctx, &mut app);
    app.leave_session_layout();
    assert!(!egui::Popup::is_any_open(&ctx));
    assert_eq!(app.draft, "Kept draft");
    assert!(!egui::text_selection::LabelSelectionState::load(&ctx).has_selection());
}

#[test]
fn diff_menu_geometry_and_settled_paint_fit_narrow_zoom_themes() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut time = 0.0;
            let mut draw = |events| {
                time += 0.016;
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 560.0),
                        )),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        context_capture::inputs(ctx);
                        diff_viewer::show(ctx, "src/narrow.rs", DIFF, true);
                    },
                )
            };
            for _ in 0..5 {
                draw(vec![]);
            }
            let selected = range(&display(DIFF), "+new", "+new selected");
            let output = draw(vec![]);
            let start = pos(&output, selected.start);
            let end = pos(&output, selected.end);
            draw(pointer(start, egui::PointerButton::Primary, true));
            draw(vec![egui::Event::PointerMoved(end)]);
            draw(pointer(end, egui::PointerButton::Primary, false));
            for pressed in [true, false] {
                draw(pointer(start, egui::PointerButton::Secondary, pressed));
            }
            for _ in 0..100 {
                draw(vec![]);
            }
            let output = draw(vec![]);
            let target = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
                .unwrap();
            assert!(ctx.screen_rect().contains_rect(target));
            assert!(
                output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                    > Duration::from_secs(1)
            );
        }
    }
}

#[test]
fn inline_recorded_and_working_diff_use_actual_source_and_session_identity() {
    for recorded in [false, true] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.prefs.hide_sidebar = true;
        app.prefs.hide_inspector = true;
        app.inspector_panel = true;
        app.state.busy = true;
        app.draft = "Kept".into();
        app.state.messages = vec![Message {
            tools: vec![protocol::Tool {
                name: "edit".into(),
                path: "src/selected.rs".into(),
                diff: DIFF.into(),
                added: 1,
                removed: 1,
                finished: true,
                ..Default::default()
            }],
            ..Default::default()
        }];
        app.state.outputs_revision = app.state.outputs_revision.wrapping_add(1);
        app.state.prose_revision = app.state.prose_revision.wrapping_add(1);
        app.outputs = activity::recorded_files(&app.state.messages);
        app.workspace = Workspace {
            is_git: true,
            root: app.prefs.cwd.clone(),
            files: vec![workspace::FileChange {
                path: "src/selected.rs".into(),
                status: " M".into(),
                added: "1".into(),
                removed: "1".into(),
            }],
            ..Default::default()
        };
        // UI opens the recorded row; the Git fixture injects the completed worker result below.
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let output = frame(&ctx, &mut app, vec![]);
        let find = |output: &egui::FullOutput, needle: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == needle => {
                        Some(text.pos + text.galley.size() / 2.0)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("Missing {needle}, recorded={recorded}"))
        };
        if !recorded {
            let target = find(&output, "Current Git");
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut app,
                    pointer(target, egui::PointerButton::Primary, pressed),
                );
            }
            app.changes.test_diff("src/selected.rs", DIFF);
        }
        let output = frame(&ctx, &mut app, vec![]);
        let target = find(&output, "src/selected.rs");
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut app,
                pointer(target, egui::PointerButton::Primary, pressed),
            );
        }
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        select_menu(&ctx, &mut app);
        let target = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
            .unwrap()
            .center();
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut app,
                pointer(target, egui::PointerButton::Primary, pressed),
            );
        }
        let kind = if recorded {
            "Recorded edits (not net Git)"
        } else {
            "Working tree vs HEAD"
        };
        assert!(app.draft.contains(kind), "{}", app.draft);
        assert!(app.draft.ends_with("\n-old\n+new selected\n[/Context]"));
        assert!(app.changes.review.is_none());
        assert!(app.state.busy);
    }
}
