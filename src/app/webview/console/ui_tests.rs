use super::*;

#[test]
fn console_actions_fit_narrow_themes_zoom_and_reserved_native_page_rect() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let key = "http://localhost/fixture";
            let mut state = PageState {
                location: key.into(),
                ..Default::default()
            };
            push(
                &mut state,
                true,
                &"long literal message 界 ".repeat(40),
                "http://localhost/a.js",
                8,
            );
            let mut browser = BrowserView::default();
            browser.options.insert(
                key.into(),
                Options {
                    console: true,
                    ..Default::default()
                },
            );
            let mut time = 0.0;
            let mut native = egui::Rect::NOTHING;
            let mut draw = |events| {
                time += 0.016;
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(390.0, 650.0),
                        )),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            native = browser.viewport_with_state(ui, key, state.clone())
                        });
                    },
                )
            };
            for _ in 0..100 {
                draw(vec![]);
            }
            let action = super::tests::button(&ctx, key, &state.console[0]);
            assert!(
                ctx.screen_rect().contains_rect(action),
                "{action:?} at {zoom}"
            );
            let output = draw(vec![]);
            assert!(
                native.bottom() < action.top(),
                "Native page may not cover Add: {native:?} {action:?} zoom={zoom}"
            );
            assert!(
                output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                    > Duration::from_secs(1)
            );
        }
    }
}

#[test]
fn empty_hidden_and_cleared_consoles_offer_no_capture_and_stale_press_is_ignored() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let key = "http://localhost/fixture";
    let mut browser = BrowserView::default();
    browser.options.insert(
        key.into(),
        Options {
            console: true,
            ..Default::default()
        },
    );
    let mut state = PageState::default();
    for _ in 0..3 {
        super::tests::frame(&ctx, &state, &mut browser, vec![], key);
    }
    assert!(
        ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new((
            "browser-control",
            "Add console message to chat"
        ))))
        .is_none()
    );
    push(&mut state, false, "available", "a.js", 1);
    for _ in 0..3 {
        super::tests::frame(&ctx, &state, &mut browser, vec![], key);
    }
    let target = super::tests::button(&ctx, key, &state.console[0]).center();
    super::tests::frame(
        &ctx,
        &state,
        &mut browser,
        super::tests::pointer(target, true),
        key,
    );
    state.console.clear();
    super::tests::frame(
        &ctx,
        &state,
        &mut browser,
        super::tests::pointer(target, false),
        key,
    );
    let mut app = crate::app::files::tests::app(&ctx);
    app.draft = "Kept".into();
    app.context_chips.clear_quotes();
    app.collect_context_capture();
    assert_eq!(app.outgoing_text(), "Kept");
    browser.options.get_mut(key).unwrap().console = false;
    super::tests::frame(&ctx, &state, &mut browser, vec![], key);
    app.collect_context_capture();
    assert_eq!(app.outgoing_text(), "Kept");
}

#[test]
fn draft_limit_rejection_keeps_existing_text_and_console_capture_stays_local() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    let line = ConsoleLine::new(true, "Literal error text", "a.js", 3);
    let (label, text) = line.snippet("http://localhost/");
    app.draft = "x".repeat(256 * 1024);
    app.context_chips.clear_quotes();
    let draft = app.outgoing_text().clone();
    app.state.busy = true;
    context_capture::queue(&ctx, &label, &text);
    app.collect_context_capture();
    assert_eq!(app.outgoing_text(), draft);
    assert!(app.state.busy);
    assert!(app.state.error.as_deref().unwrap().contains("too large"));
    assert!(line.text.contains("Literal"));
}
