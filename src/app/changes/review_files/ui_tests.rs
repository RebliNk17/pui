use super::*;
use crate::app::files::tests as file_tests;

fn app(ctx: &egui::Context) -> Pui {
    let mut app = file_tests::app(ctx);
    app.prefs.hide_sidebar = true;
    file_tests::frame(ctx, &mut app, vec![]); // populate the real recorded-output projection
    let document = (
        app.outputs.last().unwrap().path.clone(),
        app.outputs.last().unwrap().diff.clone(),
    )
        .into();
    app.open_diff_review(document, true);
    app
}
fn control(ctx: &egui::Context, name: &str) -> (egui::Id, egui::Rect, bool) {
    ctx.data(|data| data.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new(name)))
        .unwrap()
}
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}

#[test]
fn demo_review_files_is_real_readonly_projection_without_provider() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_changes: true,
            demo_review_files: true,
            ..Default::default()
        },
    );
    for _ in 0..4 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    assert!(app.client.is_none() && !app.changes.expanded());
    assert!(control(&ctx, "review-next-file").2);
    assert!(
        app.changes
            .review_files
            .label(app.output_revision)
            .starts_with("Recorded file 1 of ")
    );
}

#[test]
fn pointer_keyboard_and_copy_navigate_recorded_files_preserving_live_approval_run_draft() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.busy = true;
    app.draft = "Keep draft".into();
    app.state.dialogs.push_back(json!({"id":"file-review-confirm","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."}));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let count = app.state.messages.len();
    for _ in 0..4 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    assert!(!control(&ctx, "review-previous-file").2);
    assert!(control(&ctx, "review-next-file").2);
    file_tests::click(&ctx, &mut app, control(&ctx, "review-next-file").1.center());
    assert_eq!(
        app.changes.review.as_ref().unwrap().0,
        app.outputs[app.outputs.len() - 2].path
    );
    let (id, _, _) = control(&ctx, "review-next-file");
    ctx.memory_mut(|memory| memory.request_focus(id));
    file_tests::frame(&ctx, &mut app, vec![]);
    file_tests::frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(
        app.changes.review.as_ref().unwrap().0,
        app.outputs[app.outputs.len() - 3].path
    );
    file_tests::frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::ArrowLeft, egui::Modifiers::ALT),
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
        ],
    );
    assert_eq!(
        app.changes.review.as_ref().unwrap().0,
        app.outputs[app.outputs.len() - 2].path
    );
    assert_eq!(app.draft, "Keep draft");
    assert_eq!(app.state.messages.len(), count);
    assert!(app.state.busy && app.state.dialogs.len() == 1 && app.pending_dialog.is_none());
    assert!(commands.try_recv().is_err());
    let copy = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("diff-copy")))
        .unwrap()
        .center();
    let mut copied = false;
    for pressed in [true, false] {
        let out = file_tests::frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(copy),
                egui::Event::PointerButton {
                    pos: copy,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        copied|=out.platform_output.commands.iter().any(|c|matches!(c,egui::OutputCommand::CopyText(text)if text==&app.changes.review.as_ref().unwrap().1));
    }
    assert!(copied);
}

#[test]
fn changed_output_projection_disables_navigation_without_retargeting_frozen_document() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    for _ in 0..4 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    let before = app.changes.review.as_ref().unwrap().1.clone();
    app.state.outputs_revision = app.state.outputs_revision.wrapping_add(1);
    let out = file_tests::frame(&ctx, &mut app, vec![]);
    assert!(!control(&ctx, "review-next-file").2);
    file_tests::text(&out, "Recorded file list changed");
    file_tests::frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::ArrowRight, egui::Modifiers::ALT)],
    );
    assert_eq!(app.changes.review.as_ref().unwrap().1, before);
    assert!(app.changes.review.as_ref().unwrap().3);
    // Git review has its own controls, not recorded-file actions or automatic reads.
    app.open_diff_review(("git.rs".into(), "-old\n+new".into()).into(), false);
    for _ in 0..4 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    let out = file_tests::frame(&ctx, &mut app, vec![]);
    file_tests::text(&out, "Git file navigation unavailable");
    let (_, _, enabled) = ctx
        .data(|data| data.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new("git-next-file")))
        .unwrap();
    assert!(!enabled);
}

#[test]
fn navigation_does_not_consume_composer_or_hidden_settings_keys_and_new_session_does_not_keep_list()
{
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    for _ in 0..4 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    let path = app.changes.review.as_ref().unwrap().0.clone();
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
    file_tests::frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::ArrowRight, egui::Modifiers::ALT)],
    );
    assert_eq!(app.changes.review.as_ref().unwrap().0, path);
    app.settings_open = true;
    file_tests::frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::ArrowRight, egui::Modifiers::ALT)],
    );
    assert_eq!(app.changes.review.as_ref().unwrap().0, path);
    app.leave_session_layout();
    app.state.session_file = Some("/fixture/other-review".into());
    app.agents.active.clear(); // distinct new chat, not the same agent acquiring its path
    app.ensure_agent();
    app.enter_session_layout();
    assert!(app.changes.review.is_none());
}

#[test]
fn narrow_light_dark_zoom_file_controls_fit_and_settle_without_background_projection_copies() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            let mut time = 0.0;
            let mut draw = || {
                time += 0.1;
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(480.0, 900.0),
                        )),
                        time: Some(time),
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut eframe::Frame::_new_kittest()),
                )
            };
            for _ in 0..50 {
                draw();
            }
            let out = draw();
            for name in ["review-previous-file", "review-next-file"] {
                assert!(ctx.screen_rect().contains_rect(control(&ctx, name).1));
            }
            assert!(
                out.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(1)
            );
            assert!(!app.changes.expanded());
        }
    }
}
