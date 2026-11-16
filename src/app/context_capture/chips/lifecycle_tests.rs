use super::*;
use crate::app::files::tests::app;

#[test]
fn explicit_send_is_exact_once_and_failed_or_disconnected_send_keeps_association() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.demo = false;
    app.connected = true;
    app.state = State::default();
    app.draft = "Keep original".into();
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    queue(&ctx, "reply · rendered text", "literal untrusted capture");
    app.collect_context_capture();
    let id = app.context_chips.chips[0].id;
    let text = app.draft.clone();
    app.submit(&ctx); // adapter not ready, must retain both draft and chip
    assert_eq!(app.draft, text);
    assert_eq!(app.context_chips.chips[0].id, id);
    assert!(commands.try_recv().is_err());
    app.ultracode_ready = true;
    app.submit(&ctx);
    let record = commands.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(record["type"], "prompt");
    assert_eq!(record["message"], text);
    assert!(app.draft.is_empty());
    assert!(app.context_chips.chips.is_empty());
    app.collect_context_capture();
    assert!(app.draft.is_empty());
    assert!(commands.try_recv().is_err());
    let delivery = app.deliveries[0].id.clone();
    app.delivery_response(&json!({"id":delivery,"success":false}));
    assert_eq!(app.draft, text);
    assert!(
        app.context_chips.chips.is_empty(),
        "failed sent body is plain retained draft, not stale capture metadata"
    );
}

#[test]
fn checkpoint_serializes_only_existing_draft_and_restore_never_recreates_chips_or_execution() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.demo = false;
    app.new_chat = true;
    app.state = State::default();
    app.draft = "Keep checkpoint draft".into();
    queue(&ctx, "console", "private literal capture");
    app.collect_context_capture();
    let text = app.draft.clone();
    app.checkpoint_chats();
    let json = serde_json::to_string(&app.prefs.restart_checkpoints).unwrap();
    assert_eq!(
        json.matches("private literal capture").count(),
        1,
        "body is only in pre-existing draft recovery"
    );
    assert!(!json.contains("binding"));
    assert!(!json.contains("chips"));
    app.restart_pending = app.prefs.restart_checkpoints.clone();
    app.draft.clear();
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.restore_blank_checkpoint();
    assert_eq!(app.draft, text);
    assert!(app.context_chips.chips.is_empty());
    assert!(app.state.dialogs.is_empty());
    assert!(app.deliveries.is_empty());
    assert!(!app.state.busy);
    assert!(commands.try_recv().is_err());
}

#[test]
fn history_prompt_replacement_preserves_unsent_capture_body_but_does_not_retarget_chip() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.demo = false;
    app.connected = true;
    app.ultracode_ready = true;
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state = State {
        session_file: Some("/fixture/chip-history.jsonl".into()),
        messages: vec![Message {
            role: "user".into(),
            text: "Original prompt".into(),
            ..Default::default()
        }],
        history_points: vec![protocol::HistoryPoint {
            entry_id: "user-entry".into(),
            role: "user".into(),
            text: "Original prompt".into(),
            code_reason: Some("No checkpoint".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    app.draft = "Unsent".into();
    queue(&ctx, "source", "capture kept as text");
    app.collect_context_capture();
    let original = app.draft.clone();
    app.transcript.message_action = Some((0, crate::transcript::MessageAction::Edit));
    app.history_action();
    let record = commands.recv_timeout(Duration::from_secs(2)).unwrap();
    let operation: serde_json::Value = serde_json::from_str(
        record["message"]
            .as_str()
            .unwrap()
            .split_once(' ')
            .unwrap()
            .1,
    )
    .unwrap();
    let id = operation["id"].as_str().unwrap();
    let data = json!({"id":id,"success":true,"files":0});
    assert!(app.history_event(&json!({"type":"extension_ui_request","method":"setStatus","statusKey":"pui-history-result","statusText":data.to_string()})));
    assert_eq!(app.draft, "Original prompt");
    assert!(app.context_chips.chips.is_empty());
    assert_eq!(app.history_drafts[0].text, original);
    assert!(app.deliveries.is_empty());
}
