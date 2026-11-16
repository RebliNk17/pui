use super::*;

#[test]
fn long_activity_culls_cached_offscreen_messages_and_keeps_search_target_visible() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo_task();
    app.tasks
        .histories
        .get_mut("demo-Builder")
        .unwrap()
        .messages = (0..300)
        .map(|i| Message {
            role: "assistant".into(),
            text: format!("{i} {}", "Builder activity ".repeat(25)),
            ..Default::default()
        })
        .collect();
    let task = app.tasks.current().unwrap().clone();
    app.tasks.selected_run = Some("demo-Builder".into());
    let mut view = View::default();
    let render = |app: &mut Pui, view: &mut View| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(700.0, 600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| app.work_item_activity(ui, &task, view));
                });
            },
        )
    };
    render(&mut app, &mut view);
    render(&mut app, &mut view);
    assert!(
        view.activity_rendered < 25,
        "must cull long history: {}",
        view.activity_rendered
    );
    app.tasks.activity_target = Some(("demo-Builder".into(), 250));
    render(&mut app, &mut view);
    assert!(view.activity_heights.contains_key(&250));
    assert!(app.tasks.activity_target.is_none());
}

#[test]
fn segmented_tabs_are_bounded_and_keyboard_navigation_wraps() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        mode.apply(&ctx);
        for width in [240.0, 350.0, 736.0] {
            let mut selected = Tab::Overview;
            let render = |events, selected: &mut Tab| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width + 16.0, 250.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            tabs(ui, "fixture", selected);
                        });
                    },
                )
            };
            for _ in 0..3 {
                render(vec![], &mut selected);
            }
            let rect = ctx
                .data(|d| {
                    d.get_temp::<egui::Rect>(egui::Id::new(("work-item-tab-rect", "Evidence")))
                })
                .unwrap();
            assert!(rect.width() <= width / 4.0);
            for pressed in [true, false] {
                render(
                    vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::PointerButton {
                            pos: rect.center(),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    &mut selected,
                );
            }
            assert_eq!(selected, Tab::Evidence);
            for (key, expected) in [
                (egui::Key::ArrowRight, Tab::Changes),
                (egui::Key::End, Tab::Checkpoints),
                (egui::Key::ArrowRight, Tab::Overview),
                (egui::Key::ArrowLeft, Tab::Checkpoints),
                (egui::Key::Home, Tab::Overview),
            ] {
                render(
                    vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    &mut selected,
                );
                assert_eq!(selected, expected, "{mode:?}/{width}/{key:?}");
                render(vec![], &mut selected);
            }
        }
    }
}
