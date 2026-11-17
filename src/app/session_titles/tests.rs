use super::*;

fn app() -> Pui {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(egui::Context::default()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.state = State::default();
    app.sessions.clear();
    app
}
fn saved(path: &str, title: &str) -> Session {
    Session {
        path: path.into(),
        cwd: "/fixture".into(),
        project: "/fixture".into(),
        title: title.into(),
        created: None,
        modified: std::time::SystemTime::UNIX_EPOCH,
    }
}
fn state(path: &str, title: &str) -> State {
    State {
        session_file: Some(path.into()),
        session_name: title.into(),
        ..Default::default()
    }
}
fn count() -> usize {
    RECONCILIATIONS.with(|count| count.get())
}

#[test]
fn idle_and_streaming_polls_do_not_reconcile_titles() {
    let mut app = app();
    app.state = state("/fixture/active.jsonl", "Active title");
    app.sessions = (0..10_000)
        .map(|i| saved(&format!("/fixture/history-{i}.jsonl"), "Saved history"))
        .collect();
    app.sessions
        .push(saved("/fixture/active.jsonl", "Active title"));
    let (client, events) = rpc::Client::stub_with_events();
    app.client = Some(client);
    let before = count();
    for _ in 0..100 {
        app.poll_rpc();
    }
    for record in [
        json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"token"}}),
        json!({"type":"tool_execution_update","toolCallId":"tool","partialResult":{"content":[]}}),
        json!({"type":"response","command":"get_session_stats","success":true,"data":{}}),
        json!({"type":"response","command":"get_state","success":false,"error":"fixture"}),
    ] {
        events.send(rpc::Event::Record(record)).unwrap();
        app.poll_rpc();
    }
    assert_eq!(
        count(),
        before,
        "No session scans for unchanged title inputs"
    );
    assert_eq!(app.state.session_name, "Active title");
}

#[test]
fn history_preview_and_metadata_reconcile_once_per_batch() {
    let mut app = app();
    app.state = state("/fixture/active.jsonl", "");
    app.sessions = vec![saved("/fixture/active.jsonl", "Discovered title")];
    let (client, events) = rpc::Client::stub_with_events();
    app.client = Some(client);
    let before = count();
    events.send(rpc::Event::History(vec![])).unwrap();
    app.poll_rpc();
    assert_eq!(app.state.session_name, "Discovered title");
    assert_eq!(count(), before + 1);

    app.state.session_name.clear();
    app.sessions[0].title.clear();
    let (tx, rx) = mpsc::channel();
    app.preview_rx = Some(rx);
    tx.send(Ok(crate::history::Preview {
        messages: vec![Message {
            role: "user".into(),
            text: "Preview prompt title".into(),
            ..Default::default()
        }],
        partial: true,
    }))
    .unwrap();
    app.poll_rpc();
    assert_eq!(app.state.session_name, "Preview prompt title");
    assert_eq!(count(), before + 2);

    events
        .send(rpc::Event::Record(
            json!({"type":"session_info_changed","name":"Renamed"}),
        ))
        .unwrap();
    events
        .send(rpc::Event::Record(
            json!({"type":"session_info_changed","name":"Final name"}),
        ))
        .unwrap();
    app.poll_rpc();
    assert_eq!(app.sessions[0].title, "Final name");
    assert_eq!(count(), before + 3, "Coalesce metadata in one RPC batch");
    app.poll_rpc();
    assert_eq!(count(), before + 3);
}

#[test]
fn state_and_user_events_keep_title_fallbacks() {
    let mut app = app();
    let (client, events) = rpc::Client::stub_with_events();
    app.client = Some(client);
    for command in ["get_state", "get_entries", "get_messages"] {
        assert!(title_event(
            &json!({"type":"response","command":command,"success":true})
        ));
    }
    events.send(rpc::Event::Record(json!({"type":"response","command":"get_state","success":true,"data":{"sessionFile":"/fixture/new.jsonl","sessionName":""}}))).unwrap();
    events.send(rpc::Event::Record(json!({"type":"message_start","message":{"role":"user","content":[{"type":"text","text":"First prompt"}]}}))).unwrap();
    app.poll_rpc();
    assert_eq!(app.state.session_name, "First prompt");
    assert!(title_event(
        &json!({"type":"message_end","message":{"role":"user"}})
    ));
    assert!(!title_event(
        &json!({"type":"message_end","message":{"role":"assistant"}})
    ));

    for title in ["", "Untitled session", "New conversation", "Opening chat…"] {
        let mut state = state("/fixture/image.jsonl", title);
        state.messages.push(Message {
            role: "user".into(),
            images: vec![
                crate::media::Attachment::from_json(
                    &json!({"type":"image","data":"","mimeType":"image/png"}),
                )
                .unwrap(),
            ],
            ..Default::default()
        });
        reconcile(&mut state, None);
        assert_eq!(state.session_name, "Image attachment");
    }
}

#[test]
fn session_refresh_reconciles_parked_chats_once_not_each_frame() {
    let mut app = app();
    app.demo = false;
    app.state = state("/fixture/active.jsonl", "Active live title");
    app.ensure_agent();
    for i in 0..64 {
        let visible = app.take_chat();
        let active = app.agents.active.clone();
        app.agents.active.clear();
        app.state = state(&format!("/fixture/parked-{i}.jsonl"), &format!("Live {i}"));
        let id = app.ensure_agent();
        let chat = app.take_chat();
        app.agents.parked.insert(id, chat);
        app.agents.active = active;
        app.put_chat(visible);
    }
    let mut sessions: Vec<_> = (0..64)
        .map(|i| saved(&format!("/fixture/parked-{i}.jsonl"), "Stale scan title"))
        .collect();
    sessions.push(saved("/fixture/active.jsonl", "Stale active title"));
    let (tx, rx) = mpsc::channel();
    tx.send(sessions).unwrap();
    app.session_rx = Some(rx);
    let before = count();
    app.poll_views();
    assert_eq!(count(), before + 65);
    assert_eq!(app.sessions[64].title, "Active live title");
    for i in 0..64 {
        assert_eq!(app.sessions[i].title, format!("Live {i}"));
    }
    for _ in 0..10 {
        app.poll_other_agents();
        app.poll_rpc();
    }
    assert_eq!(
        count(),
        before + 65,
        "Parked idle chats must not rescan history"
    );
}
