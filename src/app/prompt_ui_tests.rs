use super::*;

fn text_rects(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(text) => out.push((
            text.galley.job.text.clone(),
            egui::Rect::from_min_size(text.pos, text.galley.size()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                text_rects(shape, out);
            }
        }
        _ => {}
    }
}

#[test]
fn information_dialogs_fit_small_windows_and_escape_closes_without_work() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for size in [Vec2::new(420.0, 620.0), Vec2::new(860.0, 620.0)] {
            for zoom in [0.8, 1.0, 1.4] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let mut about_open = true;
                let mut updates = updates::Updates::default();
                updates.open = true;
                for about in [true, false] {
                    let render = |events, about_open: &mut bool, updates: &mut updates::Updates| {
                        ctx.run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    size,
                                )),
                                events,
                                ..Default::default()
                            },
                            |ctx| {
                                if about {
                                    super::about::show(ctx, about_open);
                                } else {
                                    updates.show(ctx);
                                }
                            },
                        )
                    };
                    for _ in 0..4 {
                        render(vec![], &mut about_open, &mut updates);
                    }
                    let output = render(vec![], &mut about_open, &mut updates);
                    let id = egui::Id::new(if about { "about-pui" } else { "pui-updates" });
                    let rect = ctx.read_response(id).expect("stable modal ID").rect;
                    assert!(
                        ctx.screen_rect().contains_rect(rect),
                        "{rect:?} outside {:?}",
                        ctx.screen_rect()
                    );
                    let mut labels = vec![];
                    for shape in output.shapes {
                        text_rects(&shape.shape, &mut labels);
                    }
                    let action = labels
                        .iter()
                        .find(|(text, _)| text == if about { "Done" } else { "Close" })
                        .unwrap();
                    assert!(ctx.screen_rect().contains_rect(action.1));
                    render(
                        vec![egui::Event::Key {
                            key: egui::Key::Escape,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        }],
                        &mut about_open,
                        &mut updates,
                    );
                    assert!(!(if about { about_open } else { updates.open }));
                }
            }
        }
    }
}

#[test]
fn information_modal_never_approves_the_agent_prompt_behind_it() {
    for key in [egui::Key::Enter, egui::Key::Backspace, egui::Key::Escape] {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        app.connected = true;
        app.client = Some(rpc::Client::stub());
        app.state = State {
            busy: true,
            ..Default::default()
        };
        app.state.dialogs.push_back(json!({"method":"confirm","id":"behind-modal","title":"Allow bash?","message":"Review the action.\n\n{\"command\":\"cargo test\"}\n\nApprove this call only."}));
        app.menu.about = true;
        let frame = |app: &mut Pui, events| {
            ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    app.conversation(ctx);
                    about::show(ctx, &mut app.menu.about);
                },
            )
        };
        for _ in 0..4 {
            frame(&mut app, vec![]);
        }
        frame(
            &mut app,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: if key == egui::Key::Escape {
                    egui::Modifiers::NONE
                } else {
                    egui::Modifiers::COMMAND
                },
            }],
        );
        assert_eq!(
            app.state.dialogs.len(),
            1,
            "{key:?} must not answer the covered approval"
        );
    }
}

#[test]
fn preview_flags_open_information_without_starting_a_real_session() {
    for about in [false, true] {
        let ctx = egui::Context::default();
        let app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx),
            Options {
                demo: true,
                demo_about: about,
                demo_updates: !about,
                ..Default::default()
            },
        );
        assert!(app.demo);
        assert!(app.client.is_none());
        assert_eq!(app.menu.about, about);
        assert_eq!(app.updates.open, !about);
    }
}
