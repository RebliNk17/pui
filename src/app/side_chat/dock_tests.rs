use super::*;

fn frame(ctx: &egui::Context, app: &mut Pui, size: egui::Vec2, events: Vec<egui::Event>) {
    let mut native = eframe::Frame::_new_kittest();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    );
}
fn enter() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn command_preserves_main_run_workspace_and_history_beyond_128_nodes() {
    let ctx = egui::Context::default();
    let mut app = super::tests::app(&ctx);
    let main = app.agents.active.clone();
    app.agents.nodes.get_mut(&main).unwrap().permission = Some(settings::PermissionMode::Plan);
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.dialogs.push_back(questions::demo_dialog());
    for _ in 0..136 {
        app.agents.active.clear();
        let id = app.ensure_agent();
        app.agents.nodes.get_mut(&id).unwrap().stopped = true;
        app.agents.parked.insert(id, Default::default());
    }
    app.agents.active = main.clone();
    app.files.toggle(&ctx, &app.prefs.cwd);
    let layout = serde_json::to_value(app.files.saved_layout()).unwrap();
    app.draft = "/btw hi".into();
    assert!(app.submit_command());
    let side = app.docked_side_chat_target().unwrap();
    assert_eq!(app.agents.nodes.len(), 138);
    assert_eq!(app.agents.active, main);
    assert!(app.state.busy);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.client.is_some());
    assert_eq!(
        serde_json::to_value(app.files.saved_layout()).unwrap(),
        layout
    );
    assert_eq!(app.agents.parked[&side].draft, "hi");
    assert!(app.agents.parked[&side].state.messages.is_empty());
    assert!(app.agents.nodes[&side].permission == Some(settings::PermissionMode::Plan));
    assert!(app.agents.nodes[&side].parent.is_none());
    assert!(!app.agents.nodes[&side].needs_report);
    assert!(
        commands
            .try_iter()
            .all(|record| record["type"] != "prompt" && record["type"] != "abort")
    );
}

#[test]
fn live_connection_capacity_refusal_keeps_command_and_reports_exact_reason() {
    let ctx = egui::Context::default();
    let mut app = super::tests::app(&ctx);
    let main = app.agents.active.clone();
    for _ in 0..126 {
        app.agents.active.clear();
        let id = app.ensure_agent();
        let mut chat = agents::Chat::default();
        chat.client = Some(rpc::Client::stub());
        app.agents.parked.insert(id, chat);
    }
    app.agents.active = main.clone();
    app.draft = "/btw last available".into();
    assert!(app.submit_command());
    assert!(app.docked_side_chat_target().is_some());
    assert_eq!(app.live_side_chat_connections(), 128);
    app.draft = "/btw kept".into();
    assert!(app.submit_command());
    assert_eq!(app.draft, "/btw kept");
    assert_eq!(app.agents.active, main);
    assert_eq!(app.side_chat.pairs.len(), 1);
    assert!(
        app.state
            .error
            .as_deref()
            .unwrap()
            .contains("128 live connections")
    );
}

#[test]
fn split_keeps_details_files_and_browser_above_chat_in_themes_and_zoom() {
    for dark in [false, true] {
        for zoom in [0.8, 1.4] {
            for mode in 0..3 {
                let ctx = egui::Context::default();
                ctx.set_visuals(if dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
                ctx.set_zoom_factor(zoom);
                let mut app = super::tests::app(&ctx);
                app.prefs.hide_sidebar = true;
                if mode == 1 {
                    app.files.toggle(&ctx, &app.prefs.cwd);
                }
                if mode == 2 {
                    app.files.open_visual_html(
                        &ctx,
                        &app.prefs.cwd,
                        "fixture",
                        "Example page",
                        "<h1>Example</h1>",
                    );
                }
                let main = app.agents.active.clone();
                assert!(app.start_docked_side_chat("unsent question"));
                let side = app.docked_side_chat_target().unwrap();
                for width in [420.0, 1600.0] {
                    for _ in 0..3 {
                        frame(&ctx, &mut app, egui::vec2(width, 1000.0), vec![]);
                    }
                    let chat = ctx
                        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("btw-chat-rect")))
                        .unwrap();
                    let editor = ctx
                        .read_response(egui::Id::new(("btw-editor", side.as_str())))
                        .unwrap();
                    assert!(chat.intersects(editor.rect));
                    assert!(
                        ctx.screen_rect().contains_rect(editor.interact_rect),
                        "{editor:?}"
                    );
                    let main_editor = ctx.read_response(egui::Id::new("composer")).unwrap();
                    assert!(!main_editor.interact_rect.intersects(editor.interact_rect));
                    if mode != 0 {
                        let pane = ctx
                            .data(|data| {
                                data.get_temp::<egui::Rect>(egui::Id::new("btw-workspace-rect"))
                            })
                            .unwrap();
                        assert!(
                            pane.bottom() <= chat.top() + 2.0,
                            "file/browser {pane:?}, chat {chat:?}"
                        );
                        assert_eq!(app.files.browser_open(), mode == 2);
                    }
                    assert_eq!(app.agents.active, main);
                    assert_eq!(app.agents.parked[&side].draft, "unsent question");
                }
            }
        }
    }
}

#[test]
fn side_send_goes_only_to_side_and_main_approval_remains_unanswered() {
    let ctx = egui::Context::default();
    let mut app = super::tests::app(&ctx);
    let main = app.agents.active.clone();
    let (client, _, main_commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.dialogs.push_back(questions::demo_dialog());
    assert!(app.start_docked_side_chat("side only"));
    app.draft = "main draft".into();
    let side = app.docked_side_chat_target().unwrap();
    let (client, _, side_commands) = rpc::Client::stub_with_records();
    app.view_agent(&side);
    app.client = Some(client);
    app.connected = true;
    app.loading_chat = false;
    app.ultracode_ready = true;
    app.view_agent(&main);
    for _ in 0..3 {
        frame(&ctx, &mut app, egui::vec2(1600.0, 1000.0), vec![]);
    }
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("btw-editor", side.as_str()))));
    frame(&ctx, &mut app, egui::vec2(1600.0, 1000.0), vec![enter()]);
    assert_eq!(app.agents.active, main);
    assert_eq!(app.draft, "main draft");
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.agents.parked[&side].draft.is_empty());
    let records: Vec<_> = side_commands.try_iter().collect();
    assert_eq!(
        records
            .iter()
            .filter(|record| record["type"] == "prompt" && record["message"] == "side only")
            .count(),
        1
    );
    assert!(
        main_commands
            .try_iter()
            .all(|record| record["type"] != "prompt" && record["type"] != "extension_ui_response")
    );
}
