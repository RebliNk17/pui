use super::*;

fn fixture() -> (Pui, Receiver<Value>) {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.connected = true;
    app.ultracode_ready = true;
    app.state = State {
        session_file: Some("/fixture/continue.jsonl".into()),
        messages: vec![Message {
            role: "assistant".into(),
            text: "Completed part of the task".into(),
            error: Some("Request timed out.".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let (client, _, commands) = rpc::Client::stub_with_commands();
    app.client = Some(client);
    (app, commands)
}

#[test]
fn continue_uses_existing_session_once_without_touching_draft_or_history() {
    let (mut app, commands) = fixture();
    app.draft = "Unsent draft".into();
    assert!(app.can_continue_after_error());
    app.continue_after_error(0);
    let prompt = commands
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(prompt["type"], "prompt");
    assert!(
        prompt["message"]
            .as_str()
            .unwrap()
            .starts_with("Continue from where you stopped")
    );
    assert_eq!(prompt["images"], json!([]));
    assert_eq!(app.draft, "Unsent draft");
    assert_eq!(
        app.state.session_file.as_deref(),
        Some("/fixture/continue.jsonl")
    );
    assert_eq!(app.state.messages.len(), 1);
    assert_eq!(app.state.messages[0].text, "Completed part of the task");
    assert!(app.state.messages[0].error.is_some());
    assert!(app.state.pending);
    assert_eq!(app.deliveries.len(), 1);
    assert_eq!(app.state.pending_messages.len(), 1);
    assert!(!app.can_continue_after_error());
    app.continue_after_error(0);
    assert!(
        commands.try_recv().is_err(),
        "double click cannot send twice"
    );
}

#[test]
fn continue_rechecks_latest_error_and_normal_send_guards() {
    for case in 0..11 {
        let (mut app, commands) = fixture();
        match case {
            0 => app.state.busy = true,
            1 => app.state.compacting = true,
            2 => app.state.pending = true,
            3 => app.loading_chat = true,
            4 => app.connected = false,
            5 => app.ultracode_ready = false,
            6 => app.demo = true,
            7 => app.state.messages[0].error = None,
            8 => app.state.messages.push(Message {
                role: "user".into(),
                text: "New request".into(),
                ..Default::default()
            }),
            9 => {
                app.waiting.insert(
                    PathBuf::from("/fixture/continue.jsonl"),
                    vec![WaitingMessage {
                        text: "Queued request".into(),
                        ..Default::default()
                    }],
                );
            }
            10 => app.state.pending_messages.push(protocol::PendingMessage {
                id: "uncertain".into(),
                text: "Earlier message".into(),
                images: vec![],
                status: protocol::DeliveryStatus::Uncertain,
                observed: 0,
            }),
            _ => unreachable!(),
        }
        assert!(!app.can_continue_after_error(), "case {case}");
        app.continue_after_error(0);
        assert!(commands.try_recv().is_err(), "case {case}");
    }
    let (mut app, commands) = fixture();
    app.continue_after_error(usize::MAX);
    assert!(commands.try_recv().is_err(), "stale action is ignored");
    assert!(!app.state.pending);
}
