use super::redesign_tests::{frame, key, ready};
use super::*;

#[test]
fn disabled_side_question_cannot_answer_by_shortcut() {
    let ctx = egui::Context::default();
    let (mut app, _, side, records) = ready(&ctx);
    app.agents.parked.get_mut(&side).unwrap().state.dialogs.push_back(json!({
        "id":"disabled", "method":"confirm", "title":"Allow bash?", "message":"{}\n\nApprove this call only."
    }));
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    app.agents.exiting = true;
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("btw-editor", side.as_str()))));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
    );
    assert_eq!(app.agents.parked[&side].state.dialogs.len(), 1);
    assert!(
        records
            .try_iter()
            .all(|record| record["type"] != "extension_ui_response")
    );
}

#[test]
fn side_image_escape_closes_gallery_without_stopping_main() {
    let ctx = egui::Context::default();
    let (mut app, main, side, _) = ready(&ctx);
    let (client, _, records) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let image =
        crate::media::Attachment::from_bytes(include_bytes!("../../../assets/icon.png")).unwrap();
    app.agents
        .parked
        .get_mut(&side)
        .unwrap()
        .open_test_gallery(image);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert_eq!(app.agents.active, main);
    assert!(!app.agents.parked[&side].gallery_open());
    assert!(records.try_iter().all(|record| record["type"] != "abort"));
}

#[test]
fn side_background_click_does_not_claim_main_editor_focus() {
    let ctx = egui::Context::default();
    let (mut app, _, side, _) = ready(&ctx);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
    let rect = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("side-chat-input-rect")))
        .unwrap();
    let pos = rect.left_top() + egui::vec2(4.0, 50.0);
    for pressed in [true, false] {
        frame(
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
            ],
        );
    }
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("main typing".into())],
    );
    assert!(app.draft.contains("main typing"));
    assert!(!app.agents.parked[&side].draft.contains("main typing"));
    assert_ne!(
        app.side_chat.keyboard_focus,
        Some(egui::Id::new("composer"))
    );
}
