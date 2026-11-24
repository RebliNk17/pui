use super::*;

#[test]
fn bypass_is_not_inherited_by_an_independent_chat_and_nested_dock_is_visible() {
    let ctx = egui::Context::default();
    let mut app = super::tests::app(&ctx);
    let main = app.agents.active.clone();
    app.agents.nodes.get_mut(&main).unwrap().permission = Some(settings::PermissionMode::Bypass);
    app.prefs.settings.permission = settings::PermissionMode::Manual;
    assert!(app.start_docked_side_chat("first"));
    let first = app.docked_side_chat_target().unwrap();
    assert!(app.agents.nodes[&first].permission == Some(settings::PermissionMode::Manual));
    app.view_agent(&first);
    app.loading_chat = false;
    app.draft = "/btw nested".into();
    assert!(app.submit_command());
    let second = app
        .docked_side_chat_target()
        .expect("new side composer visible while first side is selected");
    assert_ne!(first, second);
    assert_eq!(app.agents.active, first);
    assert_eq!(app.side_chat.pairs.last(), Some(&(main, second.clone())));
    assert_eq!(app.agents.parked[&second].draft, "nested");
}

#[test]
fn escape_from_side_editor_never_aborts_main_or_cancels_its_question() {
    for question in [false, true] {
        let ctx = egui::Context::default();
        let mut app = super::tests::app(&ctx);
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        if question {
            app.state.dialogs.push_back(questions::demo_dialog());
        }
        assert!(app.start_docked_side_chat("kept side"));
        let side = app.docked_side_chat_target().unwrap();
        for _ in 0..3 {
            super::tests::frame(&ctx, &mut app, vec![]);
        }
        let editor = egui::Id::new(("btw-editor", side.as_str()));
        ctx.memory_mut(|memory| memory.request_focus(editor));
        super::tests::frame(&ctx, &mut app, vec![]);
        super::tests::frame(
            &ctx,
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(app.state.dialogs.len(), usize::from(question));
        assert_eq!(app.agents.parked[&side].draft, "kept side");
        assert!(commands.try_iter().all(|record| record["type"] != "abort"
            && record["type"] != "clear_queue"
            && record["type"] != "extension_ui_response"));
    }
}
