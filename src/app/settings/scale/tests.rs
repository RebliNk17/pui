use super::*;

fn frame(ctx: &egui::Context, scale: &mut Scale, events: Vec<egui::Event>) -> egui::Response {
    let mut response = None;
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1000.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    response = Some(scale.slider(ui));
                });
            });
        },
    );
    response.unwrap()
}

fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn drag_is_stationary_until_release_and_can_recover_from_maximum_scale() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        for (start, end) in [(1.0, 1.4), (1.4, 0.8)] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(start);
            let mut scale = Scale::default();
            for _ in 0..3 {
                frame(&ctx, &mut scale, vec![]);
            }
            let rect = frame(&ctx, &mut scale, vec![]).rect;
            // In the right-to-left settings row the rail is the rightmost 110pt.
            let pos = egui::pos2(rect.right() - 55.0, rect.center().y);
            frame(&ctx, &mut scale, pointer(pos, true));
            let end_pos = egui::pos2(
                if end > start {
                    rect.right()
                } else {
                    rect.right() - 110.0
                },
                pos.y,
            );
            for _ in 0..4 {
                let response = frame(&ctx, &mut scale, vec![egui::Event::PointerMoved(end_pos)]);
                assert_eq!(ctx.zoom_factor(), start, "Scale applied during drag");
                assert_eq!(response.rect, rect, "Slider moved during drag");
                assert_eq!(scale.pending, Some((end * 100.0).round() as u32));
            }
            // Releasing outside the slider still commits the last selected value.
            frame(
                &ctx,
                &mut scale,
                pointer(end_pos + egui::vec2(0.0, 100.0), false),
            );
            frame(&ctx, &mut scale, vec![]);
            assert_eq!(ctx.zoom_factor(), end);
            assert_eq!(scale.pending, None);
        }
    }
}

#[test]
fn dragging_the_percentage_field_also_waits_for_release() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut scale = Scale::default();
    for _ in 0..3 {
        frame(&ctx, &mut scale, vec![]);
    }
    let rect = frame(&ctx, &mut scale, vec![]).rect;
    let pos = egui::pos2(rect.left() + 20.0, rect.center().y);
    frame(&ctx, &mut scale, pointer(pos, true));
    let moved = pos + egui::vec2(30.0, 0.0);
    frame(&ctx, &mut scale, vec![egui::Event::PointerMoved(moved)]);
    frame(&ctx, &mut scale, vec![]);
    assert_eq!(ctx.zoom_factor(), 1.0);
    let pending = scale
        .pending
        .expect("Numeric drag must update the selection");
    assert!(pending > 100);
    frame(&ctx, &mut scale, pointer(moved, false));
    frame(&ctx, &mut scale, vec![]);
    assert_eq!(ctx.zoom_factor(), pending as f32 / 100.0);
}

#[test]
fn numeric_entry_uses_percent_units_and_waits_for_enter() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    ctx.set_zoom_factor(1.4);
    let mut scale = Scale::default();
    for _ in 0..3 {
        frame(&ctx, &mut scale, vec![]);
    }
    let rect = frame(&ctx, &mut scale, vec![]).rect;
    let pos = egui::pos2(rect.left() + 10.0, rect.center().y);
    frame(&ctx, &mut scale, pointer(pos, true));
    frame(&ctx, &mut scale, pointer(pos, false));
    frame(&ctx, &mut scale, vec![egui::Event::Text("100".into())]);
    frame(&ctx, &mut scale, vec![]);
    assert_eq!(ctx.zoom_factor(), 1.4, "Typing must not resize the editor");
    frame(&ctx, &mut scale, vec![key(egui::Key::Enter)]);
    frame(&ctx, &mut scale, vec![]);
    assert_eq!(ctx.zoom_factor(), 1.0);
}

#[test]
fn keyboard_adjustments_commit_without_a_pointer_gesture() {
    for (start, expected) in [(1.0, 0.95), (1.4, 1.35)] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.set_zoom_factor(start);
        let mut scale = Scale::default();
        for _ in 0..3 {
            frame(&ctx, &mut scale, vec![]);
        }
        let response = frame(&ctx, &mut scale, vec![]);
        ctx.memory_mut(|m| m.request_focus(response.id));
        frame(&ctx, &mut scale, vec![key(egui::Key::ArrowLeft)]);
        frame(&ctx, &mut scale, vec![]);
        assert_eq!(ctx.zoom_factor(), expected);
    }
}

#[test]
fn reset_button_restores_actual_size_from_140_percent() {
    fn label(shape: &egui::Shape) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(t) if t.galley.job.text == "Reset to 100%" => {
                Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
            }
            egui::Shape::Vec(v) => v.iter().find_map(label),
            _ => None,
        }
    }
    let ctx = egui::Context::default();
    theme::install(&ctx);
    ctx.set_zoom_factor(1.4);
    let mut scale = Scale::default();
    let mut render = |events| {
        ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| scale.show(ui));
            },
        )
    };
    for _ in 0..3 {
        render(vec![]);
    }
    let output = render(vec![]);
    let pos = output
        .shapes
        .iter()
        .find_map(|s| label(&s.shape))
        .unwrap()
        .center();
    render(pointer(pos, true));
    render(pointer(pos, false));
    render(vec![]);
    assert_eq!(ctx.zoom_factor(), 1.0);
    assert_eq!(scale.pending, None);
}
