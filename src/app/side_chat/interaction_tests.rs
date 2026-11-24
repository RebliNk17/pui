use super::redesign_tests::{frame, key, ready, sized_frame};
use super::*;

fn focus(ctx: &egui::Context, side: &str) {
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("btw-editor", side))));
}
fn click(ctx: &egui::Context, app: &mut Pui, pos: egui::Pos2) -> egui::FullOutput {
    let mut output = None;
    for pressed in [true, false] {
        output = Some(frame(
            ctx,
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        ));
    }
    output.unwrap()
}
fn text_position(output: &egui::FullOutput, needle: &str) -> egui::Pos2 {
    fn find(shape: &egui::Shape, needle: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == needle => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, needle)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, needle))
        .unwrap_or_else(|| panic!("missing {needle}"))
}

#[test]
fn inline_permission_keyboard_answers_only_side() {
    let ctx = egui::Context::default();
    let (mut app, _, side, side_records) = ready(&ctx);
    let (client, _, main_records) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let approval = |id| json!({"id":id,"method":"confirm","title":"Allow bash?","message":"{\"command\":\"pwd\"}\n\nApprove this call only."});
    app.state.dialogs.push_back(approval("main-approval"));
    app.agents
        .parked
        .get_mut(&side)
        .unwrap()
        .state
        .dialogs
        .push_back(approval("side-approval"));
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    focus(&ctx, &side);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
    );
    assert!(app.agents.parked[&side].state.dialogs.is_empty());
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(
        side_records
            .try_iter()
            .any(|record| record["type"] == "extension_ui_response"
                && record["id"] == "side-approval"
                && record["confirmed"] == true)
    );
    assert!(
        main_records
            .try_iter()
            .all(|record| record["type"] != "extension_ui_response")
    );
}

#[test]
fn side_effort_popup_does_not_send_to_main_or_change_defaults() {
    let ctx = egui::Context::default();
    let (mut app, main, side, side_records) = ready(&ctx);
    let permission = app.prefs.settings.permission;
    let main_permission = app.active_permission();
    let (client, _, main_records) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.agents.parked.get_mut(&side).unwrap().state.levels = vec!["low".into(), "high".into()];
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let effort = egui::Id::new(("side-chat", side.as_str(), "composer-effort"));
    let pos = ctx.read_response(effort).unwrap().rect.center();
    click(&ctx, &mut app, pos);
    let output = frame(&ctx, &mut app, vec![]);
    let pos = text_position(&output, "Low");
    click(&ctx, &mut app, pos);
    assert!(
        side_records
            .try_iter()
            .any(|record| record["type"] == "set_thinking_level" && record["level"] == "low")
    );
    assert!(
        main_records
            .try_iter()
            .all(|record| record["type"] != "set_thinking_level")
    );
    assert_eq!(app.agents.active, main);
    assert!(app.prefs.settings.permission == permission);
    assert!(app.active_permission() == main_permission);
}

#[test]
fn regular_side_composer_actions_are_within_viewport_in_themes_zoom_and_narrow_windows() {
    for dark in [false, true] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            ctx.set_zoom_factor(zoom);
            let (mut app, _, side, _) = ready(&ctx);
            app.prefs.theme = if dark {
                theme::Theme::Dark
            } else {
                theme::Theme::Light
            };
            for width in [420.0, 1600.0] {
                for _ in 0..4 {
                    sized_frame(&ctx, &mut app, egui::vec2(width, 1000.0), vec![]);
                }
                let editor = ctx
                    .read_response(egui::Id::new(("btw-editor", side.as_str())))
                    .unwrap();
                let send = ctx
                    .data(|data| {
                        data.get_temp::<egui::Rect>(egui::Id::new((
                            "side-chat",
                            side.as_str(),
                            "composer-send-rect",
                        )))
                    })
                    .unwrap();
                assert!(
                    ctx.screen_rect().contains_rect(send),
                    "send {send:?}, viewport {:?}",
                    ctx.screen_rect()
                );
                assert!(ctx.screen_rect().contains_rect(editor.interact_rect));
            }
        }
    }
}
