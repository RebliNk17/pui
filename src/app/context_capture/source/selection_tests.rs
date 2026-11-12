use super::tests::{pointer, pos, select};
use super::*;

#[test]
fn cross_widget_and_layer_selections_are_not_exposed_as_one_source_range() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut responses = Vec::new();
    let mut draw = |events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(500.0, 500.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                responses.clear();
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.style_mut().interaction.multi_widget_text_select = true;
                    responses.push(ui.add(egui::Label::new("first source").selectable(true)));
                    responses.push(ui.add(egui::Label::new("second source").selectable(true)));
                });
            },
        )
    };
    for _ in 0..3 {
        draw(vec![]);
    }
    let output = draw(vec![]);
    let start = pos(&output, "first source", 2);
    let end = pos(&output, "second source", 6);
    draw(pointer(start, egui::PointerButton::Primary, true));
    draw(vec![egui::Event::PointerMoved(end)]);
    draw(pointer(end, egui::PointerButton::Primary, false));
    let selection = egui::text_selection::LabelSelectionState::load(&ctx);
    assert!(selection.has_selection());
    assert!(
        responses
            .iter()
            .all(|response| selection.range_for_response(response).is_none())
    );
    let response = select(&ctx, "same source", 0..4);
    let selection = egui::text_selection::LabelSelectionState::load(&ctx);
    assert!(selection.range_for_response(&response).is_some());
    let mut another_layer = response.clone();
    another_layer.layer_id =
        egui::LayerId::new(egui::Order::Foreground, egui::Id::new("not-source-layer"));
    assert!(selection.range_for_response(&another_layer).is_none());
}

#[test]
fn source_menu_freezes_and_fits_themes_zoom_without_repeated_repaint() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let source = "prefix chosen suffix";
            select(&ctx, source, 7..13);
            let mut time = ctx.input(|input| input.time);
            let mut draw = |events| {
                time += 0.016;
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(390.0, 500.0),
                        )),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        inputs(ctx);
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.push_id("source-file.rs", |ui| {
                                let response = ui.add(
                                    egui::Label::new(RichText::new(source).monospace().size(14.0))
                                        .extend()
                                        .selectable(true),
                                );
                                offer(&response, Path::new("source.rs"), source, 1);
                            });
                        });
                    },
                )
            };
            for _ in 0..3 {
                draw(vec![]);
            }
            let target = pos(&draw(vec![]), source, 9);
            for pressed in [true, false] {
                draw(pointer(target, egui::PointerButton::Secondary, pressed));
            }
            for _ in 0..100 {
                draw(vec![]);
            }
            let menu = ctx.data(|data| data.get_temp::<SourceMenu>(key())).unwrap();
            let rect = ctx.memory(|memory| memory.area_rect(menu.owner)).unwrap();
            assert!(ctx.screen_rect().contains_rect(rect));
            assert_eq!(menu.selected.as_ref().unwrap().text, "chosen");
            let output = draw(vec![]);
            assert!(
                output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                    > Duration::from_secs(1)
            );
            // Replace the rendered source but preserve the open snapshot until explicit action.
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.push_id("source-file.rs", |ui| {
                        let response = ui.add(egui::Label::new("changed source").selectable(true));
                        offer(&response, Path::new("source.rs"), "changed source", 1);
                    });
                });
            });
            assert_eq!(
                ctx.data(|data| data.get_temp::<SourceMenu>(key()))
                    .unwrap()
                    .selected
                    .as_ref()
                    .unwrap()
                    .text,
                "chosen"
            );
        }
    }
}

#[test]
fn same_label_copy_survives_the_secondary_click_fix() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let source = "copy selected text";
    select(&ctx, source, 5..13);
    let output = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Copy],
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.push_id("source-file.rs", |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(source).monospace().size(14.0))
                            .extend()
                            .selectable(true),
                    );
                });
            });
        },
    );
    assert!(output.platform_output.commands.iter().any(
        |command| matches!(command, egui::OutputCommand::CopyText(text) if text == "selected")
    ));
}
