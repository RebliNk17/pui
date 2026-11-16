use eframe::egui::*;
fn draw(ctx: &Context, bottom: bool, events: Vec<Event>) -> Rect {
    let mut rect = Rect::NOTHING;
    let _ = ctx.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 650.0))),
            modifiers: events
                .iter()
                .find_map(|event| {
                    if let Event::Key { modifiers, .. } = event {
                        Some(*modifiers)
                    } else {
                        None
                    }
                })
                .unwrap_or_default(),
            events,
            ..Default::default()
        },
        |ctx| {
            let panel = if bottom {
                TopBottomPanel::bottom("vertical-panel")
            } else {
                TopBottomPanel::top("vertical-panel")
            };
            rect = panel
                .resizable(true)
                .default_height(250.0)
                .height_range(150.0..=400.0)
                .show(ctx, |ui| {
                    ui.set_min_height(ui.available_height());
                    let _ = ui.button("Neighbor");
                })
                .response
                .rect;
        },
    );
    rect
}
fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}
#[test]
fn top_bottom_arrows_steps_clamps_and_focus_are_native() {
    for bottom in [false, true] {
        for dark in [false, true] {
            let ctx = Context::default();
            ctx.set_visuals(if dark {
                Visuals::dark()
            } else {
                Visuals::light()
            });
            draw(&ctx, bottom, vec![]);
            draw(&ctx, bottom, vec![key(Key::Tab, Modifiers::NONE)]);
            draw(&ctx, bottom, vec![key(Key::Tab, Modifiers::NONE)]);
            let id = Id::new("vertical-panel").with("__resize");
            assert!(ctx.memory(|memory| memory.has_focus(id)));
            let grown = draw(
                &ctx,
                bottom,
                vec![key(
                    if bottom { Key::ArrowUp } else { Key::ArrowDown },
                    Modifiers::SHIFT,
                )],
            );
            assert_eq!(grown.height(), 282.0);
            assert!(ctx.memory(|memory| memory.has_focus(id)));
            let small = draw(
                &ctx,
                bottom,
                (0..30)
                    .map(|_| {
                        key(
                            if bottom { Key::ArrowDown } else { Key::ArrowUp },
                            Modifiers::NONE,
                        )
                    })
                    .collect(),
            );
            assert_eq!(small.height(), 150.0);
            draw(&ctx, bottom, vec![key(Key::Tab, Modifiers::SHIFT)]);
            draw(&ctx, bottom, vec![]);
            assert!(!ctx.memory(|memory| memory.has_focus(id)));
        }
    }
}
#[test]
fn vertical_maximum_zoom_and_modified_arrows_are_bounded() {
    for zoom in [0.8, 1.4] {
        let ctx = Context::default();
        ctx.set_zoom_factor(zoom);
        draw(&ctx, true, vec![]);
        ctx.memory_mut(|memory| memory.request_focus(Id::new("vertical-panel").with("__resize")));
        let max = draw(
            &ctx,
            true,
            (0..20)
                .map(|_| key(Key::ArrowUp, Modifiers::SHIFT))
                .collect(),
        );
        assert_eq!(max.height(), 400.0);
        assert_eq!(
            draw(&ctx, true, vec![key(Key::ArrowDown, Modifiers::COMMAND)]).height(),
            400.0
        );
    }
}
#[test]
fn foreign_popup_has_priority_over_vertical_resize() {
    let ctx = Context::default();
    let before = draw(&ctx, true, vec![]);
    ctx.memory_mut(|memory| memory.request_focus(Id::new("vertical-panel").with("__resize")));
    Popup::open_id(&ctx, Id::new("foreign-vertical"));
    assert_eq!(
        draw(&ctx, true, vec![key(Key::ArrowUp, Modifiers::NONE)]).height(),
        before.height()
    );
}
