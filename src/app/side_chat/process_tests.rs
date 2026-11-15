//! Real deterministic child processes, never a provider or detached AI agent.
use super::*;
fn until(app: &mut Pui, mut done: impl FnMut(&Pui) -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !done(app) {
        assert!(
            std::time::Instant::now() < deadline,
            "fixture transport deadline; {:?}",
            app.state.error
        );
        app.poll_rpc();
        app.poll_other_agents();
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn separate_child_processes_deliver_only_explicit_prompts_and_never_merge_context() {
    let ctx = egui::Context::default();
    let mut app = super::tests::app(&ctx);
    app.state.messages.clear();
    app.client = Some(rpc::Client::process_fixture(ctx.clone()));
    app.send(json!({"type":"prompt","message":"explicit main message"}));
    until(&mut app, |app| {
        app.state
            .messages
            .iter()
            .any(|m| m.role == "assistant" && !m.text.is_empty())
    });
    until(&mut app, |app| app.state.ready());
    let main = app.agents.active.clone();
    let main_count = app.state.messages.len();
    assert!(app.start_side_chat("UNSENT SIDE QUESTION"));
    let side = app.agents.active.clone();
    app.client = Some(rpc::Client::process_fixture(ctx));
    app.loading_chat = false;
    app.connected = true;
    app.send(protocol::command("get_state"));
    until(&mut app, |app| app.state.model == "fixture-model");
    assert!(app.state.messages.is_empty());
    assert_eq!(app.draft, "UNSENT SIDE QUESTION");
    assert_eq!(app.agents.parked[&main].state.messages.len(), main_count);
    app.agent_user_turn();
    app.send(json!({"type":"prompt","message":"explicit side message"}));
    until(&mut app, |app| {
        app.state
            .messages
            .iter()
            .any(|m| m.role == "assistant" && !m.text.is_empty())
            && app.state.ready()
    });
    assert!(
        app.state
            .messages
            .iter()
            .any(|m| m.text == "explicit side message")
    );
    assert!(
        app.state
            .messages
            .iter()
            .all(|m| !m.text.contains("explicit main message"))
    );
    assert_eq!(app.agents.parked[&main].state.messages.len(), main_count);
    assert!(!app.agents.nodes[&side].needs_report);
    app.view_agent(&main);
    assert!(
        app.state
            .messages
            .iter()
            .any(|m| m.text == "explicit main message")
    );
    assert!(
        app.state
            .messages
            .iter()
            .all(|m| !m.text.contains("explicit side message"))
    );
    assert_eq!(app.agents.parked[&side].draft, "UNSENT SIDE QUESTION");
}
