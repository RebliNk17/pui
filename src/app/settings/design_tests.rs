use super::*;

#[test]
fn app_controls_share_one_height_in_both_themes() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        mode.apply(&ctx);
        let mut controls = Vec::new();
        let mut compact = Vec::new();
        let mut input = String::new();
        let mut enabled = true;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                controls.push(("Tasks", ui.button("Tasks").rect));
                controls.push(("Refresh", ui.button("Refresh").rect));
                controls.push(("Config", ui.button("Personal config").rect));
                controls.push(("Tab", theme::choice(ui, true, "MCP servers  8").rect));
                controls.push((
                    "Search",
                    ui.add(theme::text_field(&mut input, "Search resources"))
                        .rect,
                ));
                controls.push(("Icon", icons::button(ui, Icon::Details, "Details").rect));
                controls.push((
                    "Dropdown",
                    egui::ComboBox::from_id_salt("height-test")
                        .selected_text("Manual")
                        .show_ui(ui, |_| {})
                        .response
                        .rect,
                ));
                controls.push(("Number", ui.add(egui::DragValue::new(&mut 4)).rect));
                controls.push(("Navigation", navigation(ui, Page::General, true).rect));
                let toggle_row = ui.horizontal(|ui| toggle(ui, &mut enabled, "Toggle"));
                controls.push(("Toggle", toggle_row.response.rect));
                ui.scope(|ui| {
                    theme::compact_controls(ui);
                    compact.push(icons::button(ui, Icon::Plus, "Attach").rect.height());
                });
                compact.push(
                    icons::labeled_button(ui, Icon::Copy, "Copy", true)
                        .rect
                        .height(),
                );
                compact.push(
                    ui.add(egui::Button::new("Show 18 more").small().frame(false))
                        .rect
                        .height(),
                );
            });
        });
        for (label, rect) in controls {
            assert!(
                (rect.height() - theme::CONTROL_HEIGHT).abs() < 0.1,
                "{label}: {rect:?}"
            );
        }
        assert!(
            compact.iter().all(|height| *height < theme::CONTROL_HEIGHT),
            "Chat and Show more must remain compact: {compact:?}"
        );
    }
}

#[test]
fn every_settings_page_stays_within_the_viewport_and_keeps_work_intact() {
    fn text(shape: &egui::Shape, output: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => output.push((
                t.galley.job.text.clone(),
                t.galley.rect.translate(t.pos.to_vec2()),
            )),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| text(s, output)),
            _ => {}
        }
    }
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        for (width, zoom) in [(860.0, 1.0), (1480.0, 1.0), (1200.0, 1.4)] {
            let ctx = egui::Context::default();
            let mut app = Pui::new(
                &eframe::CreationContext::_new_kittest(ctx.clone()),
                Options {
                    demo: true,
                    demo_settings: true,
                    theme: Some(mode),
                    ..Default::default()
                },
            );
            ctx.set_zoom_factor(zoom);
            app.state.busy = true;
            app.draft = "Preserve this draft".into();
            for page in Page::ALL {
                app.settings_view.page = page;
                let mut last = None;
                for _ in 0..4 {
                    last = Some(ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(width, 960.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| app.session_settings(ctx),
                    ));
                }
                let mut labels = Vec::new();
                for shape in &last.unwrap().shapes {
                    text(&shape.shape, &mut labels);
                }
                let title = labels
                    .iter()
                    .find(|(s, r)| s == page.title() && r.left() > 244.0)
                    .expect("Settings page title")
                    .1;
                assert!(title.left() >= 268.0);
                for (label, rect) in &labels {
                    assert!(
                        rect.right() <= ctx.screen_rect().right() + 1.0,
                        "{page:?} at {width}/{zoom}: {label}: {rect:?}"
                    );
                }
                assert!(app.state.busy);
                assert_eq!(app.draft, "Preserve this draft");
            }
        }
    }
}
