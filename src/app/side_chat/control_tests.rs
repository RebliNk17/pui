use super::redesign_tests::{frame, key, ready};
use super::*;

fn focus(ctx: &egui::Context, side: &str) {
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("btw-editor", side))));
}

#[test]
fn main_enter_cannot_answer_side_request_or_pending_bypass_or_send_its_draft() {
    let ctx = egui::Context::default();
    let (mut app, main, side, side_records) = ready(&ctx);
    app.agents
        .parked
        .get_mut(&side)
        .unwrap()
        .state
        .dialogs
        .push_back(questions::demo_dialog());
    let (client, _, main_records) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.busy = false;
    app.state.run_observed = false;
    app.ultracode_ready = true;
    app.draft = "main only".into();
    app.side_chat.ui_owner = side.clone();
    app.view_agent(&side);
    app.swap_side_chat_ui();
    app.bypass_pending = Some(app.permission_scope());
    app.swap_side_chat_ui();
    app.view_agent(&main);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(
        main_records
            .try_iter()
            .any(|record| record["type"] == "prompt" && record["message"] == "main only")
    );
    assert!(
        side_records
            .try_iter()
            .all(|record| record["type"] != "prompt" && record["type"] != "extension_ui_response")
    );
    assert_eq!(app.agents.parked[&side].state.dialogs.len(), 1);
}

#[test]
fn side_completion_inserts_without_send_and_unsafe_navigation_keeps_draft() {
    let ctx = egui::Context::default();
    let (mut app, main, side, side_records) = ready(&ctx);
    app.agents.parked.get_mut(&side).unwrap().draft = "/mod".into();
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    focus(&ctx, &side);
    let mut state = egui::widgets::text_edit::TextEditState::load(
        &ctx,
        egui::Id::new(("btw-editor", side.as_str())),
    )
    .unwrap();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(4),
        )));
    state.store(&ctx, egui::Id::new(("btw-editor", side.as_str())));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(app.agents.parked[&side].draft, "/model ");
    assert_eq!(app.agents.active, main);
    assert!(
        side_records
            .try_iter()
            .all(|record| record["type"] != "prompt")
    );
    app.agents.parked.get_mut(&side).unwrap().draft = "/new".into();
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    focus(&ctx, &side);
    // Slash completion's Enter inserts first; the next explicit Enter executes.
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(app.agents.active, main);
    assert!(app.agents.parked[&side].draft.starts_with("/new"));
}

#[test]
fn main_capture_stays_main_and_side_permission_change_keeps_window_default() {
    let ctx = egui::Context::default();
    let (mut app, main, side, _) = ready(&ctx);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    context_capture::queue(&ctx, "Main quote", "Never transfer to side");
    frame(&ctx, &mut app, vec![]);
    assert!(app.draft.contains("Never transfer to side"));
    assert!(
        !app.agents.parked[&side]
            .draft
            .contains("Never transfer to side")
    );
    let default = app.prefs.settings.permission;
    let main_permission = app.active_permission();
    let (client, _, records) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.view_agent(&side);
    app.side_chat.rendering = true;
    app.select_permission(settings::PermissionMode::Plan);
    app.side_chat.rendering = false;
    app.view_agent(&main);
    assert!(app.prefs.settings.permission == default);
    assert!(app.active_permission() == main_permission);
    assert!(app.agents.nodes[&side].permission == Some(settings::PermissionMode::Plan));
    assert!(records.try_iter().all(|record| record["type"] != "prompt"));
}
