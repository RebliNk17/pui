use super::*;

fn label_rect(shape: &egui::Shape, label: &str) -> Option<Rect> {
    match shape {
        egui::Shape::Text(text) if text.galley.job.text == label => {
            Some(Rect::from_min_size(text.pos, text.galley.size()))
        }
        egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| label_rect(shape, label)),
        _ => None,
    }
}

#[test]
fn continue_stays_right_of_wrapped_error_and_only_clicks_when_enabled() {
    let error = format!(
        "Request timed out: {}:443 (UND_ERR_CONNECT_TIMEOUT)",
        "host".repeat(45)
    );
    for theme in [crate::theme::Theme::Light, crate::theme::Theme::Dark] {
        for width in [240.0, 736.0] {
            for zoom in [1.0, 1.5] {
                for enabled in [Some(true), Some(false), None] {
                    let ctx = egui::Context::default();
                    crate::theme::install(&ctx);
                    theme.apply(&ctx);
                    ctx.set_zoom_factor(zoom);
                    let mut time = 0.0;
                    let mut frame = |events| {
                        time += 0.1;
                        let mut clicked = false;
                        let mut bounds = Rect::NOTHING;
                        let output = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    Vec2::new(width, 1000.0),
                                )),
                                time: Some(time),
                                events,
                                ..Default::default()
                            },
                            |ctx| {
                                egui::CentralPanel::default().show(ctx, |ui| {
                                    bounds = ui.max_rect();
                                    clicked = row(ui, &error, enabled);
                                });
                            },
                        );
                        let button = output
                            .shapes
                            .iter()
                            .find_map(|shape| label_rect(&shape.shape, "Continue"));
                        let error = output
                            .shapes
                            .iter()
                            .find_map(|shape| label_rect(&shape.shape, &error))
                            .unwrap();
                        (clicked, button, error, bounds)
                    };
                    for _ in 0..3 {
                        assert!(!frame(vec![]).0);
                    }
                    let (_, button, error, bounds) = frame(vec![]);
                    assert!(
                        bounds.contains_rect(error),
                        "{theme:?}/{width}/{zoom}: {error:?} outside {bounds:?}"
                    );
                    if let Some(button) = button {
                        assert!(bounds.contains_rect(button));
                        assert!(error.right() < button.left());
                        assert!(bounds.right() - button.right() < 16.0);
                        assert!(error.height() > button.height(), "long address wraps");
                        let pos = button.center();
                        assert!(
                            !frame(vec![
                                egui::Event::PointerMoved(pos),
                                egui::Event::PointerButton {
                                    pos,
                                    button: egui::PointerButton::Primary,
                                    pressed: true,
                                    modifiers: egui::Modifiers::NONE,
                                }
                            ])
                            .0
                        );
                        assert_eq!(
                            frame(vec![egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed: false,
                                modifiers: egui::Modifiers::NONE,
                            }])
                            .0,
                            enabled == Some(true)
                        );
                        assert!(!frame(vec![]).0, "a click is emitted only once");
                        assert!(
                            !frame(vec![egui::Event::Key {
                                key: egui::Key::Escape,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers: egui::Modifiers::NONE,
                            }])
                            .0
                        );
                    } else {
                        assert!(enabled.is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn transcript_only_offers_continue_on_latest_settled_error() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut transcript = Transcript {
        continue_enabled: true,
        ..Default::default()
    };
    let mut messages = vec![Message {
        role: "assistant".into(),
        error: Some("Final error".into()),
        ..Default::default()
    }];
    let mut frame = |messages: &[Message], running, events| {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 1000.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    transcript.show(ui, messages, "model", "high", running, "chat");
                });
            },
        );
        (
            output
                .shapes
                .iter()
                .find_map(|shape| label_rect(&shape.shape, "Continue")),
            transcript.action.take(),
        )
    };
    for _ in 0..3 {
        frame(&messages, false, vec![]);
    }
    let pos = frame(&messages, false, vec![]).0.unwrap().center();
    assert!(
        frame(
            &messages,
            false,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }
            ]
        )
        .1
        .is_none()
    );
    assert_eq!(
        frame(
            &messages,
            false,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }]
        )
        .1,
        Some(TranscriptAction::Continue(0))
    );
    assert!(
        frame(&messages, true, vec![]).0.is_none(),
        "no action during automatic retry"
    );
    messages.push(Message {
        role: "user".into(),
        text: "A newer request".into(),
        ..Default::default()
    });
    assert!(
        frame(&messages, false, vec![]).0.is_none(),
        "old errors cannot continue a newer turn"
    );
    messages.push(Message {
        role: "assistant".into(),
        text: "Recovered".into(),
        ..Default::default()
    });
    assert!(frame(&messages, false, vec![]).0.is_none());
}
