use super::tests::{fixture, key};
use super::*;
use crate::app::files::tests::frame;
#[test]
fn explicit_merged_pointer_and_native_keyboard_toggle_keeps_draft_and_questions() {
    for keyboard in [false, true] {
        let (ctx, mut app) = fixture();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let (id, rect) = ctx
            .data(|data| data.get_temp::<(egui::Id, egui::Rect)>(popup_id().with("history-merged")))
            .unwrap();
        app.state.dialogs.push_back(questions::demo_dialog());
        if keyboard {
            ctx.memory_mut(|memory| memory.request_focus(id));
            frame(
                &ctx,
                &mut app,
                vec![
                    key(egui::Key::Space, egui::Modifiers::NONE),
                    egui::Event::Text(" ".into()),
                ],
            );
        } else {
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::PointerButton {
                            pos: rect.center(),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                        key(egui::Key::Enter, egui::Modifiers::COMMAND),
                        key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                    ],
                );
            }
        }
        assert!(app.changes.branches.history.merged);
        assert_eq!(app.changes.branches.history.requests, 0);
        assert_eq!(app.draft, "Kept draft");
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
        for _ in 0..10 {
            frame(&ctx, &mut app, vec![]);
        }
        assert_eq!(app.changes.branches.history.requests, 0);
    }
}
#[test]
fn merged_native_tab_enter_reverse_escape_mode_reset_and_narrow_themes() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app) = fixture();
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            let tab = app.changes.branches.tabs[1].unwrap();
            ctx.memory_mut(|memory| memory.request_focus(tab));
            let target = app.changes.branches.history.mode_control.unwrap();
            for _ in 0..8 {
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
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
            );
            assert!(app.changes.branches.history.merged);
            let start = std::time::Instant::now();
            for _ in 0..15 {
                let mut native = eframe::Frame::_new_kittest();
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 780.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut native),
                );
                assert!(output.platform_output.commands.is_empty());
            }
            assert!(
                ctx.screen_rect()
                    .contains_rect(ctx.memory(|memory| memory.area_rect(popup_id())).unwrap())
            );
            eprintln!("15mergedmodeframes {:?}", start.elapsed());
            let target = app.changes.branches.history.mode_control.unwrap();
            ctx.memory_mut(|memory| memory.request_focus(target));
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
            assert!(!app.changes.branches.open);
            assert!(!app.changes.branches.history.merged);
            assert_eq!(app.draft, "Kept draft");
        }
    }
}
#[test]
fn real_merged_request_uses_shared_permit_and_owner_cancellation() {
    let root = crate::workspace::tests::fixture();
    let ctx = egui::Context::default();
    let owner = Owner::new(
        &(root.clone(), "owner".into()),
        &Workspace {
            root: root.clone(),
            is_git: true,
            branch: "observed".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let before = crate::workspace::tests::snapshot(&root);
    let mut history = History {
        active: true,
        merged: true,
        ..Default::default()
    };
    history.request(&ctx, owner.clone());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while history.pending.is_some() {
        history.poll(Some(owner.clone()));
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(history.snapshot.is_some());
    assert!(history.merged);
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    history.poll(None);
    assert!(history.snapshot.is_none());
    assert!(history.mode_control.is_none());
}
