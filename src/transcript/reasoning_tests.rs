use super::*;

fn text_shapes(shape: &egui::Shape, rows: &mut Vec<(String, Rect)>) {
    match shape {
        egui::Shape::Text(t) => rows.push((
            t.galley.job.text.clone(),
            t.galley.rect.translate(t.pos.to_vec2()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                text_shapes(shape, rows);
            }
        }
        _ => {}
    }
}

#[test]
fn bounded_reasoning_expands_independently_and_preserves_streaming_state() {
    for mode in [crate::theme::Theme::Light, crate::theme::Theme::Dark] {
        for width in [320.0, 800.0] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            mode.apply(&ctx);
            let source = format!("{}\n\nTAIL_MARKER", "Reasoning 世界. ".repeat(180));
            let draw = |events, source: &str| {
                let mut changed = false;
                let mut height = 0.0;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, 5000.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let top = ui.cursor().top();
                            changed = thinking_text(ui, source, "first");
                            height = ui.cursor().top() - top;
                            thinking_text(ui, &"second block ".repeat(100), "second");
                        });
                    },
                );
                let mut rows = Vec::new();
                for shape in output.shapes {
                    text_shapes(&shape.shape, &mut rows);
                }
                (changed, height, rows)
            };
            draw(vec![], &source);
            let (_, collapsed, rows) = draw(vec![], &source);
            assert!(
                collapsed < 210.0,
                "preview must remain bounded: {collapsed}"
            );
            assert!(!rows.iter().any(|(s, _)| s.contains("TAIL_MARKER")));
            let pos = rows
                .iter()
                .find(|(s, _)| s == "Show more")
                .unwrap()
                .1
                .center();
            let pointer = |pressed| {
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
            draw(pointer(true), &source);
            assert!(
                draw(pointer(false), &source).0,
                "must invalidate turn height"
            );
            let (_, expanded, rows) = draw(vec![], &source);
            assert!(expanded > collapsed);
            assert!(rows.iter().any(|(s, _)| s.contains("TAIL_MARKER")));
            assert_eq!(
                rows.iter().filter(|(s, _)| s == "Show more").count(),
                1,
                "second block stays bounded"
            );
            let grown = format!("{source}\nSTREAMED_END");
            let (changed, _, rows) = draw(vec![], &grown);
            assert!(!changed, "idle expanded state must settle");
            assert!(rows.iter().any(|(s, _)| s.contains("STREAMED_END")));
            let pos = rows
                .iter()
                .find(|(s, _)| s == "Show less")
                .unwrap()
                .1
                .center();
            let pointer = |pressed| {
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
            draw(pointer(true), &grown);
            assert!(draw(pointer(false), &grown).0);
            assert!(draw(vec![], &grown).1 < 210.0);
        }
    }
}

#[test]
fn structured_reasoning_is_provider_independent_and_plain_reply_is_not_reclassified() {
    for (provider, api, model) in [
        ("deepseek", "openai-completions", "deepseek-v4-flash"),
        ("anthropic", "anthropic-messages", "claude-sonnet"),
        ("openai", "openai-responses", "gpt"),
    ] {
        let reasoning = "provider reasoning ".repeat(120);
        let raw = serde_json::json!({"role":"assistant","provider":provider,"api":api,"model":model,
            "content":[{"type":"thinking","thinking":reasoning},{"type":"text","text":"FINAL_ANSWER"}]});
        let messages = crate::protocol::parse_history(&[raw]);
        assert!(matches!(
            &messages[0].blocks[..],
            [Block::Thinking(_), Block::Text(_)]
        ));
        assert_eq!(messages[0].text, "FINAL_ANSWER");
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut gallery = crate::media::Gallery::default();
        for running in [true, false] {
            for view in [View::Normal, View::Verbose] {
                let mut rows = Vec::new();
                for _ in 0..2 {
                    let output = ctx.run(egui::RawInput::default(), |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            assistant(
                                ui,
                                &messages,
                                0,
                                model,
                                "off",
                                (running, false, false, view),
                                false,
                                &mut gallery,
                                None,
                                &std::sync::Arc::new(std::sync::Mutex::new(None)),
                            );
                        });
                    });
                    rows.clear();
                    for shape in output.shapes {
                        text_shapes(&shape.shape, &mut rows);
                    }
                }
                assert_eq!(
                    rows.iter().any(|(s, _)| s == "Thinking"),
                    view == View::Verbose,
                    "{provider}"
                );
                assert_eq!(
                    rows.iter().any(|(s, _)| s == "Show more"),
                    view == View::Verbose,
                    "{provider}"
                );
                if view == View::Normal {
                    assert!(
                        !rows.iter().any(|(s, _)| s.contains("provider reasoning")),
                        "{provider}"
                    );
                }
                assert!(
                    rows.iter().any(|(s, _)| s.contains("FINAL_ANSWER")),
                    "{provider}"
                );
            }
        }
        let plain =
            crate::protocol::parse_history(&[serde_json::json!({"role":"assistant","model":model,
            "content":[{"type":"text","text":"Let me think. This is normal reply text."}]})]);
        assert!(matches!(&plain[0].blocks[..], [Block::Text(_)]));
        assert!(plain[0].thinking.is_empty());
    }
}
