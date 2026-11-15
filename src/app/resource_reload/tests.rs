use super::*;

fn app(ctx: &egui::Context) -> Pui {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.connected = true;
    app.ultracode_ready = true;
    app.client = Some(rpc::Client::stub());
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
        crate::media::Attachment::from_bytes(include_bytes!("../../../assets/icon.png")).unwrap(),
    ]);
    app
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
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
#[test]
fn confirmation_is_bounded_and_escape_cancel_backdrop_and_enter_never_reload() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for (width, zoom) in [(620.0, 1.0), (850.0, 1.4)] {
            for action in ["escape", "cancel", "backdrop", "enter"] {
                let ctx = egui::Context::default();
                let mut app = app(&ctx);
                theme.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                app.request_resource_reload();
                for _ in 0..4 {
                    frame(&ctx, &mut app, vec![], width);
                }
                let output = frame(&ctx, &mut app, vec![], width);
                let rect = ctx
                    .read_response(egui::Id::new("reload-chat-resources"))
                    .unwrap()
                    .rect;
                assert!(ctx.screen_rect().contains_rect(rect), "{rect:?}");
                match action {
                    "escape" => {
                        frame(&ctx, &mut app, vec![key(egui::Key::Escape)], width);
                    }
                    "enter" => {
                        frame(&ctx, &mut app, vec![key(egui::Key::Enter)], width);
                    }
                    _ => {
                        let pos = if action == "backdrop" {
                            egui::pos2(2.0, 2.0)
                        } else {
                            files::tests::text(&output, "Cancel")
                        };
                        for pressed in [true, false] {
                            frame(
                                &ctx,
                                &mut app,
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
                }
                if action != "enter" {
                    assert!(!app.resource_reload.confirming());
                }
                assert!(!app.resource_reload.pending());
                assert!(app.client.is_some());
                assert_eq!(app.draft, "Keep this unsent draft");
                assert_eq!(app.draft_images.images().len(), 1);
                assert!(app.deliveries.is_empty());
            }
        }
    }
}

#[test]
fn reload_reaps_old_process_refreshes_commands_and_keeps_history_drafts_and_access() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let id = app.ensure_agent();
    app.agents.nodes.get_mut(&id).unwrap().access = "read-only".into();
    app.agents.nodes.get_mut(&id).unwrap().launch = Some(rpc::Launch {
        readonly: true,
        worker: true,
        ..Default::default()
    });
    app.request_resource_reload();
    // Becoming busy between request and confirmation is rechecked.
    app.state.busy = true;
    app.begin_resource_reload();
    assert!(app.client.is_some());
    assert!(!app.resource_reload.pending());
    app.state.busy = false;
    app.begin_resource_reload();
    assert!(app.client.is_none());
    assert!(app.resource_reload.pending());
    assert!(app.state.commands.is_empty());
    let child = run_status::WaitingMessage {
        text: "Do not send during reload".into(),
        ..Default::default()
    };
    assert!(!app.deliver_message(child, "prompt"));
    for _ in 0..100 {
        app.poll_resource_reload();
        if app.client.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.resource_reload.starting);
    assert!(app.loading_chat);
    assert_eq!(app.state.messages[0].text, "Existing question");
    assert_eq!(app.draft, "Keep this unsent draft");
    assert_eq!(app.draft_images.images().len(), 1);
    assert_eq!(app.agents.nodes[&id].access, "read-only");
    assert!(app.agents.nodes[&id].launch.as_ref().unwrap().readonly);
    assert!(app.deliveries.is_empty());
    let (client, events, commands) = rpc::Client::stub_with_commands();
    app.client.take().unwrap().retire();
    app.client = Some(client);
    events.send(rpc::Event::Record(json!({"type":"response","command":"get_commands","success":true,"data":{"commands":[{"name":"skill:tdd","description":"TDD"}]}}))).unwrap();
    app.poll_rpc();
    assert!(app.loading_chat, "Catalog alone is not enough");
    events
        .send(rpc::Event::History(app.state.messages.clone()))
        .unwrap();
    events.send(rpc::Event::Record(json!({"type":"extension_ui_request","method":"setStatus","statusKey":"pui-ultracode","statusText":"{\"ready\":true,\"enabled\":false}"}))).unwrap();
    app.poll_rpc();
    assert!(!app.resource_reload.pending());
    assert!(!app.loading_chat);
    assert_eq!(app.state.commands[0].name, "skill:tdd");
    assert_eq!(
        app.state.session_file.as_deref(),
        Some("/fixture/reload.jsonl")
    );
    assert_eq!(app.state.messages[0].text, "Existing question");
    assert!(commands.try_iter().all(|r| r["type"] != "prompt"));
    assert!(
        app.state
            .notice
            .as_ref()
            .unwrap()
            .contains("Resources reloaded")
    );
}

#[test]
fn disabled_confirmation_rechecks_busy_state_and_failed_reconnect_keeps_draft() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.request_resource_reload();
    app.state.busy = true;
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![], 850.0);
    }
    let output = frame(&ctx, &mut app, vec![], 850.0);
    let pos = files::tests::text(&output, "Reload resources");
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            850.0,
        );
    }
    assert!(!app.resource_reload.pending());
    assert!(app.client.is_some());
    app.state.busy = false;
    app.begin_resource_reload();
    for _ in 0..100 {
        app.poll_resource_reload();
        if app.resource_reload.starting {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    app.resource_reload.started = Some(std::time::Instant::now() - Duration::from_secs(61));
    app.poll_resource_reload();
    assert!(app.client.is_none());
    assert!(
        app.resource_reload.pending(),
        "Wait for failed writer to retire too"
    );
    for _ in 0..100 {
        app.poll_resource_reload();
        if !app.resource_reload.pending() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!app.resource_reload.pending());
    assert!(!app.loading_chat);
    assert_eq!(app.draft, "Keep this unsent draft");
    assert_eq!(app.state.messages[0].text, "Existing question");
    assert!(app.state.error.as_ref().unwrap().contains("timed out"));
    assert!(app.deliveries.is_empty());
}

#[test]
fn reload_is_idle_only_and_scoped_to_the_parked_chat() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.draft = "/reload".into();
    app.state.busy = true;
    assert!(app.submit_command());
    assert!(!app.resource_reload.confirming());
    assert_eq!(app.draft, "/reload");
    app.state.busy = false;
    app.waiting.insert(
        "/fixture/reload.jsonl".into(),
        vec![run_status::WaitingMessage::default()],
    );
    app.request_resource_reload();
    assert!(!app.resource_reload.confirming());
    app.waiting.clear();
    let id = app.ensure_agent();
    app.request_resource_reload();
    assert!(app.resource_reload.confirming());
    app.resource_reload.confirming = false;
    app.begin_resource_reload();
    let chat = app.take_chat();
    assert!(!app.resource_reload.pending());
    app.put_chat(chat);
    assert_eq!(app.agents.active, id);
    assert!(app.resource_reload.pending());
    app.resource_reload = Default::default();
    app.client = Some(rpc::Client::stub());
    app.connected = true;
    app.loading_chat = false;
    app.agents.background = true;
    app.request_resource_reload();
    assert!(!app.resource_reload.confirming());
}
