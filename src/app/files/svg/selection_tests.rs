use super::*;
fn pointer(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
#[test]
fn svg_source_real_unicode_drag_copy_and_frozen_capture_is_exact_draft_only() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let source =
        "<svg width='16' height='8'>\r\n<!-- chosen 世界 e\u{301} -->\r\n</svg>".to_owned();
    let mut preview = Preview {
        retained: source.capacity(),
        source: source.clone(),
        pixels: None,
        error: Some("Source fixture".into()),
        controls: [None; 3],
        keyboard_pass: None,
    };
    let mut texture = None;
    let mut line = None;
    let mut notice = None;
    let mut draw = |events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(620.0, 540.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                context_capture::inputs(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    preview.show(
                        ui,
                        Path::new("selection.svg"),
                        true,
                        &mut texture,
                        &mut line,
                        &mut notice,
                    );
                    preview.finish_inputs(ctx);
                });
            },
        )
    };
    for _ in 0..3 {
        draw(vec![]);
    }
    let output = draw(vec![]);
    let chosen = "chosen 世界 e\u{301}";
    let text = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == source => Some(text),
            _ => None,
        })
        .unwrap();
    let index = source[..source.find(chosen).unwrap()].chars().count();
    let at = |index| {
        text.pos
            + text
                .galley
                .pos_from_cursor(egui::text::CCursor::new(index))
                .center()
                .to_vec2()
    };
    let start = at(index);
    let end = at(index + chosen.chars().count());
    draw(pointer(start, egui::PointerButton::Primary, true));
    draw(vec![egui::Event::PointerMoved(end)]);
    draw(pointer(end, egui::PointerButton::Primary, false));
    let copied = draw(vec![egui::Event::Copy]);
    assert!(
        copied
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command,egui::OutputCommand::CopyText(text) if text==chosen))
    );
    for pressed in [true, false] {
        draw(pointer(start, egui::PointerButton::Secondary, pressed));
    }
    for _ in 0..3 {
        draw(vec![]);
    }
    let menu = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        draw(pointer(menu, egui::PointerButton::Primary, pressed));
    }
    let mut app = crate::app::files::tests::app(&ctx);
    app.draft = "Kept draft".into();
    app.context_chips.clear_quotes();
    app.state.busy = true;
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.collect_context_capture();
    assert_eq!(
        app.outgoing_text(),
        format!(
            "Kept draft\n\n[Context: selection.svg:2–2 · selected source]\n{chosen}\n[/Context]"
        )
    );
    app.collect_context_capture();
    assert_eq!(app.outgoing_text().matches("[Context:").count(), 1);
    assert!(commands.try_recv().is_err());
    assert!(app.state.busy);
    assert_eq!(preview.source, source);
}
