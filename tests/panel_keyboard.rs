use eframe::egui::*;
fn draw(ctx: &Context, right: bool, events: Vec<Event>) -> Rect {
    let mut rect = Rect::NOTHING;
    let _ = ctx.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0))),
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
            let panel = if right {
                SidePanel::right("keyboard-panel")
            } else {
                SidePanel::left("keyboard-panel")
            };
            rect = panel
                .default_width(250.0)
                .width_range(220.0..=480.0)
                .show(ctx, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.button("Panel content").on_hover_text("Focus neighbor");
                })
                .response
                .rect;
        },
    );
    rect
}
fn arrow(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}
#[test]
fn focused_panel_keyboard_resizes_both_sides_and_respects_limits() {
    for right in [false, true] {
        let ctx = Context::default();
        let before = draw(&ctx, right, vec![]);
        ctx.memory_mut(|memory| memory.request_focus(Id::new("keyboard-panel").with("__resize")));
        let after = draw(
            &ctx,
            right,
            vec![arrow(
                if right {
                    Key::ArrowLeft
                } else {
                    Key::ArrowRight
                },
                Modifiers::NONE,
            )],
        );
        assert!(
            (after.width() - before.width() - 8.0).abs() < 1.0,
            "right={right} before={before:?} after={after:?}"
        );
        let after = draw(
            &ctx,
            right,
            vec![arrow(
                if right {
                    Key::ArrowRight
                } else {
                    Key::ArrowLeft
                },
                Modifiers::SHIFT,
            )],
        );
        assert!((after.width() - 226.0).abs() < 1.0);
        let after = draw(
            &ctx,
            right,
            vec![arrow(
                if right {
                    Key::ArrowRight
                } else {
                    Key::ArrowLeft
                },
                Modifiers::SHIFT,
            )],
        );
        assert_eq!(after.width(), 220.0);
    }
}
#[test]
fn native_tab_reaches_edge_arrows_keep_focus_and_shift_tab_leaves() {
    let ctx = Context::default();
    for _ in 0..2 {
        draw(&ctx, false, vec![]);
    }
    draw(&ctx, false, vec![arrow(Key::Tab, Modifiers::NONE)]);
    draw(&ctx, false, vec![arrow(Key::Tab, Modifiers::NONE)]);
    let id = Id::new("keyboard-panel").with("__resize");
    assert!(ctx.memory(|memory| memory.has_focus(id)));
    draw(&ctx, false, vec![arrow(Key::ArrowRight, Modifiers::NONE)]);
    assert!(ctx.memory(|memory| memory.has_focus(id)));
    draw(&ctx, false, vec![arrow(Key::Tab, Modifiers::SHIFT)]);
    draw(&ctx, false, vec![]);
    assert!(!ctx.memory(|memory| memory.has_focus(id)));
}
#[test]
fn theme_zoom_maximum_and_modified_arrows_remain_bounded() {
    for dark in [false, true] {
        for zoom in [0.8, 1.4] {
            let ctx = Context::default();
            ctx.set_visuals(if dark {
                Visuals::dark()
            } else {
                Visuals::light()
            });
            ctx.set_zoom_factor(zoom);
            draw(&ctx, false, vec![]);
            let id = Id::new("keyboard-panel").with("__resize");
            ctx.memory_mut(|memory| memory.request_focus(id));
            let max = draw(
                &ctx,
                false,
                (0..20)
                    .map(|_| arrow(Key::ArrowRight, Modifiers::SHIFT))
                    .collect(),
            );
            assert_eq!(max.width(), 480.0);
            let unchanged = draw(&ctx, false, vec![arrow(Key::ArrowLeft, Modifiers::COMMAND)]);
            assert_eq!(unchanged.width(), 480.0);
        }
    }
}
#[test]
fn unfocused_or_foreign_popup_cannot_resize() {
    let ctx = Context::default();
    let before = draw(&ctx, false, vec![]);
    assert_eq!(
        draw(&ctx, false, vec![arrow(Key::ArrowRight, Modifiers::NONE)]).width(),
        before.width()
    );
    ctx.memory_mut(|memory| memory.request_focus(Id::new("keyboard-panel").with("__resize")));
    Popup::open_id(&ctx, Id::new("foreign"));
    assert_eq!(
        draw(&ctx, false, vec![arrow(Key::ArrowRight, Modifiers::NONE)]).width(),
        before.width()
    );
}
