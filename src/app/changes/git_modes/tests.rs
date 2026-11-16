use super::*;
#[test]
fn full_app_mode_focus_sendstop_escape_text_and_question_are_isolated() {
    use crate::app::files::tests as app_test;
    let ctx = egui::Context::default();
    let mut app = app_test::app(&ctx);
    app.inspector_panel = true;
    app.prefs.hide_sidebar = true;
    app.focus_composer = false;
    let owner = app.review_comment_owner();
    app.changes.bind(&app.prefs.cwd.clone(), &owner);
    app.changes.source = Source::Git;
    app.draft = "Keep mode draft".into();
    app.state.busy = true;
    for _ in 0..4 {
        app_test::frame(&ctx, &mut app, vec![]);
    }
    let (id, _) = ctx
        .data(|d| d.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new(("git-diff-mode", "Staged"))))
        .unwrap();
    ctx.memory_mut(|m| m.request_focus(id));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.dialogs.push_back(questions::demo_dialog());
    let key = |key, modifiers| egui::Event::Key {
        key,
        modifiers,
        pressed: true,
        repeat: false,
        physical_key: None,
    };
    app_test::frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
            key(egui::Key::Escape, egui::Modifiers::NONE),
            egui::Event::Text("bad".into()),
            egui::Event::Paste("bad".into()),
        ],
    );
    assert!(commands.try_recv().is_err());
    assert_eq!(app.draft, "Keep mode draft");
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.state.busy);
    assert!(app.changes.git_mode == workspace::DiffMode::Combined);
}
#[test]
fn status_projection_keeps_mm_in_both_and_untracked_only_unstaged() {
    let mut changes = Changes::default();
    for (status, staged, unstaged) in [
        ("MM", true, true),
        ("M ", true, false),
        (" M", false, true),
        ("??", false, true),
        ("A ", true, false),
    ] {
        let file = workspace::FileChange {
            status: status.into(),
            ..Default::default()
        };
        changes.git_mode = workspace::DiffMode::Staged;
        assert_eq!(changes.git_mode_includes(&file), staged);
        changes.git_mode = workspace::DiffMode::Unstaged;
        assert_eq!(changes.git_mode_includes(&file), unstaged);
    }
}
#[test]
fn pointer_mode_switch_themes_zoom_clears_cached_body_without_index_action() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut changes = Changes {
                source: Source::Git,
                ..Default::default()
            };
            changes.test_diff("x", "old cached body");
            let draw = |changes: &mut Changes, events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 620.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        changes.git_mode_inputs(ctx, true);
                        egui::CentralPanel::default().show(ctx, |ui| changes.git_mode_controls(ui));
                    },
                )
            };
            for _ in 0..3 {
                draw(&mut changes, vec![]);
            }
            let (_, rect) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new(("git-diff-mode", "Staged")))
                })
                .unwrap();
            for pressed in [true, false] {
                draw(
                    &mut changes,
                    vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::PointerButton {
                            pos: rect.center(),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert!(changes.git_mode == workspace::DiffMode::Staged);
            assert!(changes.diffs.is_empty());
            assert!(!changes.refresh);
        }
    }
}
