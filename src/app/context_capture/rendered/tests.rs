use super::*;

#[test]
fn selected_rendered_unicode_is_exact_bounded_and_not_source_coordinates() {
    let text = "before שלום 世界 e\u{301} after";
    let begin = "before ".chars().count();
    let end = begin + "שלום 世界 e\u{301}".chars().count();
    let captured = selected(text, begin..end, "Assistant reply").unwrap();
    assert_eq!(captured.text, "שלום 世界 e\u{301}");
    assert_eq!(
        captured.label,
        "Assistant reply · selected rendered text (not Markdown source)"
    );
    assert_eq!(selected(text, 0..1, "reply").unwrap().text, "b");
    assert_eq!(
        selected(text, 0..text.chars().count(), "reply")
            .unwrap()
            .text,
        text
    );
    assert!(selected(text, 0..1000, "reply").is_none());
    assert!(selected(text, 5..5, "reply").is_none());
    let text = "界".repeat(200_000);
    let start = std::time::Instant::now();
    let captured = selected(&text, 10..190_000, "reply").unwrap();
    eprintln!(
        "Rendered selection: {} retained bytes in {:?}",
        captured.text.len(),
        start.elapsed()
    );
    assert!(captured.text.len() > MAX_BYTES && captured.text.len() <= MAX_BYTES + 4);
}

pub(super) fn pointer(
    position: egui::Pos2,
    button: egui::PointerButton,
    pressed: bool,
) -> Vec<egui::Event> {
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
pub(super) fn text<'a>(output: &'a egui::FullOutput, needle: &str) -> &'a egui::epaint::TextShape {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.contains(needle) => Some(text),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing {needle}"))
}
pub(super) fn position(output: &egui::FullOutput, needle: &str, character: usize) -> egui::Pos2 {
    let text = text(output, needle);
    text.pos
        + text
            .galley
            .pos_from_cursor(egui::text::CCursor::new(character))
            .center()
            .to_vec2()
}
pub(super) fn range_in(text: &str, selected: &str) -> std::ops::Range<usize> {
    let byte = text.find(selected).unwrap();
    text[..byte].chars().count()..text[..byte + selected.len()].chars().count()
}

#[test]
fn backward_unicode_link_selection_quotes_without_opening_url_or_merging_another_reply() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let draw = |events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(620.0, 600.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                context_capture::inputs(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    markdown::show_reply(
                        ui,
                        "Prefix [世界 e\u{301}](https://example.test) suffix",
                        "one",
                    );
                    markdown::show_reply(ui, "Other reply must not be captured", "two");
                });
            },
        )
    };
    for _ in 0..4 {
        draw(vec![]);
    }
    let output = draw(vec![]);
    let needle = "Prefix 世界 e\u{301} suffix";
    let range = range_in(needle, "世界 e\u{301}");
    let start = position(&output, needle, range.end);
    let end = position(&output, needle, range.start);
    draw(pointer(start, egui::PointerButton::Primary, true));
    draw(vec![egui::Event::PointerMoved(end)]);
    draw(pointer(end, egui::PointerButton::Primary, false));
    let mut commands = Vec::new();
    for pressed in [true, false] {
        commands.extend(
            draw(pointer(end, egui::PointerButton::Secondary, pressed))
                .platform_output
                .commands,
        );
    }
    for _ in 0..3 {
        draw(vec![]);
    }
    let target = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        commands.extend(
            draw(pointer(target, egui::PointerButton::Primary, pressed))
                .platform_output
                .commands,
        );
    }
    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, egui::OutputCommand::OpenUrl(_)))
    );
    let mut app = crate::app::files::tests::app(&ctx);
    app.collect_context_capture();
    assert!(app.draft.ends_with("\n世界 e\u{301}\n[/Context]"));
    assert!(!app.draft.contains("Other reply"));
}

#[test]
fn native_demo_fixture_has_explicit_synthetic_snapshot_and_no_provider() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_quote_selection: true,
            ..Default::default()
        },
    );
    for _ in 0..4 {
        crate::app::files::tests::frame(&ctx, &mut app, vec![]);
    }
    assert!(egui::Popup::is_any_open(&ctx));
    assert!(app.client.is_none());
    assert_eq!(app.draft, "Keep this draft");
    let target = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        crate::app::files::tests::frame(
            &ctx,
            &mut app,
            pointer(target, egui::PointerButton::Primary, pressed),
        );
    }
    assert!(app.draft.ends_with("\nchosen reply\n[/Context]"));
}

#[test]
fn actual_rendered_paragraph_code_cell_and_alert_quote_only_displayed_selection() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for (markdown, needle, chosen) in [
            (
                "Prefix **bold** [link](https://example.test) `code` suffix",
                "Prefix bold link code suffix",
                "bold link code",
            ),
            (
                "```rust\nlet selected = \"世界\";\nnever_run();\n```",
                "let selected",
                "selected = \"世界\"",
            ),
            (
                "| Head | Other |\n| --- | --- |\n| Selected **cell** | Not this |",
                "Selected cell",
                "Selected cell",
            ),
            (
                "> [!NOTE]\n> Keep **chosen** text here.",
                "Keep chosen text here.",
                "chosen text",
            ),
        ] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            let draw = |events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(620.0, 600.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        context_capture::inputs(ctx);
                        egui::CentralPanel::default()
                            .show(ctx, |ui| markdown::show_reply(ui, markdown, "message-1"));
                    },
                )
            };
            for _ in 0..4 {
                draw(vec![]);
            }
            let output = draw(vec![]);
            let rendered = &text(&output, needle).galley.job.text;
            let range = range_in(rendered, chosen);
            let start = position(&output, needle, range.start);
            let end = position(&output, needle, range.end);
            draw(pointer(start, egui::PointerButton::Primary, true));
            draw(vec![egui::Event::PointerMoved(end)]);
            draw(pointer(end, egui::PointerButton::Primary, false));
            let mut commands = Vec::new();
            for pressed in [true, false] {
                commands.extend(
                    draw(pointer(start, egui::PointerButton::Secondary, pressed))
                        .platform_output
                        .commands,
                );
            }
            for _ in 0..3 {
                draw(vec![]);
            }
            let target = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
                .unwrap()
                .center();
            for pressed in [true, false] {
                commands.extend(
                    draw(pointer(target, egui::PointerButton::Primary, pressed))
                        .platform_output
                        .commands,
                );
            }
            assert!(
                !commands
                    .iter()
                    .any(|command| matches!(command, egui::OutputCommand::OpenUrl(_)))
            );
            let mut app = crate::app::files::tests::app(&ctx);
            app.draft = "Keep draft".into();
            app.state.busy = true;
            let messages = app.state.messages.len();
            app.collect_context_capture();
            assert_eq!(
                app.draft,
                format!(
                    "Keep draft\n\n[Context: Assistant reply · selected rendered text (not Markdown source)]\n{chosen}\n[/Context]"
                )
            );
            assert!(app.state.busy);
            assert_eq!(app.state.messages.len(), messages);
            let draft = app.draft.clone();
            app.collect_context_capture();
            assert_eq!(app.draft, draft);
        }
    }
}
