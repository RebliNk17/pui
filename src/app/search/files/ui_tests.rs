use super::tests::{app, search};
use super::*;

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
fn frame(app: &mut Pui, size: Vec2, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.ctx.clone();
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    )
}
#[test]
fn shortcut_claims_composer_keyboard_and_respects_approvals() {
    let mut app = app();
    let size = Vec2::new(1134.0, 900.0);
    app.state.busy = true;
    app.draft = "Never send".into();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    for _ in 0..3 {
        frame(&mut app, size, vec![]);
    }
    let count = app.state.messages.len();
    app.ctx
        .memory_mut(|m| m.request_focus(egui::Id::new("composer")));
    frame(
        &mut app,
        size,
        vec![
            key(egui::Key::P, egui::Modifiers::COMMAND),
            key(egui::Key::Enter, egui::Modifiers::NONE),
        ],
    );
    assert_eq!(app.draft, "Never send");
    assert_eq!(app.state.messages.len(), count);
    assert!(app.state.busy);
    assert!(commands.try_iter().all(|command| !matches!(
        command["type"].as_str(),
        Some("prompt" | "abort" | "switch_session")
    )));
    app.close_search();
    app.state.dialogs.push_back(questions::demo_dialog());
    frame(
        &mut app,
        size,
        vec![key(egui::Key::P, egui::Modifiers::COMMAND)],
    );
    assert!(!app.search_open);
    assert_eq!(app.state.dialogs.len(), 1);
    app.state.dialogs.clear();
    frame(
        &mut app,
        size,
        vec![key(egui::Key::P, egui::Modifiers::COMMAND)],
    );
    assert!(app.search_open);
    assert_eq!(app.spotlight.scope, Scope::Files);
    frame(
        &mut app,
        size,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!app.search_open && app.state.busy);
    assert_eq!(app.draft, "Never send");
}
#[test]
fn actual_keyboard_and_pointer_results_open_readonly_and_cancel_does_not_open() {
    for pointer in [false, true] {
        let mut app = app();
        app.open_quick_files();
        app.search = "example.rs:1".into();
        search(&mut app);
        let size = Vec2::new(1134.0, 900.0);
        for _ in 0..3 {
            frame(&mut app, size, vec![]);
        }
        if pointer {
            let rect = app
                .ctx
                .data(|d| d.get_temp::<egui::Rect>(egui::Id::new(("spotlight-row", 0))))
                .unwrap();
            for pressed in [true, false] {
                frame(
                    &mut app,
                    size,
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
        } else {
            frame(
                &mut app,
                size,
                vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
            );
        }
        assert!(!app.search_open);
        assert!(app.files.files_open());
        assert!(
            app.files
                .navigation_target()
                .unwrap()
                .ends_with("src/nested/example.rs")
        );
    }
    let mut app = app();
    app.open_quick_files();
    app.search = "example.rs".into();
    search(&mut app);
    let size = Vec2::new(1134.0, 900.0);
    for _ in 0..3 {
        frame(&mut app, size, vec![]);
    }
    for pressed in [true, false] {
        frame(
            &mut app,
            size,
            vec![
                egui::Event::PointerMoved(egui::pos2(3.0, 3.0)),
                egui::Event::PointerButton {
                    pos: egui::pos2(3.0, 3.0),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(!app.search_open);
    assert!(!app.files.is_open());
}
#[test]
fn narrow_theme_zoom_error_results_fit_and_settled_frames_keep_projection_and_worker() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.4] {
            let mut app = app();
            app.prefs.color_styles.apply(&app.ctx, theme);
            app.ctx.set_zoom_factor(zoom);
            app.open_quick_files();
            app.search = "example.rs:123".into();
            search(&mut app);
            let size = Vec2::new(360.0, 560.0);
            for _ in 0..100 {
                frame(&mut app, size, vec![]);
            }
            let paths = app.spotlight.files.paths.as_ptr();
            let hits = app.spotlight.hits.as_ptr();
            let cancel = app.spotlight.files.cancel.clone();
            let out = frame(&mut app, size, vec![]);
            assert!(Arc::ptr_eq(&cancel, &app.spotlight.files.cancel));
            assert!(!app.spotlight.files.pending());
            assert_eq!(paths, app.spotlight.files.paths.as_ptr());
            assert_eq!(hits, app.spotlight.hits.as_ptr());
            assert!(
                !out.viewport_output[&egui::ViewportId::ROOT]
                    .repaint_delay
                    .is_zero()
            );
            for query in ["example.rs:123", "example.rs:0"] {
                app.search = query.into();
                search(&mut app);
                for _ in 0..3 {
                    frame(&mut app, size, vec![]);
                }
                let rect = egui::AreaState::load(&app.ctx, egui::Id::new("spotlight-search"))
                    .unwrap()
                    .rect();
                assert!(
                    rect.left() >= 0.0
                        && rect.right() <= size.x + 1.0
                        && rect.top() >= 0.0
                        && rect.bottom() <= size.y + 1.0,
                    "{rect:?}, {zoom}"
                );
            }
        }
    }
}
