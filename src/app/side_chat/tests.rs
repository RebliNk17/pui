use super::*;
pub(super) fn frame(
    ctx: &egui::Context,
    app: &mut Pui,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(420.0, 850.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    )
}
pub(super) fn app(ctx: &egui::Context) -> Pui {
    let mut app = crate::app::files::tests::app(ctx);
    app.demo = false;
    app.connected = true;
    app.new_chat = false;
    app.client = Some(rpc::Client::stub());
    app.state.busy = true;
    app.state.run_observed = true;
    app.ensure_agent();
    app
}
#[test]
fn busy_main_approval_and_client_parked_independent_context_question_never_sent() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let origin = app.agents.active.clone();
    app.state.dialogs.push_back(questions::demo_dialog());
    app.draft = "/btw Explain ownership".into();
    app.agents.nodes.get_mut(&origin).unwrap().permission = Some(settings::PermissionMode::Plan);
    assert!(app.submit_command());
    let side = app.agents.active.clone();
    assert_ne!(origin, side);
    assert_eq!(app.draft, "Explain ownership");
    assert!(app.state.messages.is_empty());
    assert!(app.state.dialogs.is_empty());
    assert!(!app.state.busy);
    assert!(app.client.is_some());
    let node = &app.agents.nodes[&side];
    assert_eq!(node.parent, None);
    assert!(node.permission == Some(settings::PermissionMode::Plan));
    assert!(!node.needs_report);
    assert!(app.agents.nodes[&origin].children.is_empty());
    let main = &app.agents.parked[&origin];
    assert!(main.state.busy);
    assert!(main.client.is_some());
    assert_eq!(main.state.dialogs.len(), 1);
    assert!(main.draft.is_empty());
    app.agent_user_turn();
    assert!(!app.agents.nodes[&side].needs_report);
    app.draft = "side draft".into();
    app.view_agent(&origin);
    assert!(app.state.busy);
    assert_eq!(app.state.dialogs.len(), 1);
    app.draft = "main draft".into();
    app.view_agent(&side);
    assert_eq!(app.draft, "side draft");
    assert_eq!(app.agents.parked[&origin].draft, "main draft");
}
#[test]
fn creation_limits_demo_and_exit_leave_command_and_client_intact() {
    for guard in 0..4 {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let id = app.agents.active.clone();
        app.draft = "/btw kept".into();
        match guard {
            0 => app.demo = true,
            1 => app.agents.exiting = true,
            2 => app.side_chat.pairs = vec![(id.clone(), id.clone()); 16],
            _ => {
                app.prefs.cwd = PathBuf::from("/nonexistent-pui-sidechat-project");
            }
        }
        assert!(app.submit_command());
        assert_eq!(app.agents.active, id);
        assert_eq!(app.draft, "/btw kept");
        assert!(app.client.is_some());
        assert_eq!(app.agents.nodes.len(), 1);
    }
}
#[test]
fn nested_side_chats_keep_original_return_without_worker_edges() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let main = app.agents.active.clone();
    assert!(app.start_side_chat("first"));
    let first = app.agents.active.clone();
    app.loading_chat = false;
    assert!(app.start_side_chat("second"));
    let second = app.agents.active.clone();
    assert_eq!(app.side_chat.pairs.last(), Some(&(main, second.clone())));
    assert_eq!(app.agents.parked[&first].draft, "");
    assert!(app.agents.nodes[&second].parent.is_none());
}
#[test]
fn native_return_rail_light_dark_narrow_zoom_keeps_drafts_and_no_destination_send() {
    for dark in [false, true] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            if dark {
                ctx.set_visuals(egui::Visuals::dark());
            }
            ctx.set_zoom_factor(zoom);
            let mut app = app(&ctx);
            let main = app.agents.active.clone();
            let messages = app.state.messages.len();
            assert!(app.start_side_chat("question"));
            let side = app.agents.active.clone();
            for _ in 0..2 {
                frame(&ctx, &mut app, vec![]);
            }
            let id = ctx
                .data(|data| data.get_temp::<egui::Id>(egui::Id::new("side-chat-switch")))
                .unwrap();
            ctx.memory_mut(|m| m.request_focus(id));
            frame(
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
            assert_eq!(app.agents.active, main);
            assert_eq!(app.agents.parked[&side].draft, "question");
            assert_eq!(app.state.messages.len(), messages);
            assert!(app.state.busy);
        }
    }
}
