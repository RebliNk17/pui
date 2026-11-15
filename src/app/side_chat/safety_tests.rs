use super::*;
#[test]
fn readonly_workers_and_session_operations_cannot_escape_through_side_chat() {
    for guard in 0..3 {
        let ctx = egui::Context::default();
        let mut app = super::tests::app(&ctx);
        let id = app.agents.active.clone();
        app.draft = "/btw kept".into();
        match guard {
            0 => app.agents.nodes.get_mut(&id).unwrap().access = "read-only".into(),
            1 => app.loading_chat = true,
            _ => app.agents.nodes.get_mut(&id).unwrap().parent = Some("worker-owner".into()),
        }
        assert!(app.submit_command());
        assert_eq!(app.draft, "/btw kept");
        assert_eq!(app.agents.active, id);
        assert_eq!(app.agents.nodes.len(), 1);
    }
}
#[test]
fn ready_main_draft_switch_activation_cannot_dispatch_destination_prompt() {
    let ctx = egui::Context::default();
    let mut app = super::tests::app(&ctx);
    app.state.busy = false;
    app.state.run_observed = false;
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let main = app.agents.active.clone();
    assert!(app.start_side_chat("unsent side"));
    app.agents.parked.get_mut(&main).unwrap().draft = "NEVER SEND THIS ON RETURN".into();
    for _ in 0..2 {
        super::tests::frame(&ctx, &mut app, vec![]);
    }
    let id = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new("side-chat-switch")))
        .unwrap();
    let pos = ctx.read_response(id).unwrap().interact_rect.center();
    ctx.memory_mut(|m| m.request_focus(id));
    for pressed in [true, false] {
        super::tests::frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert_eq!(app.agents.active, main);
    assert_eq!(app.draft, "NEVER SEND THIS ON RETURN");
    for command in commands.try_iter() {
        assert_ne!(command["type"], "prompt");
        assert_ne!(command["type"], "extension_ui_response");
    }
}
#[test]
fn disposed_blank_pairs_are_pruned_before_capacity_and_no_model_context_retained() {
    let ctx = egui::Context::default();
    let mut app = super::tests::app(&ctx);
    let main = app.agents.active.clone();
    app.side_chat.pairs = vec![(main, "deleted".into()); 16];
    assert!(app.start_side_chat(""));
    assert_eq!(app.side_chat.pairs.len(), 1);
    assert!(app.state.messages.is_empty());
    eprintln!(
        "SideChat fixed state {}B; pairs capacity {} entries, no transcript/body copies",
        std::mem::size_of::<SideChat>(),
        app.side_chat.pairs.capacity()
    );
}
