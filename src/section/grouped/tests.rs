use super::*;

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn grouped_rows_are_single_line_outlined_and_pointer_keyboard_operable() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [160.0, 300.0, 700.0] {
            for zoom in [0.8, 1.4] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let mut rect = Rect::NOTHING;
                let mut render = |events, enabled| {
                    let mut shown = false;
                    let out = ctx.run(
                        egui::RawInput {
                            events,
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(width + 40.0, 400.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                ui.set_width(width);
                                ui.add_enabled_ui(enabled, |ui| {
                                    let r = Section::new("Usage details")
                                        .grouped()
                                        .summary("353k · session")
                                        .show(ui, |ui| {
                                            shown = true;
                                            ui.label("Actual retained details");
                                        });
                                    rect = r.rect;
                                });
                            });
                        },
                    );
                    (out, rect, shown)
                };
                for _ in 0..3 {
                    render(vec![], true);
                }
                let (out, rect, shown) = render(vec![], true);
                assert!(!shown);
                assert_eq!(rect.height(), 32.0);
                assert!(out.shapes.iter().any(|shape| matches!(
                    &shape.shape,
                    egui::Shape::Rect(r) if r.stroke.width == 1.0 && r.rect.contains_rect(rect)
                )));
                assert!(
                    !out.shapes.iter().any(|shape| matches!(
                        &shape.shape,
                        egui::Shape::Rect(r) if r.rect.width() <= 30.0 && r.rect.height() >= 20.0
                    )),
                    "no boxed icon or plus button"
                );
                let pos = rect.center();
                for pressed in [true, false] {
                    render(
                        vec![
                            egui::Event::PointerMoved(pos),
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                        true,
                    );
                }
                assert!(render(vec![], true).2);
                assert!(!render(vec![key(egui::Key::ArrowLeft)], true).2);
                assert!(render(vec![key(egui::Key::ArrowRight)], true).2);
                assert!(!render(vec![key(egui::Key::Space)], true).2);
                assert!(render(vec![key(egui::Key::Enter)], true).2);
                // Disabled rows must not toggle through retained focus.
                assert!(render(vec![key(egui::Key::ArrowLeft)], false).2);
                let out = render(vec![], true).0;
                for shape in out.shapes {
                    if let egui::Shape::Text(text) = shape.shape {
                        assert!(
                            text.pos.x + text.galley.size().x <= width + 20.0,
                            "text overflows"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn grouped_disclosure_reuses_existing_open_state_and_is_independent() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let id = ui.make_persistent_id(egui::Id::new("retained-thinking"));
            let mut state =
                egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, true);
            state.set_open(true);
            state.store(ctx);
            let mut retained = false;
            Section::new("Thinking")
                .grouped()
                .id_salt("retained-thinking")
                .show(ui, |_| retained = true);
            assert!(retained);
            Section::new("Thinking")
                .grouped()
                .id_salt("other-thinking")
                .show(ui, |_| panic!("sibling remains collapsed"));
        });
    });
}

#[test]
fn grouped_thinking_has_no_subtitle_and_does_not_execute_closed_body() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    for _ in 0..3 {
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                Section::new("Thinking")
                    .grouped()
                    .icon(Icon::Spark)
                    .show(ui, |_| panic!("closed body must not run"));
            });
        });
        assert!(!out.shapes.iter().any(
            |s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text=="Model reasoning")
        ));
    }
}
