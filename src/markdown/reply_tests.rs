use super::*;

const SOURCE: &str = "Intro **bold**, *italic*, [guide](https://example.test/guide), and `/tmp/demo/report.md`.\n\n3. **First** item with `src/demo.rs`, *emphasis*, and [linked `code`](https://example.test/code). A comfortable wrapping sentence keeps every word.\n\n   A second paragraph repeats `src/demo.rs` unchanged.\n\n   - Nested **child** and `src/demo.rs` with enough words to wrap in a narrow view.\n\n     8. Deep *ordered* child.\n     9. Another deep child.\n   - Second nested sibling.\n\n4. Second top-level item with digest `0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef`.\n\nAfter.\n\n- Plain bullet one.\n- Plain bullet two.\n\n| Key | Value |\n| --- | --- |\n| **Public** | [table link](https://example.test/table) |\n\n```rust\nlet demo = \"unchanged\";\n```\n\n> [!NOTE]\n> Keep **callout** and `code`.";

fn collect(
    shape: &egui::Shape,
    texts: &mut Vec<egui::epaint::TextShape>,
    lines: &mut Vec<[egui::Pos2; 2]>,
) {
    match shape {
        egui::Shape::Text(text) => texts.push(text.clone()),
        egui::Shape::LineSegment { points, .. } => lines.push(*points),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect(shape, texts, lines);
            }
        }
        _ => {}
    }
}

fn frame(
    ctx: &egui::Context,
    source: &str,
    width: f32,
    reply: bool,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, egui::Rect) {
    let mut rect = egui::Rect::NOTHING;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 6000.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                rect = ui
                    .scope(|ui| {
                        if reply {
                            show_reply(ui, source, "synthetic")
                        } else {
                            show(ui, source)
                        }
                    })
                    .response
                    .rect;
            });
        },
    );
    (output, rect)
}

fn shapes(output: &egui::FullOutput) -> (Vec<egui::epaint::TextShape>, Vec<[egui::Pos2; 2]>) {
    let (mut texts, mut lines) = (Vec::new(), Vec::new());
    for shape in &output.shapes {
        collect(&shape.shape, &mut texts, &mut lines);
    }
    (texts, lines)
}

#[test]
fn reply_style_preserves_nested_blocks_words_formats_baselines_and_bounded_geometry() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.4] {
            for width in [270.0, 760.0] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                // egui applies a requested zoom at the next input frame.
                frame(&ctx, SOURCE, width, true, vec![]);
                let (output, rect) = frame(&ctx, SOURCE, width, true, vec![]);
                let (texts, _) = shapes(&output);
                assert!(rect.is_finite());
                assert!(rect.right() <= ctx.screen_rect().right() + 1.0, "{rect:?}");
                let find = |needle: &str| {
                    texts
                        .iter()
                        .find(|text| text.galley.job.text.contains(needle))
                        .unwrap_or_else(|| panic!("Missing {needle:?}: {texts:?}"))
                };
                let first = find("3. First item");
                let second = find("4. Second top-level");
                let nested = find("•  Nested child");
                let deep = find("8. Deep ordered child.");
                let paragraph = find("A second paragraph");
                assert!(nested.pos.x > first.pos.x);
                assert!(
                    deep.pos.x > nested.pos.x,
                    "deep={:?}, nested={:?}, first={:?}, width={width}, zoom={zoom}",
                    deep.pos,
                    nested.pos,
                    first.pos
                );
                assert!(paragraph.pos.x > first.pos.x);
                assert_eq!(second.pos.x, first.pos.x);
                assert_eq!(find("9. Another deep child.").pos.x, deep.pos.x);
                assert!(paragraph.pos.y > first.pos.y + first.galley.size().y);
                assert!(second.pos.y > find("•  Second nested sibling.").pos.y);
                let displayed = texts
                    .iter()
                    .map(|text| text.galley.job.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                assert_eq!(displayed.matches("src/demo.rs").count(), 3);
                assert_eq!(displayed.matches("0123456789abcdef").count(), 5);
                for expected in [
                    "Intro bold, italic, guide, and /tmp/demo/report.md.",
                    "A second paragraph repeats src/demo.rs unchanged.",
                    "After.",
                    "Public",
                    "table link",
                    "let demo = \"unchanged\";",
                    "Keep callout and code.",
                ] {
                    assert!(displayed.contains(expected), "{expected:?}: {displayed}");
                }
                let section = |needle: &str| {
                    let text = find(needle);
                    text.galley
                        .job
                        .sections
                        .iter()
                        .find(|section| &text.galley.job.text[section.byte_range.clone()] == needle)
                        .unwrap()
                        .format
                        .clone()
                };
                assert_eq!(
                    section("First").font_id.family,
                    egui::FontFamily::Name("body-bold".into())
                );
                assert!(section("emphasis").italics);
                assert_eq!(section("code").color, mode.palette().accent);
                let code = section("src/demo.rs");
                assert_eq!(code.font_id.family, egui::FontFamily::Monospace);
                assert!(code.background.a() > 0 && code.background.a() <= 8);
                assert_eq!(code.line_height, Some(21.0));
                for text in &texts {
                    if !text.galley.job.align_baseline {
                        continue;
                    }
                    assert!(!text.galley.elided);
                    for row in &text.galley.rows {
                        if let Some(first) = row.glyphs.first() {
                            for glyph in &row.glyphs {
                                assert_eq!(glyph.pos.y, first.pos.y);
                                assert_eq!(
                                    glyph.logical_rect().y_range(),
                                    first.logical_rect().y_range()
                                );
                            }
                        }
                    }
                }
                if first.galley.rows.len() > 1 {
                    assert!(first.galley.rows[1].pos.x > 10.0);
                }
                let (again, again_rect) = frame(&ctx, SOURCE, width, true, vec![]);
                assert_eq!(rect, again_rect);
                assert_eq!(
                    texts
                        .iter()
                        .map(|text| (text.pos, text.galley.size()))
                        .collect::<Vec<_>>(),
                    shapes(&again)
                        .0
                        .iter()
                        .map(|text| (text.pos, text.galley.size()))
                        .collect::<Vec<_>>()
                );
            }
        }
    }
}

#[test]
fn reply_separators_only_between_top_level_ordered_siblings_and_tool_style_is_unchanged() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let source = "3. First `code`.\n   7. Deep one.\n   8. Deep two.\n4. Second.\n5. Third.\n\n- Bullet one.\n- Bullet two.\n\n1. Separate list.";
    let (output, _) = frame(&ctx, source, 500.0, true, vec![]);
    let (texts, lines) = shapes(&output);
    assert_eq!(
        lines.len(),
        2,
        "Only the existing ordered siblings get rules"
    );
    for line in lines {
        let second = texts
            .iter()
            .find(|text| text.galley.job.text == "4. Second.")
            .unwrap();
        assert!(line[0].y > texts[0].pos.y && line[0].y < texts.last().unwrap().pos.y);
        assert_eq!(line[0].x, second.pos.x);
    }
    let (plain, _) = frame(&ctx, source, 500.0, false, vec![]);
    let (texts, lines) = shapes(&plain);
    assert!(lines.is_empty());
    let code = texts[0]
        .galley
        .job
        .sections
        .iter()
        .find(|section| section.format.font_id.family == egui::FontFamily::Monospace)
        .unwrap();
    assert_eq!(code.format.line_height, Some(22.0));
    assert_eq!(code.format.background, theme::palette(&ctx).surface);
}

#[test]
fn reply_nested_link_hit_targets_include_wrapped_code_and_block_unsafe_schemes() {
    for (label, url) in [
        ("`/tmp/public.md`", "https://example.test/a"),
        (
            "a long linked label with unicode 世界 and more wrapped words",
            "https://example.test/b",
        ),
        (
            "English שלום linked label with more wrapped words",
            "https://example.test/mixed",
        ),
        (
            "שלום English linked label with more wrapped words",
            "https://example.test/rtl",
        ),
        ("unsafe", "javascript:bad"),
    ] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let source = format!("1. Parent\n   - [{label}]({url})");
        let (output, _) = frame(&ctx, &source, 250.0, true, vec![]);
        let (texts, _) = shapes(&output);
        let linked = texts
            .iter()
            .find(|text| text.galley.job.text.contains(label.trim_matches('`')))
            .unwrap();
        let row = linked.galley.rows.last().unwrap();
        let glyph = &row.glyphs[row.glyphs.len() - 2];
        let pos = linked.pos + row.pos.to_vec2() + glyph.logical_rect().center().to_vec2();
        let mut commands = Vec::new();
        for pressed in [true, false] {
            commands.extend(
                frame(
                    &ctx,
                    &source,
                    250.0,
                    true,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                )
                .0
                .platform_output
                .commands,
            );
        }
        assert_eq!(
            commands.iter().any(
                |command| matches!(command, egui::OutputCommand::OpenUrl(open) if open.url == url)
            ),
            safe_url(url),
            "{url}: {commands:?}"
        );
    }
}

#[test]
fn reply_deep_narrow_lists_and_block_first_items_keep_all_markers_and_words() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut source = String::new();
    for depth in 0..128 {
        source.push_str(&format!("{}- level{depth} literal\n", "  ".repeat(depth)));
    }
    let (output, rect) = frame(&ctx, &source, 190.0, true, vec![]);
    assert!(rect.is_finite());
    assert!(rect.width() <= 190.0, "{rect:?}");
    let (texts, _) = shapes(&output);
    assert_eq!(
        texts
            .iter()
            .filter(|text| text.galley.job.text.starts_with("•  "))
            .count(),
        128
    );
    for depth in 0..128 {
        assert!(
            texts
                .iter()
                .any(|text| text.galley.job.text == format!("•  level{depth} literal"))
        );
    }
    let source = "1. # Heading first\n\n2. ```rust\n   literal_code();\n   ```\n\n3. > [!NOTE]\n   > callout first\n\n4. | Key |\n   | --- |\n   | cell first |\n\n5. Last.";
    let (output, _) = frame(&ctx, source, 400.0, true, vec![]);
    let (texts, _) = shapes(&output);
    let all = texts
        .iter()
        .map(|text| text.galley.job.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for expected in [
        "1. ",
        "2. ",
        "3. ",
        "4. ",
        "5. Last.",
        "Heading first",
        "literal_code();",
        "callout first",
        "cell first",
    ] {
        assert!(all.contains(expected), "Missing {expected}: {all}");
    }
}

#[test]
fn reply_list_callout_keeps_nested_list_and_trailing_text_inside_one_frame() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let source = "1. > [!NOTE]\n   > Before\n   >\n   > - Child\n   >\n   > After\n\n2. Outside.";
    let (output, _) = frame(&ctx, source, 430.0, true, vec![]);
    let (texts, _) = shapes(&output);
    let bounds = |needle: &str| {
        let text = texts
            .iter()
            .find(|text| text.galley.job.text.contains(needle))
            .unwrap();
        egui::Rect::from_min_size(text.pos, text.galley.size())
    };
    fn frames(shape: &egui::Shape, out: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Rect(rect) if rect.fill != Color32::TRANSPARENT => out.push(rect.rect),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    frames(shape, out);
                }
            }
            _ => {}
        }
    }
    let mut rects = Vec::new();
    for shape in &output.shapes {
        frames(&shape.shape, &mut rects);
    }
    assert!(
        rects.iter().any(|rect| rect.contains_rect(bounds("Before"))
            && rect.contains_rect(bounds("Child"))
            && rect.contains_rect(bounds("After"))
            && !rect.contains_rect(bounds("Outside"))),
        "Callout subtree must keep its original frame"
    );
}

#[test]
fn reply_nested_wrapped_selection_keeps_one_label_and_native_copy_is_exact() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        mode.apply(&ctx);
        let source = "1. Parent\n   - Start **chosen 世界** with [linked words](https://example.test) and `src/demo.rs` then more wrapping words to retain.";
        let (output, _) = frame(&ctx, source, 260.0, true, vec![]);
        let (texts, _) = shapes(&output);
        let text = texts
            .iter()
            .find(|text| text.galley.job.text.starts_with("•  Start"))
            .unwrap();
        let chosen = "chosen 世界 with linked words and src/demo.rs";
        let byte = text.galley.job.text.find(chosen).unwrap();
        let start = text.galley.job.text[..byte].chars().count();
        let end = start + chosen.chars().count();
        let position = |index| {
            text.pos
                + text
                    .galley
                    .pos_from_cursor(egui::text::CCursor::new(index))
                    .center()
                    .to_vec2()
        };
        let (start, end) = (position(start), position(end));
        assert!(end.y > start.y, "Selection must span hanging rows");
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
        frame(&ctx, source, 260.0, true, pointer(start, true));
        frame(
            &ctx,
            source,
            260.0,
            true,
            vec![egui::Event::PointerMoved(end)],
        );
        let (release, _) = frame(&ctx, source, 260.0, true, pointer(end, false));
        assert!(
            !release
                .platform_output
                .commands
                .iter()
                .any(|command| matches!(command, egui::OutputCommand::OpenUrl(_)))
        );
        let (copied, _) = frame(&ctx, source, 260.0, true, vec![egui::Event::Copy]);
        assert!(
            copied.platform_output.commands.iter().any(
                |command| matches!(command, egui::OutputCommand::CopyText(value) if value == chosen)
            ),
            "{:?}",
            copied.platform_output.commands
        );
        for pressed in [true, false] {
            frame(
                &ctx,
                source,
                260.0,
                true,
                vec![
                    egui::Event::PointerMoved(start),
                    egui::Event::PointerButton {
                        pos: start,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        for _ in 0..3 {
            frame(&ctx, source, 260.0, true, vec![]);
        }
        assert!(
            ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
                .is_some(),
            "Nested selected text must offer the capture action"
        );
    }
}
