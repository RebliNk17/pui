use super::*;
#[test]
fn complete_error_batch_preserves_order_provenance_and_whole_refuses_overflow() {
    let mut state = PageState {
        location: "https://example.test/original".into(),
        ..Default::default()
    };
    push(&mut state, false, "not included", "a.js", 1);
    push(&mut state, true, "first 世界", "a.js", 2);
    push(&mut state, true, "second =1+2", "data:text/html,secret", 3);
    let text = collect("page", &state.console).unwrap();
    assert!(!text.contains("not included"));
    assert!(text.find("first 世界").unwrap() < text.find("second =1+2").unwrap());
    assert!(text.contains("https://example.test/original"));
    assert!(text.contains("Source: a.js:2"));
    assert!(!text.contains("data:text/html"));
    for _ in 0..10 {
        push(&mut state, true, &"x".repeat(4000), "a", 1);
    }
    assert!(collect("page", &state.console).is_err());
    assert!(collect("page", &std::collections::VecDeque::new()).is_err());
}
#[test]
fn native_batch_pointer_appends_all_errors_once_without_send() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut state = PageState::default();
    push(&mut state, true, "first retained error", "a", 1);
    push(&mut state, true, "second retained error", "b", 2);
    let draw = |events| {
        ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| show(ui, "page", &state.console));
            },
        )
    };
    for _ in 0..3 {
        draw(vec![]);
    }
    let (_, rect) = ctx
        .data(|d| d.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("console-error-batch-button")))
        .unwrap();
    for pressed in [true, false] {
        draw(vec![
            egui::Event::PointerMoved(rect.center()),
            egui::Event::PointerButton {
                pos: rect.center(),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
    }
    let mut app = crate::app::files::tests::app(&ctx);
    app.draft = "Keep".into();
    app.state.busy = true;
    app.collect_context_capture();
    assert!(app.draft.contains("first retained error"));
    assert!(app.draft.contains("second retained error"));
    let draft = app.draft.clone();
    app.collect_context_capture();
    assert_eq!(draft, app.draft);
    assert!(app.state.busy);
}
#[test]
fn batch_control_narrow_themes_zoom_does_not_queue_without_activation() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut state = PageState::default();
            push(&mut state, true, "test error", "script", 1);
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(420.0, 620.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| show(ui, "page", &state.console));
                },
            );
            assert!(output.platform_output.commands.is_empty());
            let mut app = crate::app::files::tests::app(&ctx);
            app.draft = "Keep".into();
            app.collect_context_capture();
            assert_eq!(app.draft, "Keep");
        }
    }
}
