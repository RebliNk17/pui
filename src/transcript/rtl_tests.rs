use super::*;

fn collect(
    shape: &egui::Shape,
    fill: egui::Color32,
    bubbles: &mut Vec<Rect>,
    texts: &mut Vec<egui::epaint::TextShape>,
) {
    match shape {
        egui::Shape::Rect(r) if r.fill == fill => bubbles.push(r.rect),
        egui::Shape::Text(t) => texts.push(t.clone()),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, fill, bubbles, texts)),
        _ => {}
    }
}

#[test]
fn rtl_user_bubbles_use_the_inner_right_edge_and_keep_find_and_copy_logical() {
    for mode in [crate::theme::Theme::Light, crate::theme::Theme::Dark] {
        for source in [
            "שלום",
            "שלום עולם שלום עולם שלום עולם שלום עולם שלום עולם\nEnglish 123\nשלום",
        ] {
            for with_image in [false, true] {
                let ctx = egui::Context::default();
                crate::theme::install(&ctx);
                mode.apply(&ctx);
                let message = Message {
                    role: "user".into(),
                    text: source.into(),
                    timestamp: 60000,
                    images: if with_image {
                        vec![
                            crate::media::Attachment::from_bytes(include_bytes!(
                                "../../assets/icon.png"
                            ))
                            .unwrap(),
                        ]
                    } else {
                        vec![]
                    },
                    ..Default::default()
                };
                let mut gallery = crate::media::Gallery::default();
                let active_rect = Arc::new(Mutex::new(None));
                let mut render = |events, find: bool| {
                    markdown::set_find_highlight(
                        &ctx,
                        find.then(|| markdown::FindHighlight {
                            query: "שלום".into(),
                            active_in_scope: Some(0),
                            seen: Arc::new(AtomicUsize::new(0)),
                            active_rect: active_rect.clone(),
                        }),
                    );
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(420.0, 700.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                user_actions(ui, &message, &mut gallery, true);
                            });
                        },
                    )
                };
                for _ in 0..4 {
                    render(vec![], false);
                }
                let output = render(vec![], true);
                let (mut bubbles, mut texts) = (vec![], vec![]);
                for shape in &output.shapes {
                    collect(
                        &shape.shape,
                        crate::theme::palette(&ctx).user_bg,
                        &mut bubbles,
                        &mut texts,
                    );
                }
                let bubble = bubbles[0];
                let text = texts
                    .iter()
                    .find(|text| text.galley.job.text == source)
                    .unwrap();
                assert_eq!(text.galley.job.text, source);
                for row in &text.galley.rows {
                    if row.bidi.as_ref().is_some_and(|b| b.base_rtl) {
                        assert!(
                            (text.pos.x + row.rect().right() - (bubble.right() - 17.0)).abs() < 0.1
                        );
                    } else if !row.glyphs.is_empty() {
                        assert!((text.pos.x + row.pos.x - (bubble.left() + 17.0)).abs() < 0.1);
                    }
                }
                let position = |index| {
                    text.pos
                        + text
                            .galley
                            .pos_from_cursor(egui::text::CCursor::new(index))
                            .center()
                            .to_vec2()
                };
                let expected = text
                    .galley
                    .pos_from_cursor(egui::text::CCursor::new(0))
                    .translate(text.pos.to_vec2());
                assert_eq!(active_rect.lock().unwrap().unwrap(), expected);
                let pointer = |pos, pressed| {
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]
                };
                // Stay inside the label's half-open hit rectangle, while remaining
                // in the first/last glyph's logical boundary half.
                let start = position(0) - egui::vec2(1.5, 0.0);
                let end = position(source.chars().count()) + egui::vec2(1.5, 0.0);
                render(pointer(start, true), false);
                render(vec![egui::Event::PointerMoved(end)], false);
                render(pointer(end, false), false);
                let copied = render(vec![egui::Event::Copy], false);
                assert!(
                    copied.platform_output.commands.iter().any(
                        |c| matches!(c, egui::OutputCommand::CopyText(value) if value == source)
                    ),
                    "source={source:?}, image={with_image}, theme={mode:?}, start={start:?}, end={end:?}: {:?}",
                    copied.platform_output.commands
                );
            }
        }
    }
}
