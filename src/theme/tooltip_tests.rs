use super::*;

fn text_shapes<'a>(shape: &'a egui::Shape, out: &mut Vec<&'a egui::epaint::TextShape>) {
    match shape {
        egui::Shape::Text(text) => out.push(text),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| text_shapes(shape, out)),
        _ => {}
    }
}

#[test]
fn tooltip_font_is_compact_scoped_and_works_for_automatic_elision() {
    for mode in [Theme::Light, Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            assert_eq!(ctx.style().interaction.tooltip_delay, 0.3);
            assert_eq!(ctx.style().interaction.tooltip_grace_time, 0.15);
            assert!(ctx.style().interaction.show_tooltips_only_when_still);
            assert!(ctx.style().override_font_id.is_none());
            assert_eq!(ctx.style().text_styles[&TextStyle::Body].size, 14.0);
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let response = ui.label("Explicit tooltip anchor");
                    egui::Tooltip::for_widget(&response).show(|ui| {
                        assert_eq!(ui.style().override_font_id.as_ref().unwrap().size, 11.0);
                        ui.label("Compact tooltip");
                    });
                });
            });
            let mut time = 1.0;
            let mut frame = |pointer: Option<egui::Pos2>| {
                time += 0.5;
                let mut rect = egui::Rect::NOTHING;
                let output = ctx.run(
                    egui::RawInput {
                        time: Some(time),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(500.0, 400.0),
                        )),
                        events: pointer.map(egui::Event::PointerMoved).into_iter().collect(),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.label("Ordinary body");
                            rect = ui
                                .add_sized(
                                    [90.0, 24.0],
                                    egui::Label::new(
                                        egui::RichText::new(
                                            "Long label whose hidden ending must be revealed",
                                        )
                                        .size(18.0),
                                    )
                                    .truncate(),
                                )
                                .rect;
                        });
                    },
                );
                (output, rect)
            };
            frame(None);
            let (_, rect) = frame(None);
            frame(Some(rect.center()));
            frame(None);
            let (output, _) = frame(None);
            let mut text = vec![];
            output
                .shapes
                .iter()
                .for_each(|shape| text_shapes(&shape.shape, &mut text));
            let body = text
                .iter()
                .find(|t| t.galley.job.text == "Ordinary body")
                .unwrap();
            assert!(
                body.galley
                    .job
                    .sections
                    .iter()
                    .all(|s| s.format.font_id.size == 14.0)
            );
            let full = text
                .iter()
                .find(|t| {
                    t.galley.job.text == "Long label whose hidden ending must be revealed"
                        && !t.galley.elided
                })
                .expect("Elided labels reveal their full text");
            assert!(
                full.galley
                    .job
                    .sections
                    .iter()
                    .all(|s| s.format.font_id.size == 11.0)
            );
            assert!(ctx.style().override_font_id.is_none());
        }
    }
}

#[test]
fn icon_tooltip_waits_for_short_delay_and_does_not_show_after_click() {
    let ctx = egui::Context::default();
    install(&ctx);
    let frame = |time, events| {
        let mut rect = egui::Rect::NOTHING;
        let mut tooltip = false;
        let _ = ctx.run(
            egui::RawInput {
                time: Some(time),
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 400.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let response = crate::icons::button(
                        ui,
                        crate::icons::Icon::Folder,
                        "Browse workspace files",
                    );
                    rect = response.rect;
                    tooltip = egui::Tooltip::was_tooltip_open_last_frame(ctx, response.id);
                });
            },
        );
        (rect, tooltip)
    };
    frame(0.0, vec![]);
    let (rect, _) = frame(0.1, vec![]);
    let pos = rect.center();
    frame(0.2, vec![egui::Event::PointerMoved(pos)]);
    assert!(!frame(0.4, vec![]).1);
    frame(0.55, vec![]);
    assert!(frame(0.6, vec![]).1);
    for (time, pressed) in [(0.7, true), (0.75, false)] {
        frame(
            time,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
    frame(1.2, vec![]);
    assert!(!frame(1.3, vec![]).1);
}
