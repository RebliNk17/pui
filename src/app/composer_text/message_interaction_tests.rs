use super::*;
use std::sync::{Mutex, atomic::AtomicUsize};

fn collect(
    shape: &egui::Shape,
    texts: &mut Vec<egui::epaint::TextShape>,
    code: &mut Vec<egui::Rect>,
    fill: Color32,
) {
    match shape {
        egui::Shape::Text(t) => texts.push(t.clone()),
        egui::Shape::Rect(r) if r.fill == fill => code.push(r.rect),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, texts, code, fill)),
        _ => {}
    }
}

#[test]
fn sent_label_paints_code_without_fence_row_and_preserves_find_and_copy_source() {
    let source = "שלום **עולם**\nEnglish `inline`\n```text\nbody <IP>:443\n```";
    for dark in [false, true] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.set_visuals(if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        let found = Arc::new(Mutex::new(None));
        let render = |events: Vec<egui::Event>, find: bool| {
            markdown::set_find_highlight(
                &ctx,
                find.then(|| markdown::FindHighlight {
                    query: "body".into(),
                    active_in_scope: Some(0),
                    seen: Arc::new(AtomicUsize::new(0)),
                    active_rect: found.clone(),
                }),
            );
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(400.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| show_message(ui, source));
                },
            )
        };
        for _ in 0..3 {
            render(vec![], false);
        }
        let out = render(vec![], true);
        let (mut texts, mut backgrounds) = (vec![], vec![]);
        for shape in &out.shapes {
            collect(
                &shape.shape,
                &mut texts,
                &mut backgrounds,
                theme::palette(&ctx).code_bg,
            );
        }
        let text = texts.iter().find(|t| t.galley.job.text == source).unwrap();
        let body_index = source[..source.find("body").unwrap()].chars().count();
        let expected = text
            .galley
            .pos_from_cursor(egui::text::CCursor::new(body_index))
            .translate(text.pos.to_vec2());
        assert_eq!(found.lock().unwrap().unwrap(), expected);
        assert!(
            text.galley
                .job
                .sections
                .iter()
                .any(|s| s.format.background != Color32::TRANSPARENT)
        );
        let row = text
            .galley
            .rows
            .iter()
            .find(|row| row.text().contains("body"))
            .unwrap();
        assert_eq!(backgrounds.len(), 1);
        assert!((backgrounds[0].top() - text.pos.y - row.pos.y).abs() < 0.1);
        assert!((backgrounds[0].height() - row.size.y).abs() < 0.1);
        let point = |index| {
            text.pos
                + text
                    .galley
                    .pos_from_cursor(egui::text::CCursor::new(index))
                    .center()
                    .to_vec2()
        };
        let pointer = |pos, pressed| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        // Select the visible code body. Clipboard must contain source characters,
        // never zero-width presentation replacements or escaped HTML.
        let start = point(body_index) + egui::vec2(0.1, 0.0);
        let end_index = body_index + "body <IP>:443".chars().count();
        let end = point(end_index) - egui::vec2(0.1, 0.0);
        render(pointer(start, true), false);
        render(vec![egui::Event::PointerMoved(end)], false);
        render(pointer(end, false), false);
        let copy = render(vec![egui::Event::Copy], false);
        assert!(
            copy.platform_output.commands.iter().any(
                |c| matches!(c, egui::OutputCommand::CopyText(value) if value == "body <IP>:443")
            ),
            "{:?}",
            copy.platform_output.commands
        );
    }
}
