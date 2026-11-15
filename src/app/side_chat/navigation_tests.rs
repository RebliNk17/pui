use super::*;
#[test]
fn foreign_popup_or_modal_keeps_side_chat_return_inert() {
    for modal in [false, true] {
        let ctx = egui::Context::default();
        let mut app = super::tests::app(&ctx);
        assert!(app.start_side_chat("keep side"));
        let side = app.agents.active.clone();
        for _ in 0..2 {
            super::tests::frame(&ctx, &mut app, vec![]);
        }
        let id = ctx
            .data(|data| data.get_temp::<egui::Id>(egui::Id::new("side-chat-switch")))
            .unwrap();
        ctx.memory_mut(|memory| memory.request_focus(id));
        if modal {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                prompt_ui::modal(ctx, egui::Id::new("foreign-side-chat-modal"), 220.0, |ui| {
                    prompt_ui::heading(ui, "Information", "Foreign owner")
                });
            });
        } else {
            egui::Popup::open_id(&ctx, egui::Id::new("foreign-side-chat-popup"));
        }
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
        assert_eq!(app.draft, "keep side");
    }
}
#[test]
fn disappearing_origin_never_retargets_return_to_other_chat() {
    let ctx = egui::Context::default();
    let mut app = super::tests::app(&ctx);
    let main = app.agents.active.clone();
    assert!(app.start_side_chat("keep"));
    let side = app.agents.active.clone();
    super::tests::frame(&ctx, &mut app, vec![]);
    let id = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new("side-chat-switch")))
        .unwrap();
    app.agents.parked.remove(&main);
    app.agents.nodes.remove(&main);
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
    assert_eq!(app.draft, "keep");
}
