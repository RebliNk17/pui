use super::*;
use crate::app::files::tests::frame;
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    }
}
#[test]
fn actual_explorer_menu_busy_chat_question_and_chords_never_send_stop_or_answer() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.busy = true;
    app.state.run_observed = true;
    app.draft = "kept draft".into();
    app.files.toggle(&ctx, &app.prefs.cwd);
    app.files.tree_open = true;
    for _ in 0..8 {
        frame(&ctx, &mut app, vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
    let path = app.files.root.join("README.md");
    // Discover the real row through its accessibility-independent render instrumentation.
    let pos = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("path-menu-test-row", &path))))
        .expect("README explorer row")
        .center();
    frame(&ctx, &mut app, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
                key(egui::Key::Enter),
                key(egui::Key::Backspace),
            ],
        );
    }
    assert!(egui::Popup::is_any_open(&ctx));
    app.state.dialogs.push_back(questions::demo_dialog());
    let button = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("explorer-copy-path", 0))))
        .unwrap();
    ctx.memory_mut(|memory| memory.request_focus(button));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter), key(egui::Key::Backspace)],
    );
    assert_eq!(app.draft, "kept draft");
    assert!(app.state.busy);
    assert_eq!(app.state.dialogs.len(), 1);
    let output = frame(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(
        output.platform_output.commands.iter().any(
            |command| matches!(command,egui::OutputCommand::CopyText(text)if text=="README.md")
        )
    );
    assert!(!egui::Popup::is_any_open(&ctx));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter), key(egui::Key::Backspace)],
    );
    assert_eq!(app.draft, "kept draft");
    assert!(app.state.busy);
    assert_eq!(app.state.dialogs.len(), 1);
    // Dismiss over the actual composer, including paired text and global chords.
    frame(&ctx, &mut app, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut app,
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
    let destination = ctx
        .read_response(egui::Id::new("composer"))
        .expect("visible composer")
        .interact_rect
        .center();
    for pressed in [true, false] {
        let output = frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(destination),
                egui::Event::PointerButton {
                    pos: destination,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::Text(" ".into()),
                egui::Event::Paste("must not enter draft".into()),
                key(egui::Key::Enter),
                key(egui::Key::Backspace),
            ],
        );
        assert!(
            output
                .platform_output
                .commands
                .iter()
                .all(|command| !matches!(command, egui::OutputCommand::CopyText(_)))
        );
    }
    assert!(!egui::Popup::is_any_open(&ctx));
    assert_eq!(app.draft, "kept draft");
    assert!(app.state.busy);
    assert_eq!(app.state.dialogs.len(), 1);
    // Reopen, then park the session layout: action state must not park with Files.
    frame(&ctx, &mut app, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut app,
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
    app.leave_session_layout();
    assert!(!egui::Popup::is_any_open(&ctx));
    assert!(
        ctx.data(|data| data.get_temp::<OpenMenu>(egui::Id::new(OWNER)))
            .is_none()
    );
    app.enter_session_layout();
    assert!(!egui::Popup::is_any_open(&ctx));
    for command in commands.try_iter() {
        assert_ne!(command["type"], "prompt");
        assert_ne!(command["type"], "abort");
        assert_ne!(command["type"], "extension_ui_response");
    }
}
