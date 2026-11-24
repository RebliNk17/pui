use super::*;

fn app(ctx: &egui::Context) -> (Pui, Receiver<Value>) {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.connected = true;
    app.state = State {
        session_file: Some("/fixture/reload.jsonl".into()),
        messages: vec![Message {
            role: "user".into(),
            text: "Existing question".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    app.draft = "Keep this unsent draft".into();
    app.draft_images.set_images(vec![
        crate::media::Attachment::from_bytes(include_bytes!("../../../../assets/icon.png"))
            .unwrap(),
    ]);
    let (client, _, commands) = rpc::Client::stub_with_commands();
    app.client = Some(client);
    app.state.commands.push(protocol::SlashCommand {
        name: "mcp".into(),
        ..Default::default()
    });
    (app, commands)
}
fn receipt(app: &Pui) -> Value {
    json!({"type":"response", "command":"prompt", "success":true,
        "id":app.resource_reload.connector_pending.as_ref().unwrap().id,
        "data":{"disposition":"handled"}})
}
fn success(name: &str) -> Value {
    json!({"type":"extension_ui_request", "method":"notify", "notifyType":"info",
        "message":format!("Reconnected to MCP server \"{name}\" (connected, 2 tools).")})
}

#[test]
fn named_reconnect_keeps_chat_draft_attachments_approvals_and_other_connectors() {
    let ctx = egui::Context::default();
    let (mut app, commands) = app(&ctx);
    app.state
        .dialogs
        .push_back(json!({"method":"confirm", "id":"approval"}));
    app.resource_reload.reconnected.insert("other".into());
    app.request_connector_reconnect("slack");
    let command = commands.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(command["message"], "/mcp reconnect slack");
    assert_eq!(command["type"], "prompt");
    assert!(command.get("streamingBehavior").is_none());
    assert!(app.connected && app.client.is_some());
    assert!(!app.resource_reload.pending());
    assert_eq!(
        app.state.session_file.as_deref(),
        Some("/fixture/reload.jsonl")
    );
    assert_eq!(app.state.messages[0].text, "Existing question");
    assert_eq!(app.draft, "Keep this unsent draft");
    assert_eq!(app.draft_images.images().len(), 1);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.deliveries.is_empty() && app.state.pending_messages.is_empty());
    assert!(!app.state.busy && !app.state.pending);
    let receipt = receipt(&app);
    assert!(!app.connector_reconnect_record(
        &json!({"type":"response", "command":"prompt", "id":"other", "success":true})
    ));
    assert!(!app.connector_reconnect_record(&success("other")));
    assert!(app.connector_reconnect_record(&success("slack")));
    assert!(app.connector_reconnect_record(&receipt));
    assert!(!app.resource_reload.connector_pending());
    assert!(app.resource_reload.connector_reconnected("slack"));
    assert!(app.resource_reload.connector_reconnected("other"));
    assert!(app.state.notice.as_ref().unwrap().contains("not restarted"));
    assert!(
        commands.try_recv().is_err(),
        "No reload, model prompt or refresh commands"
    );
}

#[test]
fn accepted_command_is_not_connection_success_and_failure_remains_diagnostic() {
    let ctx = egui::Context::default();
    let (mut app, _) = app(&ctx);
    app.request_connector_reconnect("slack");
    let receipt = receipt(&app);
    let failure = json!({"type":"extension_ui_request", "method":"notify", "notifyType":"error", "message":"slack: sign in required"});
    assert!(!app.connector_reconnect_record(&failure));
    app.state.apply(failure);
    assert!(app.connector_reconnect_record(&receipt));
    assert!(!app.resource_reload.connector_reconnected("slack"));
    assert_eq!(app.state.diagnostics, ["slack: sign in required"]);
    assert!(app.state.error.as_ref().unwrap().contains("not confirmed"));
    assert!(app.connected && app.client.is_some());
}

fn frame(
    ctx: &egui::Context,
    app: &mut Pui,
    events: Vec<egui::Event>,
    width: f32,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(width, 1000.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| app.resource_reload_dialog(ctx),
    )
}
fn click(ctx: &egui::Context, app: &mut Pui, pos: egui::Pos2, width: f32) {
    for pressed in [true, false] {
        frame(
            ctx,
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            width,
        );
    }
}
#[test]
fn active_chat_can_reconnect_only_after_explicit_confirmation_and_cancel_is_safe() {
    for dark in [false, true] {
        for (width, zoom) in [(560.0, 1.0), (850.0, 1.4)] {
            for action in ["confirm", "cancel", "escape", "backdrop", "disabled"] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                ctx.set_visuals(if dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
                ctx.set_zoom_factor(zoom);
                let (mut app, commands) = app(&ctx);
                app.state.busy = true;
                app.request_connector_reconnect("slack");
                assert!(app.resource_reload.confirming());
                assert!(!app.resource_reload.connector_pending());
                if action == "disabled" {
                    app.connected = false;
                }
                for _ in 0..4 {
                    frame(&ctx, &mut app, vec![], width);
                }
                let out = frame(&ctx, &mut app, vec![], width);
                let rect = ctx
                    .read_response(egui::Id::new("reconnect-mcp-connector"))
                    .unwrap()
                    .rect;
                assert!(ctx.screen_rect().contains_rect(rect), "{rect:?}");
                match action {
                    "escape" => {
                        frame(
                            &ctx,
                            &mut app,
                            vec![egui::Event::Key {
                                key: egui::Key::Escape,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers: egui::Modifiers::NONE,
                            }],
                            width,
                        );
                    }
                    "backdrop" => click(&ctx, &mut app, egui::pos2(2.0, 2.0), width),
                    _ => {
                        let text = if action == "cancel" {
                            "Cancel"
                        } else {
                            "Reconnect connector"
                        };
                        click(&ctx, &mut app, files::tests::text(&out, text), width);
                    }
                }
                if action == "confirm" {
                    assert_eq!(
                        commands.recv_timeout(Duration::from_secs(1)).unwrap()["message"],
                        "/mcp reconnect slack"
                    );
                    assert!(app.resource_reload.connector_pending());
                } else {
                    assert!(commands.try_recv().is_err());
                    assert!(!app.resource_reload.connector_pending());
                }
                assert!(app.state.busy, "Must not interrupt/settle the chat");
                assert!(app.client.is_some());
                assert_eq!(app.draft, "Keep this unsent draft");
                assert_eq!(app.draft_images.images().len(), 1);
                assert!(app.state.error.is_none());
                assert!(!app.resource_reload.pending());
            }
        }
    }
}

#[test]
fn missing_command_invalid_name_and_duplicate_requests_never_send_or_start_a_model() {
    let ctx = egui::Context::default();
    let (mut app, commands) = app(&ctx);
    for name in ["", "slack\nother", "slack extra", "/reload"] {
        app.request_connector_reconnect(name);
    }
    app.state.commands.clear();
    app.request_connector_reconnect("slack");
    assert!(commands.try_recv().is_err());
    assert!(
        app.connector_reconnect_blocked()
            .unwrap()
            .contains("does not provide /mcp")
    );
    app.state.commands.push(protocol::SlashCommand {
        name: "mcp".into(),
        ..Default::default()
    });
    app.request_connector_reconnect("slack");
    commands.recv_timeout(Duration::from_secs(1)).unwrap();
    app.request_connector_reconnect("other");
    assert!(commands.try_recv().is_err());
    app.resource_reload
        .connector_pending
        .as_mut()
        .unwrap()
        .started = std::time::Instant::now() - Duration::from_secs(121);
    app.poll_connector_reconnect();
    assert!(app.state.error.as_ref().unwrap().contains("not completed"));
    assert!(
        app.resource_reload.connector_pending(),
        "Uncertain request stays locked against duplicate retries"
    );
    assert!(app.connected && app.client.is_some());
    assert!(commands.try_recv().is_err());
}

#[test]
fn reconnect_state_follows_parked_chat_and_failed_connection_clears_stale_success() {
    let ctx = egui::Context::default();
    let (mut app, _) = app(&ctx);
    app.request_connector_reconnect("slack");
    let receipt = receipt(&app);
    let chat = app.take_chat();
    assert!(!app.resource_reload.connector_pending());
    app.put_chat(chat);
    assert!(app.connector_reconnect_record(&success("slack")));
    assert!(app.connector_reconnect_record(&receipt));
    let warning = json!({"type":"extension_ui_request", "method":"notify", "notifyType":"warning", "message":"slack: disconnected"});
    assert!(!app.connector_reconnect_record(&warning));
    assert!(!app.resource_reload.connector_reconnected("slack"));
    app.request_connector_reconnect("slack");
    app.connected = false;
    app.poll_connector_reconnect();
    assert!(!app.resource_reload.connector_pending());
    assert!(
        app.state
            .error
            .as_ref()
            .unwrap()
            .contains("Outcome unknown")
    );
}

#[test]
fn pending_reconnect_polls_in_background_blocks_retirement_and_clears_only_owned_errors() {
    let ctx = egui::Context::default();
    let (mut app, _) = app(&ctx);
    app.draft.clear();
    app.draft_images.clear();
    app.request_connector_reconnect("slack");
    let id = app.agents.active.clone();
    assert!(!id.is_empty());
    assert!(app.retain_chat_on_switch());
    let now = std::time::Instant::now();
    app.poll_idle_agents(now);
    app.poll_idle_agents(now + Duration::from_secs(601));
    assert!(
        app.client.is_some(),
        "Active reconnect must not idle-retire"
    );

    app.agents.background = true;
    app.resource_reload
        .connector_pending
        .as_mut()
        .unwrap()
        .started = std::time::Instant::now() - Duration::from_secs(121);
    app.poll_rpc();
    assert!(
        app.state
            .error
            .as_ref()
            .is_some_and(|e| e.contains("not completed")),
        "Background poll must still run connector timeout"
    );

    let first_receipt = receipt(&app);
    let chat = app.take_chat();
    assert!(chat.deletion_busy());
    app.agents.parked.insert(id.clone(), chat);
    app.poll_idle_agents(now + Duration::from_secs(1202));
    assert!(
        app.agents.parked[&id].client.is_some(),
        "Parked reconnect must not idle-retire"
    );
    let chat = app.agents.parked.remove(&id).unwrap();
    app.put_chat(chat);
    app.agents.background = false;

    assert!(app.connector_reconnect_record(&success("slack")));
    assert!(app.connector_reconnect_record(&first_receipt));
    assert!(
        app.state.error.is_none(),
        "Late success clears owned timeout error"
    );

    app.state.error = Some("Unrelated error".into());
    app.request_connector_reconnect("slack");
    let second_receipt = receipt(&app);
    assert!(app.connector_reconnect_record(&second_receipt));
    assert_eq!(app.state.error.as_deref(), Some("Unrelated error"));
}
