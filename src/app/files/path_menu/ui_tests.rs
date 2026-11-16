use super::*;
fn frame(ctx: &egui::Context, browser: &Browser, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(420.0, 500.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            inputs(ctx, &browser.root, true);
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = ui.add(egui::Button::new("שלום.csv"));
                let rect = response.rect;
                ctx.data_mut(|data| data.insert_temp(egui::Id::new("path-menu-row"), rect));
                browser.path_context_menu(&response, &browser.root.join("שלום.csv"));
            });
        },
    )
}
#[test]
fn relative_root_full_path_action_is_disabled_and_cannot_copy() {
    let ctx = egui::Context::default();
    let mut browser = Browser::default();
    browser.root = PathBuf::from("relative-root");
    frame(&ctx, &browser, vec![]);
    let pos = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("path-menu-row")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        frame(
            &ctx,
            &browser,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    for _ in 0..2 {
        frame(&ctx, &browser, vec![]);
    }
    let id = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("explorer-copy-path", 1))))
        .unwrap();
    assert!(!ctx.read_response(id).unwrap().enabled());
    ctx.memory_mut(|memory| memory.request_focus(id));
    let output = frame(
        &ctx,
        &browser,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(
        output
            .platform_output
            .commands
            .iter()
            .all(|command| !matches!(command, egui::OutputCommand::CopyText(_)))
    );
}
#[test]
fn foreign_popup_prevents_new_path_menu_claim() {
    let ctx = egui::Context::default();
    let mut browser = Browser::default();
    browser.root = PathBuf::from("/workspace");
    frame(&ctx, &browser, vec![]);
    let pos = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("path-menu-row")))
        .unwrap()
        .center();
    let foreign = egui::Id::new("foreign-path-owner");
    for pressed in [true, false] {
        egui::Popup::open_id(&ctx, foreign);
        let output = frame(
            &ctx,
            &browser,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(output.platform_output.commands.is_empty());
        assert!(!owns(&ctx));
        assert!(
            ctx.data(|data| data.get_temp::<OpenMenu>(egui::Id::new(OWNER)))
                .is_none()
        );
    }
}
#[test]
fn owned_menu_escape_and_paired_text_are_consumed_but_foreign_input_is_not() {
    let ctx = egui::Context::default();
    let popup = egui::Id::new("paired-path-input");
    egui::Popup::open_id(&ctx, popup);
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new(OWNER),
            OpenMenu {
                root: PathBuf::from("/workspace"),
                popup,
            },
        )
    });
    let _ = ctx.run(
        egui::RawInput {
            events: vec![
                egui::Event::Text(" ".into()),
                egui::Event::Paste("never paste".into()),
                egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ctx| {
            inputs(ctx, Path::new("/workspace"), true);
            assert!(!egui::Popup::is_id_open(ctx, popup));
            assert!(owns(ctx));
            assert!(ctx.input(|input| input.events.is_empty()));
        },
    );
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Text("kept".into())],
            ..Default::default()
        },
        |ctx| {
            inputs(ctx, Path::new("/workspace"), true);
            assert!(!owns(ctx));
            assert!(ctx.input(|input| {
                input
                    .events
                    .iter()
                    .any(|event| matches!(event,egui::Event::Text(text)if text=="kept"))
            }));
        },
    );
}
#[test]
fn outside_primary_dismissal_never_copies_and_quarantines_chat_chords() {
    let ctx = egui::Context::default();
    let mut browser = Browser::default();
    browser.root = PathBuf::from("/workspace");
    frame(&ctx, &browser, vec![]);
    let pos = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("path-menu-row")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        frame(
            &ctx,
            &browser,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(egui::Popup::is_any_open(&ctx));
    for pressed in [true, false] {
        let output = frame(
            &ctx,
            &browser,
            vec![
                egui::Event::PointerMoved(egui::pos2(400.0, 450.0)),
                egui::Event::PointerButton {
                    pos: egui::pos2(400.0, 450.0),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
            ],
        );
        assert!(
            output
                .platform_output
                .commands
                .iter()
                .all(|command| !matches!(command, egui::OutputCommand::CopyText(_)))
        );
        assert!(!ctx.input(|input| input.key_pressed(egui::Key::Enter)));
    }
    assert!(!egui::Popup::is_any_open(&ctx));
}
#[test]
fn arriving_modal_closes_owned_menu_without_consuming_modal_chords() {
    let ctx = egui::Context::default();
    let popup = egui::Id::new("stale-path-before-modal");
    egui::Popup::open_id(&ctx, popup);
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new(OWNER),
            OpenMenu {
                root: PathBuf::from("/workspace"),
                popup,
            },
        )
    });
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        prompt_ui::modal(ctx, egui::Id::new("new-foreign-path-modal"), 220.0, |ui| {
            prompt_ui::heading(ui, "Information", "Foreign owner")
        });
    });
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            ..Default::default()
        },
        |ctx| {
            inputs(ctx, Path::new("/workspace"), true);
            assert!(!egui::Popup::is_id_open(ctx, popup));
            assert!(ctx.input(|input| input.key_pressed(egui::Key::Enter)));
            assert!(!owns(ctx));
        },
    );
}
#[test]
fn exact_row_opening_requires_previous_pass_root_layer_and_hit() {
    for stale in [false, true] {
        let ctx = egui::Context::default();
        let mut browser = Browser::default();
        browser.root = PathBuf::from("/workspace");
        frame(
            &ctx,
            &browser,
            vec![egui::Event::PointerMoved(egui::pos2(20.0, 20.0))],
        );
        frame(
            &ctx,
            &browser,
            vec![egui::Event::PointerMoved(egui::pos2(20.0, 20.0))],
        );
        if stale {
            browser.root = PathBuf::from("/other");
        }
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::PointerButton {
                    pos: egui::pos2(20.0, 20.0),
                    button: egui::PointerButton::Secondary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| {
                inputs(ctx, &browser.root, true);
                assert_eq!(owns(ctx), !stale);
            },
        );
    }
}
#[test]
fn stale_workspace_or_hidden_menu_closes_with_dismissal_chords_quarantined() {
    for hidden in [false, true] {
        let ctx = egui::Context::default();
        let popup = egui::Id::new("owned-path-fixture");
        egui::Popup::open_id(&ctx, popup);
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new(OWNER),
                OpenMenu {
                    root: PathBuf::from("/old"),
                    popup,
                },
            )
        });
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                }],
                ..Default::default()
            },
            |ctx| {
                inputs(
                    ctx,
                    Path::new(if hidden { "/old" } else { "/new" }),
                    !hidden,
                );
                assert!(owns(ctx));
                assert!(!ctx.input(|input| input.key_pressed(egui::Key::Enter)));
                assert!(!egui::Popup::is_id_open(ctx, popup));
            },
        );
    }
}
#[test]
fn native_context_pointer_then_keyboard_copy_is_exact_and_close_is_explicit() {
    for dark in [false, true] {
        for (index, expected) in [(0, "שלום.csv"), (1, "/workspace/שלום.csv")] {
            let ctx = egui::Context::default();
            ctx.set_zoom_factor(if index == 0 { 0.8 } else { 1.4 });
            if dark {
                ctx.set_visuals(egui::Visuals::dark());
            }
            let mut browser = Browser::default();
            browser.root = PathBuf::from("/workspace");
            frame(&ctx, &browser, vec![]);
            let pos = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("path-menu-row")))
                .unwrap()
                .center();
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &browser,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Secondary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert!(egui::Popup::is_any_open(&ctx));
            for _ in 0..2 {
                frame(&ctx, &browser, vec![]);
            }
            let id = ctx
                .data(|data| {
                    data.get_temp::<egui::Id>(egui::Id::new(("explorer-copy-path", index)))
                })
                .unwrap();
            ctx.memory_mut(|memory| memory.request_focus(id));
            let output = if index == 0 {
                frame(
                    &ctx,
                    &browser,
                    vec![egui::Event::Key {
                        key: egui::Key::Enter,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                )
            } else {
                let pos = ctx.read_response(id).unwrap().interact_rect.center();
                frame(
                    &ctx,
                    &browser,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
                frame(
                    &ctx,
                    &browser,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                )
            };
            assert!(output.platform_output.commands.iter().any(
                |command| matches!(command,egui::OutputCommand::CopyText(text)if text==expected)
            ));
            assert!(!egui::Popup::is_any_open(&ctx));
        }
    }
}
