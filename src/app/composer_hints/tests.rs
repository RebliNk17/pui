use super::*;

fn app(ctx: &egui::Context) -> Pui {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.connected = true;
    app.state = State::default();
    app.state.session_file = Some("/fixture/composer-hints.jsonl".into());
    app.client = Some(rpc::Client::stub());
    app
}

#[test]
fn rotation_waits_five_seconds_and_never_catches_up_in_a_burst() {
    let mut hints = Hints::default();
    let update = |h: &mut Hints, now| h.update(Some("chat"), Action::Steer, now, true, 3);
    assert_eq!(update(&mut hints, 0.0), (0, Some(Duration::from_secs(5))));
    assert_eq!(update(&mut hints, 4.0).0, 0);
    assert_eq!(update(&mut hints, 5.0).0, 1);
    assert_eq!(update(&mut hints, 50.0).0, 2);
    assert_eq!(update(&mut hints, 55.0).0, 0);
}

#[test]
fn focusing_or_drafting_pauses_and_restarts_a_full_interval() {
    let mut hints = Hints::default();
    hints.update(None, Action::Send, 0.0, true, 3);
    assert_eq!(hints.update(None, Action::Send, 5.0, true, 3).0, 1);
    assert_eq!(hints.update(None, Action::Send, 6.0, false, 3), (1, None));
    assert_eq!(hints.update(None, Action::Send, 60.0, false, 3), (1, None));
    assert_eq!(
        hints.update(None, Action::Send, 61.0, true, 3),
        (1, Some(Duration::from_secs(5)))
    );
}

#[test]
fn switching_session_or_delivery_action_resets_the_hint() {
    let mut hints = Hints::default();
    hints.update(Some("a"), Action::Steer, 0.0, true, 3);
    assert_eq!(hints.update(Some("a"), Action::Steer, 5.0, true, 3).0, 1);
    assert_eq!(hints.update(Some("b"), Action::Steer, 6.0, true, 3).0, 0);
    assert_eq!(hints.update(Some("b"), Action::Steer, 11.0, true, 3).0, 1);
    assert_eq!(hints.update(Some("b"), Action::Queue, 12.0, true, 3).0, 0);
}

fn frame(ctx: &egui::Context, app: &mut Pui, time: f64, width: f32) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 800.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.composer(ui, ctx));
        },
    )
}

fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    fn collect(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => out.push((
                t.galley.text().to_owned(),
                t.galley.rect.translate(t.pos.to_vec2()),
            )),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    for shape in &output.shapes {
        collect(&shape.shape, &mut out);
    }
    out
}
fn contains(output: &egui::FullOutput, text: &str) -> bool {
    texts(output).iter().any(|(t, _)| t == text)
}

#[test]
fn empty_composer_rotates_secondary_hints_but_keeps_the_primary_action_static() {
    for (os, command) in [
        (egui::os::OperatingSystem::Mac, "⌘"),
        (egui::os::OperatingSystem::Windows, "Ctrl+"),
    ] {
        let ctx = egui::Context::default();
        ctx.set_os(os);
        let mut app = app(&ctx);
        app.focus_composer = false;
        app.state.busy = true;
        app.prefs.settings.permission = settings::PermissionMode::Bypass;
        app.prefs.settings.delivery = settings::DeliveryMode::Steer;
        let out = frame(&ctx, &mut app, 0.0, 900.0);
        assert!(contains(&out, "Ask a follow-up…"));
        assert!(contains(&out, "to steer"));
        assert!(
            !texts(&out)
                .iter()
                .any(|(t, _)| t.contains("Bypass all permissions"))
        );
        let out = frame(&ctx, &mut app, 5.0, 900.0);
        assert!(contains(&out, &format!("{command}Enter to queue…")));
        assert!(contains(&out, "to steer"));
        let out = frame(&ctx, &mut app, 10.0, 900.0);
        assert!(contains(&out, "Shift+Enter for a new line…"));
        assert!(contains(&out, "to steer"));
        assert!(app.draft.is_empty());
        assert!(app.deliveries.is_empty());
    }
}

#[test]
fn primary_hint_matches_idle_and_both_busy_delivery_modes() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.focus_composer = false;
    ctx.set_os(egui::os::OperatingSystem::Mac);
    app.prefs.settings.delivery = settings::DeliveryMode::Queue;
    assert!(contains(&frame(&ctx, &mut app, 0.0, 900.0), "to send"));
    app.state.busy = true;
    assert!(contains(&frame(&ctx, &mut app, 1.0, 900.0), "to queue"));
    assert!(contains(
        &frame(&ctx, &mut app, 6.0, 900.0),
        "⌘Enter to steer…"
    ));
    app.prefs.settings.delivery = settings::DeliveryMode::Steer;
    assert!(contains(&frame(&ctx, &mut app, 7.0, 900.0), "to steer"));
}

#[test]
fn focusing_keeps_both_hints_but_typing_hides_them_without_consuming_draft_width() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.busy = true;
    app.prefs.settings.delivery = settings::DeliveryMode::Steer;
    frame(&ctx, &mut app, 0.0, 900.0);
    let out = frame(&ctx, &mut app, 100.0, 900.0);
    assert!(contains(&out, "Ask a follow-up…"));
    assert!(contains(&out, "to steer"));
    assert_eq!(app.composer_hints.next_at, None);
    app.draft = "Keep **this** draft exactly as typed".into();
    let original = app.draft.clone();
    let out = frame(&ctx, &mut app, 101.0, 900.0);
    assert!(!contains(&out, "Ask a follow-up…"));
    assert!(!contains(&out, "to steer"));
    assert!(!contains(&out, "↵"));
    assert_eq!(app.draft, original);
    app.draft.clear();
    assert!(contains(&frame(&ctx, &mut app, 102.0, 900.0), "to steer"));
}

#[test]
fn hints_fit_without_overlap_in_light_dark_narrow_and_zoomed_composers() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        for zoom in [1.0, 1.5] {
            for width in [360.0, 900.0] {
                let ctx = egui::Context::default();
                let mut app = app(&ctx);
                app.focus_composer = false;
                app.state.busy = true;
                app.prefs.settings.delivery = settings::DeliveryMode::Steer;
                theme.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                frame(&ctx, &mut app, 0.0, width);
                let out = frame(&ctx, &mut app, 10.0, width);
                let painted = texts(&out);
                let (_, prompt) = painted
                    .iter()
                    .find(|(t, _)| t.ends_with("to queue…"))
                    .unwrap();
                let (_, key) = painted.iter().find(|(t, _)| t == "↵").unwrap();
                let (_, label) = painted.iter().find(|(t, _)| t == "to steer").unwrap();
                assert!(prompt.right() < key.left(), "prompt={prompt:?} key={key:?}");
                assert!(key.right() < label.left());
                assert!(label.right() <= ctx.screen_rect().right());
                assert!(!painted.iter().any(|(t, _)| t.contains("Enter: Steer")));
            }
        }
    }
}

#[test]
fn placeholder_matches_caret_and_typed_text_in_both_composer_scopes() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        for zoom in [0.8, 1.0, 1.5] {
            for side_chat in [false, true] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                theme.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let mut app = app(&ctx);
                app.state.busy = true;
                app.side_chat.rendering = side_chat;
                let owner = side_chat.then(|| "placeholder-alignment".to_owned());
                let previous = composer_text::set_owner(owner);
                for preview in [false, true] {
                    app.demo = preview;
                    app.focus_composer = true;
                    let mut out = frame(&ctx, &mut app, 0.0, 420.0);
                    for _ in 0..3 {
                        out = frame(&ctx, &mut app, 0.0, 420.0);
                    }
                    let painted = texts(&out);
                    let expected = if preview {
                        "Start a real session to talk to Pi…"
                    } else {
                        "Ask a follow-up…"
                    };
                    let (_, hint) = painted.iter().find(|(text, _)| text == expected).unwrap();
                    let hint = *hint;
                    let caret = ctx
                        .data(|d| {
                            d.get_temp::<egui::Rect>(composer_text::widget_id(
                                &ctx,
                                "composer-caret-rect",
                            ))
                        })
                        .unwrap();
                    assert!(
                        (hint.left() - caret.left()).abs() <= 1.0,
                        "hint={hint:?} caret={caret:?}"
                    );
                    app.draft = "Hello".into();
                    let out = frame(&ctx, &mut app, 0.0, 420.0);
                    let painted = texts(&out);
                    let (_, typed) = painted.iter().find(|(text, _)| text == "Hello").unwrap();
                    assert!(
                        (hint.left() - typed.left()).abs() <= 1.0,
                        "hint={hint:?} typed={typed:?}"
                    );
                    assert!(
                        (hint.top() - typed.top()).abs() <= 1.0,
                        "hint={hint:?} typed={typed:?}"
                    );
                    app.draft.clear();
                }
                composer_text::set_owner(previous);
            }
        }
    }
}

#[test]
fn demo_and_disabled_animation_do_not_schedule_hint_rotation() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.focus_composer = false;
    ctx.style_mut(|style| style.animation_time = 0.0);
    frame(&ctx, &mut app, 0.0, 900.0);
    assert_eq!(app.composer_hints.next_at, None);
    assert!(contains(&frame(&ctx, &mut app, 99.0, 900.0), "Message Pi…"));
    app.demo = true;
    let out = frame(&ctx, &mut app, 100.0, 900.0);
    assert!(contains(&out, "Start a real session to talk to Pi…"));
    assert!(!contains(&out, "to send"));
    assert_eq!(app.composer_hints.next_at, None);
}

#[test]
fn dedicated_demo_shows_real_hints_without_enabling_send() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_composer_hints: true,
            ..Default::default()
        },
    );
    let out = frame(&ctx, &mut app, 0.0, 900.0);
    assert!(app.demo);
    assert!(app.client.is_none());
    assert!(contains(&out, "Ask a follow-up…"));
    assert!(contains(&out, "to steer"));
    let out = frame(&ctx, &mut app, 5.0, 900.0);
    assert!(
        texts(&out)
            .iter()
            .any(|(text, _)| text.ends_with("to queue…"))
    );
    assert!(app.deliveries.is_empty());
}

#[test]
fn shift_enter_retains_newline_behavior_with_the_overlay() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    frame(&ctx, &mut app, 0.0, 900.0);
    app.draft = "draft".into();
    let id = egui::Id::new("composer");
    let mut edit = egui::TextEdit::load_state(&ctx, id).unwrap();
    edit.cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(5),
        )));
    edit.store(&ctx, id);
    let _ = ctx.run(
        egui::RawInput {
            modifiers: egui::Modifiers::SHIFT,
            events: vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::SHIFT,
            }],
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.composer(ui, ctx));
        },
    );
    assert_eq!(app.draft, "draft\n");
    assert!(app.deliveries.is_empty());
}
