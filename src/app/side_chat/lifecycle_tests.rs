use super::*;
#[test]
fn presentation_fixture_is_static_independent_and_has_no_clients_or_model_run() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.demo_side_chat();
    assert!(app.demo);
    assert!(app.client.is_none());
    assert!(app.agents.parked.values().all(|chat| chat.client.is_none()));
    assert!(!app.state.busy);
    assert_eq!(app.side_chat.pairs.len(), 1);
    assert!(app.state.messages[0].text.contains("Static design preview"));
    assert!(app.agents.nodes[&app.agents.active].parent.is_none());
}

#[test]
fn parked_main_streams_and_editor_updates_do_not_steal_side_focus_or_draft() {
    let ctx = egui::Context::default();
    let mut app = super::tests::app(&ctx);
    app.state.messages.clear();
    app.state.messages.push(Message {
        role: "assistant".into(),
        text: "main".into(),
        ..Default::default()
    });
    let (client, events) = rpc::Client::stub_with_events();
    app.client = Some(client);
    let main = app.agents.active.clone();
    assert!(app.start_side_chat("side draft"));
    let side = app.agents.active.clone();
    app.focus_composer = false;
    events.send(rpc::Event::Record(json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":" still running"}}))).unwrap();
    events.send(rpc::Event::Record(json!({"type":"extension_ui_request","method":"set_editor_text","text":"main editor update"}))).unwrap();
    app.poll_other_agents();
    assert_eq!(app.agents.active, side);
    assert_eq!(app.draft, "side draft");
    assert!(!app.focus_composer);
    assert_eq!(
        app.agents.parked[&main].state.messages[0].text,
        "main still running"
    );
    assert_eq!(app.agents.parked[&main].draft, "main editor update");
}
#[test]
fn repeated_side_completion_never_reports_to_main_or_changes_its_context() {
    let ctx = egui::Context::default();
    let mut app = super::tests::app(&ctx);
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let main = app.agents.active.clone();
    let original = app.state.messages.len();
    assert!(app.start_side_chat("question"));
    let (client, events) = rpc::Client::stub_with_events();
    app.client = Some(client);
    app.connected = true;
    app.loading_chat = false;
    for _ in 0..3 {
        app.agent_user_turn();
        events
            .send(rpc::Event::Record(
                json!({"type":"agent_end","messages":[]}),
            ))
            .unwrap();
        app.poll_rpc();
        app.poll_other_agents();
    }
    assert!(!app.agents.nodes[&app.agents.active].needs_report);
    assert_eq!(app.agents.parked[&main].state.messages.len(), original);
    for command in commands.try_iter() {
        assert_ne!(command["type"], "prompt");
    }
    assert_eq!(app.draft, "question");
}
#[test]
fn saved_side_is_restored_dormant_as_ordinary_chat_and_reconnect_does_not_replay_question() {
    let ctx = egui::Context::default();
    let mut app = super::tests::app(&ctx);
    assert!(app.start_side_chat("never replay"));
    let side = app.agents.active.clone();
    let dir = std::env::temp_dir().join(format!("pui-btw-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("session.jsonl");
    std::fs::write(&path, "").unwrap();
    app.agents.nodes.get_mut(&side).unwrap().path = Some(path.clone());
    let records = app.agent_records();
    let mut restored = super::tests::app(&ctx);
    restored.agents = Default::default();
    restored.client = None;
    restored.state = State::default();
    restored.prefs.agent_tree = records;
    restored.restore_agents();
    assert!(restored.side_chat.pairs.is_empty());
    assert!(restored.agents.parked[&side].client.is_none());
    assert!(restored.agents.parked[&side].draft.is_empty());
    assert!(restored.agents.nodes[&side].parent.is_none());
    restored.view_agent(&side);
    assert!(restored.client.is_some());
    assert!(restored.draft.is_empty());
    assert!(restored.state.messages.is_empty());
    assert_eq!(restored.state.session_file.as_deref(), path.to_str());
    assert!(!restored.agents.nodes[&side].needs_report);
}
