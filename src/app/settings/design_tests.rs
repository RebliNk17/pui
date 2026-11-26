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

#[test]
fn permissions_sandbox_toggles_render_enforce_dependency_and_propagate() {
    fn toggle_state(ctx: &egui::Context, title: &str) -> (egui::Rect, bool) {
        ctx.data(|d| {
            d.get_temp::<(egui::Rect, bool)>(egui::Id::new(("settings-toggle-rect", title)))
                .unwrap_or_else(|| panic!("missing toggle rect for {title}"))
        })
    }

    for mode in [theme::Theme::Dark, theme::Theme::Light] {
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
        app.demo = false;
        let (client, _tx, commands) = rpc::Client::stub_with_commands();
        app.client = Some(client);
        app.settings_view.page = Page::Permissions;
        app.prefs.settings.local_sandbox = false;
        app.prefs.settings.strict_sandbox = false;

        let run_frame = |app: &mut Pui, events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1100.0, 1100.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| app.session_settings(ctx),
            )
        };
        let click = |app: &mut Pui, pos: egui::Pos2| {
            for pressed in [true, false] {
                let _ = run_frame(
                    app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        };

        let _ = run_frame(&mut app, vec![]);
        let _ = run_frame(&mut app, vec![]);
        let (local_rect, local_enabled) = toggle_state(&ctx, "Local sandbox");
        let (strict_rect, strict_enabled) = toggle_state(&ctx, "Strict sandbox mode");

        assert_eq!(local_enabled, crate::sandbox::is_supported());
        assert!(
            !strict_enabled,
            "Strict sandbox must be disabled when Local sandbox is off"
        );

        // Clicking disabled Strict toggle must not enable it.
        click(&mut app, strict_rect.center());
        assert!(!app.prefs.settings.strict_sandbox);

        if crate::sandbox::is_supported() {
            // Enable Local sandbox and verify /pui-settings propagation.
            click(&mut app, local_rect.center());
            assert!(app.prefs.settings.local_sandbox);
            assert!(!app.prefs.settings.strict_sandbox);
            let sent: Vec<_> = commands.try_iter().collect();
            assert!(
                sent.iter().any(|cmd| {
                    cmd["message"].as_str().is_some_and(|m| {
                        m.starts_with("/pui-settings ") && m.contains("\"localSandbox\":true")
                    })
                }),
                "expected /pui-settings with localSandbox:true, got: {sent:?}"
            );

            let _ = run_frame(&mut app, vec![]);
            let (strict_rect, strict_enabled) = toggle_state(&ctx, "Strict sandbox mode");
            assert!(
                strict_enabled,
                "Strict sandbox must unlock when Local sandbox is on"
            );

            // Enable Strict sandbox mode.
            click(&mut app, strict_rect.center());
            assert!(app.prefs.settings.local_sandbox);
            assert!(app.prefs.settings.strict_sandbox);
            let sent: Vec<_> = commands.try_iter().collect();
            assert!(
                sent.iter().any(|cmd| {
                    cmd["message"].as_str().is_some_and(|m| {
                        m.starts_with("/pui-settings ") && m.contains("\"strictSandbox\":true")
                    })
                }),
                "expected /pui-settings with strictSandbox:true, got: {sent:?}"
            );

            // Disabling Local sandbox must also clear Strict sandbox mode.
            click(&mut app, local_rect.center());
            assert!(!app.prefs.settings.local_sandbox);
            assert!(!app.prefs.settings.strict_sandbox);
        }
    }

    // Search entries include both sandbox toggles under Permissions.
    let entries = search_entries();
    assert!(
        entries
            .iter()
            .any(|e| e.title == "Local sandbox" && e.page == "Permissions")
    );
    assert!(
        entries
            .iter()
            .any(|e| e.title == "Strict sandbox mode" && e.page == "Permissions")
    );
}
