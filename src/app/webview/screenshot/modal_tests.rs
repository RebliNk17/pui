use super::*;
use tests::{captured, frame};
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}
#[test]
fn safe_cancel_focus_modified_focused_add_and_foreign_prompt_keep_input_isolated() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let (_, ticket, frozen) = captured(&ctx);
    let mut editor = Editor::new("fixture", ticket.epoch, 8);
    editor.ticket = Some(ticket);
    editor.frozen = Some(frozen);
    editor.phase = Phase::Review;
    for _ in 0..3 {
        frame(&ctx, &mut editor, true, vec![], 700.0);
    }
    let cancel = ctx
        .data(|d| d.get_temp::<egui::Id>(egui::Id::new(("screenshot-control-id", "Cancel"))))
        .unwrap();
    assert_eq!(ctx.memory(|m| m.focused()), Some(cancel));
    let add = ctx
        .data(|d| {
            d.get_temp::<egui::Id>(egui::Id::new((
                "screenshot-control-id",
                "Add image to draft",
            )))
        })
        .unwrap();
    ctx.memory_mut(|m| m.request_focus(add));
    let decision = frame(
        &ctx,
        &mut editor,
        true,
        vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
        700.0,
    );
    assert!(!decision.add && !decision.capture && !decision.review);
    let mut remaining = Vec::new();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700.0, 760.0),
            )),
            events: vec![
                egui::Event::Text(" ".into()),
                egui::Event::Paste("secret".into()),
                key(egui::Key::Backspace, egui::Modifiers::COMMAND),
            ],
            ..Default::default()
        },
        |ctx| {
            ui::show(ctx, &mut editor, true);
            remaining = ctx.input(|i| i.events.clone());
        },
    );
    assert!(!remaining.iter().any(|e| matches!(
        e,
        egui::Event::Text(_) | egui::Event::Paste(_) | egui::Event::Key { .. }
    )));
    // Establish a different owner as top prompt, then make sure screenshot does
    // not steal its Escape/text or permit its underlying Add button to activate.
    let mut owned = egui::LayerId::background();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        owned = prompt_ui::modal(ctx, egui::Id::new("screenshot-foreign"), 200.0, |ui| {
            prompt_ui::heading(ui, "Other prompt", "Owns input")
        })
        .response
        .layer_id;
    });
    let mut decision = ui::Decision::default();
    let mut escape = false;
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
            ..Default::default()
        },
        |ctx| {
            prompt_ui::modal(ctx, egui::Id::new("screenshot-foreign"), 200.0, |ui| {
                prompt_ui::heading(ui, "Other prompt", "Owns input")
            });
            decision = ui::show(ctx, &mut editor, true);
            escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
            assert_eq!(ctx.memory(|m| m.top_modal_layer()), Some(owned));
        },
    );
    assert!(!decision.add && !decision.cancel);
    assert!(escape);
}
