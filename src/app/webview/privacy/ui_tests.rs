use super::*;

fn labels(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(text) => out.push((
            text.galley.job.text.clone(),
            egui::Rect::from_min_size(text.pos, text.galley.size()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                labels(shape, out);
            }
        }
        _ => {}
    }
}
fn frame(
    ctx: &egui::Context,
    browser: &mut BrowserView,
    width: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(width, 760.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| browser.privacy_dialog(ctx, "https://example.test"),
    )
}
fn point(output: egui::FullOutput, label: &str) -> egui::Pos2 {
    let mut text = Vec::new();
    for shape in output.shapes {
        labels(&shape.shape, &mut text);
    }
    text.iter()
        .find(|(value, _)| value == label)
        .unwrap()
        .1
        .center()
}
fn click(ctx: &egui::Context, browser: &mut BrowserView, width: f32, pos: egui::Pos2) {
    frame(
        ctx,
        browser,
        width,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    frame(
        ctx,
        browser,
        width,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
}
fn event(key: egui::Key, modifiers: egui::Modifiers, repeat: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers,
    }
}
fn fixture() -> BrowserView {
    let mut browser = BrowserView {
        active: "https://example.test".into(),
        ..Default::default()
    };
    browser.privacy.ensure("https://example.test");
    browser.stage_privacy(
        "https://example.test",
        Change::Switch(Target::Named("Work".into())),
    );
    browser
}
#[test]
fn dialogs_wrap_at_narrow_width_zoom_and_cancel_without_creating_contexts() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [340.0, 760.0] {
            for zoom in [1.0, 1.4] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let mut browser = fixture();
                let old = browser.privacy.context("https://example.test");
                for _ in 0..3 {
                    frame(&ctx, &mut browser, width, vec![]);
                }
                let output = frame(&ctx, &mut browser, width, vec![]);
                let dialog = ctx
                    .read_response(egui::Id::new("browser-private-context-confirm"))
                    .unwrap()
                    .rect;
                assert!(
                    ctx.screen_rect().contains_rect(dialog),
                    "{dialog:?} at {width}/{zoom}"
                );
                let cancel = point(output, "Cancel");
                assert!(ctx.screen_rect().contains(cancel));
                click(&ctx, &mut browser, width, cancel);
                assert!(!browser.privacy_confirming());
                assert!(browser.privacy.groups.is_empty());
                assert_eq!(browser.privacy.context("https://example.test"), old);
                assert!(!browser.session_suspended("https://example.test"));
            }
        }
    }
}
#[test]
fn confirmation_is_explicit_suspends_current_only_and_does_not_start_cef() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut browser = fixture();
    let other = browser.privacy.ensure("other");
    for _ in 0..3 {
        frame(&ctx, &mut browser, 760.0, vec![]);
    }
    let confirm = point(frame(&ctx, &mut browser, 760.0, vec![]), "Switch group");
    click(&ctx, &mut browser, 760.0, confirm);
    assert!(!browser.privacy_confirming());
    assert_eq!(browser.privacy.groups.len(), 1);
    assert!(browser.session_suspended("https://example.test"));
    assert_eq!(browser.privacy.context("other"), Some(other));
    assert!(!browser.session_suspended("other"));
    assert!(browser.state("https://example.test").location.is_empty());
}
#[test]
fn disabled_enter_chords_repeat_escape_and_stale_page_cannot_confirm() {
    for disabled in [false, true] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut browser = fixture();
        browser.unavailable = disabled;
        for _ in 0..3 {
            frame(&ctx, &mut browser, 760.0, vec![]);
        }
        for (modifiers, repeat) in [
            (egui::Modifiers::COMMAND, false),
            (egui::Modifiers::SHIFT, false),
            (egui::Modifiers::NONE, true),
        ] {
            frame(
                &ctx,
                &mut browser,
                760.0,
                vec![event(egui::Key::Enter, modifiers, repeat)],
            );
            assert!(browser.privacy.groups.is_empty());
            assert!(browser.privacy_confirming());
        }
        if disabled {
            let confirm = point(frame(&ctx, &mut browser, 760.0, vec![]), "Switch group");
            click(&ctx, &mut browser, 760.0, confirm);
            assert!(browser.privacy.groups.is_empty());
            assert!(browser.privacy_confirming());
        }
        frame(
            &ctx,
            &mut browser,
            760.0,
            vec![event(egui::Key::Escape, egui::Modifiers::NONE, false)],
        );
        assert!(!browser.privacy_confirming());
        assert!(browser.privacy.groups.is_empty());
    }
    let ctx = egui::Context::default();
    let mut browser = fixture();
    browser.privacy.pending.as_mut().unwrap().page = Some(("https://stale.test".into(), 1));
    frame(&ctx, &mut browser, 760.0, vec![]);
    assert!(!browser.privacy_confirming());
    assert!(browser.privacy.groups.is_empty());
}
#[test]
fn menu_creation_needs_click_then_confirmation_and_resume_obeys_disabled_state() {
    for enabled in [false, true] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let key = "https://example.test";
        let mut browser = BrowserView {
            active: key.into(),
            ..Default::default()
        };
        browser.privacy.ensure(key);
        browser.privacy.draft = "Work".into();
        let draw = |browser: &mut BrowserView, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(420.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.add_enabled_ui(enabled, |ui| browser.privacy_menu(ui, key));
                    });
                },
            )
        };
        for _ in 0..3 {
            draw(&mut browser, vec![]);
        }
        let create = point(draw(&mut browser, vec![]), "Create and switch…");
        for pressed in [true, false] {
            draw(
                &mut browser,
                vec![
                    egui::Event::PointerMoved(create),
                    egui::Event::PointerButton {
                        pos: create,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert_eq!(browser.privacy_confirming(), enabled);
        assert!(browser.privacy.groups.is_empty());
    }
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut browser = fixture();
    let key = "https://example.test";
    let effect = browser.privacy.decide(key, true).unwrap();
    browser.apply_privacy_effect(effect);
    let draw = |browser: &mut BrowserView, enabled, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(420.0, 600.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_enabled_ui(enabled, |ui| {
                        assert!(browser.session_notice(ui, key));
                    });
                });
            },
        )
    };
    for enabled in [false, true] {
        for _ in 0..3 {
            draw(&mut browser, enabled, vec![]);
        }
        let resume = point(draw(&mut browser, enabled, vec![]), "Resume");
        for pressed in [true, false] {
            draw(
                &mut browser,
                enabled,
                vec![
                    egui::Event::PointerMoved(resume),
                    egui::Event::PointerButton {
                        pos: resume,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert_eq!(browser.session_suspended(key), !enabled);
    }
}

#[test]
fn key_guard_removes_modified_and_repeat_button_activation() {
    let ctx = egui::Context::default();
    let _ = ctx.run(
        egui::RawInput {
            events: vec![
                event(egui::Key::Enter, egui::Modifiers::COMMAND, false),
                event(egui::Key::Space, egui::Modifiers::NONE, true),
                event(egui::Key::Enter, egui::Modifiers::NONE, false),
            ],
            ..Default::default()
        },
        |ctx| {
            guard_keys(ctx);
            let events = ctx.input(|input| input.events.clone());
            assert_eq!(events.len(), 1);
            assert!(matches!(
                events[0],
                egui::Event::Key {
                    modifiers: egui::Modifiers::NONE,
                    repeat: false,
                    ..
                }
            ));
        },
    );
}
