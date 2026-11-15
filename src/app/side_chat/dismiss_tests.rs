use super::*;
#[test]
fn dismiss_companion_only_removes_exact_runtime_pair_and_never_stops_or_sends() {
    let ctx = egui::Context::default();
    let mut app = super::tests::app(&ctx);
    let main = app.agents.active.clone();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    assert!(app.start_side_chat("preserve side draft"));
    let side = app.agents.active.clone();
    app.side_chat
        .pairs
        .insert(0, ("unrelated-main".into(), "unrelated-side".into()));
    for _ in 0..2 {
        super::tests::frame(&ctx, &mut app, vec![]);
    }
    let id = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new("side-chat-dismiss")))
        .unwrap();
    ctx.memory_mut(|memory| memory.request_focus(id));
    super::tests::frame(
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
    assert_eq!(app.agents.active, side);
    assert_eq!(app.draft, "preserve side draft");
    assert_eq!(
        app.side_chat.pairs,
        vec![("unrelated-main".into(), "unrelated-side".into())]
    );
    assert!(app.agents.parked[&main].client.is_some());
    assert!(app.agents.parked[&main].state.busy);
    assert!(app.agents.nodes.contains_key(&side));
    for command in commands.try_iter() {
        assert_ne!(command["type"], "abort");
        assert_ne!(command["type"], "prompt");
    }
    app.draft = "/main".into();
    assert!(app.submit_command());
    assert_eq!(app.agents.active, side);
    assert_eq!(app.draft, "/main");
}
#[test]
fn foreign_popup_prevents_companion_dismissal() {
    let ctx = egui::Context::default();
    let mut app = super::tests::app(&ctx);
    assert!(app.start_side_chat("kept"));
    for _ in 0..2 {
        super::tests::frame(&ctx, &mut app, vec![]);
    }
    let id = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new("side-chat-dismiss")))
        .unwrap();
    ctx.memory_mut(|memory| memory.request_focus(id));
    egui::Popup::open_id(&ctx, egui::Id::new("foreign-dismiss"));
    super::tests::frame(
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
    assert_eq!(app.side_chat.pairs.len(), 1);
    assert_eq!(app.draft, "kept");
}
