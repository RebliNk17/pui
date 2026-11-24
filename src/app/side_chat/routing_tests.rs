use super::redesign_tests::{frame, key, ready};
use super::*;

#[test]
fn escape_stops_only_busy_side_after_regular_popup_question_gates() {
    let ctx = egui::Context::default();
    let (mut app, _, side, side_records) = ready(&ctx);
    let (client, _, main_records) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.agents.parked.get_mut(&side).unwrap().state.busy = true;
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("btw-editor", side.as_str()))));
    frame(&ctx, &mut app, vec![]);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(
        side_records
            .try_iter()
            .any(|record| record["type"] == "abort")
    );
    assert!(
        main_records
            .try_iter()
            .all(|record| record["type"] != "abort" && record["type"] != "clear_queue")
    );
}

#[test]
fn attachment_drop_targets_pointer_pane_not_opposite_keyboard_focus() {
    for (side_drop, hidden) in [(false, false), (true, false), (true, true)] {
        let ctx = egui::Context::default();
        let (mut app, _, side, _) = ready(&ctx);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let side_rect = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("side-chat-input-rect")))
            .unwrap();
        let main_rect = ctx.read_response(egui::Id::new("composer")).unwrap().rect;
        app.inspector_panel = hidden;
        let focus = if side_drop {
            egui::Id::new("composer")
        } else {
            egui::Id::new(("btw-editor", side.as_str()))
        };
        ctx.memory_mut(|memory| memory.request_focus(focus));
        let mut native = eframe::Frame::_new_kittest();
        let pos = if side_drop {
            side_rect.center()
        } else {
            main_rect.center()
        };
        let path = app.prefs.cwd.join("README.md");
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1600.0, 1100.0),
                )),
                events: vec![egui::Event::PointerMoved(pos)],
                dropped_files: vec![egui::DroppedFile {
                    path: Some(path),
                    ..Default::default()
                }],
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut app, ctx, &mut native),
        );
        assert_eq!(!app.draft_images.is_empty(), !side_drop || hidden);
        app.view_agent(&side);
        assert_eq!(!app.draft_images.is_empty(), side_drop && !hidden);
    }
}
