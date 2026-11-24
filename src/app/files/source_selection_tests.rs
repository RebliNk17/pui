use super::*;

fn point(output: &egui::FullOutput, line: &str, character: usize) -> egui::Pos2 {
    let text = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == line => Some(text),
            _ => None,
        })
        .unwrap();
    text.pos
        + text
            .galley
            .pos_from_cursor(egui::text::CCursor::new(character))
            .center()
            .to_vec2()
}
fn pointer(position: egui::Pos2, button: egui::PointerButton, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(position),
        egui::Event::PointerButton {
            pos: position,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
#[test]
fn delimited_source_selected_characters_keep_exact_line_origin_and_skip_row_number() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let preview =
        delimited::Preview::new("first,row\r\nAlpha,\"chosen\"\r\nlast,row\r\n".into(), b',');
    let draw = |events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(500.0, 400.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                context_capture::inputs(ctx);
                egui::CentralPanel::default()
                    .show(ctx, |ui| preview.show_source(ui, Path::new("selected.csv")));
            },
        )
    };
    for _ in 0..3 {
        draw(vec![]);
    }
    let output = draw(vec![]);
    let line = "Alpha,\"chosen\"";
    let start = point(&output, line, 7);
    let end = point(&output, line, 13);
    draw(pointer(start, egui::PointerButton::Primary, true));
    draw(vec![egui::Event::PointerMoved(end)]);
    draw(pointer(end, egui::PointerButton::Primary, false));
    for pressed in [true, false] {
        draw(pointer(start, egui::PointerButton::Secondary, pressed));
    }
    for _ in 0..3 {
        draw(vec![]);
    }
    let target = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        draw(pointer(target, egui::PointerButton::Primary, pressed));
    }
    let mut app = tests::app(&ctx);
    app.draft = "Kept".into();
    app.context_chips.clear_quotes();
    app.collect_context_capture();
    assert_eq!(
        app.outgoing_text(),
        "Kept\n\n[Context: selected.csv:2–2 · selected source]\nchosen\n[/Context]"
    );
    assert_eq!(
        preview.source.as_str(),
        "first,row\r\nAlpha,\"chosen\"\r\nlast,row\r\n"
    );
}
