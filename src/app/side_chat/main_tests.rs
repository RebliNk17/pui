use super::*;
#[test]
fn main_command_returns_exact_owner_busy_run_and_draft_without_send() {
    let ctx = egui::Context::default();
    let mut app = super::tests::app(&ctx);
    let main = app.agents.active.clone();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    assert!(app.start_side_chat("side question"));
    let side = app.agents.active.clone();
    app.agents.parked.get_mut(&main).unwrap().draft = "original main draft".into();
    app.draft = "/main".into();
    assert!(app.submit_command());
    assert_eq!(app.agents.active, main);
    assert_eq!(app.draft, "original main draft");
    assert!(app.state.busy);
    assert!(app.agents.parked[&side].draft.is_empty());
    for command in commands.try_iter() {
        assert_ne!(command["type"], "prompt");
        assert_ne!(command["type"], "abort");
    }
}
#[test]
fn main_missing_pair_arguments_and_foreign_owner_refusals_preserve_command() {
    for case in 0..4 {
        let ctx = egui::Context::default();
        let mut app = super::tests::app(&ctx);
        if case != 0 {
            assert!(app.start_side_chat("question"));
        }
        let active = app.agents.active.clone();
        app.draft = if case == 1 { "/main extra" } else { "/main" }.into();
        let original = app.draft.clone();
        if case == 2 {
            app.state.dialogs.push_back(questions::demo_dialog());
        }
        if case == 3 {
            egui::Popup::open_id(&ctx, egui::Id::new("foreign-main-command"));
        }
        assert!(app.submit_command());
        assert_eq!(app.agents.active, active);
        assert_eq!(app.draft, original);
        assert!(app.state.error.is_some());
    }
}
