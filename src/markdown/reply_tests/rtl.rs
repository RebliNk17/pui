use super::*;

fn right(text: &egui::epaint::TextShape) -> f32 {
    text.pos.x + text.galley.rect.right()
}

#[test]
fn rtl_prose_headings_and_lists_follow_the_right_edge_at_all_sizes() {
    let source = "# שלום עולם\n\nשלום **עולם** עם `DNS 8.8.8.8:53` וגם [קישור](https://example.test) ועוד מילים כדי לבדוק גלישה של שורות בעברית.\n\nEnglish שלום stays left.\n\n3. שלום **עולם** ועוד מילים בעברית כדי לבדוק גלישה לשורה הבאה ולהשאיר מספר בצד ימין.\n\n   פסקה נוספת בעברית.\n\n   - פריט מקונן בעברית עם עוד מילים כדי לבדוק גלישה.\n4. פריט שני.\n\n```text\nשלום code 123\n```\n\nمرحبا بالعالم";
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.4] {
            for width in [270.0, 760.0] {
                for reply in [true, false] {
                    let ctx = egui::Context::default();
                    theme::install(&ctx);
                    mode.apply(&ctx);
                    ctx.set_zoom_factor(zoom);
                    frame(&ctx, source, width, reply, vec![]);
                    let (output, rect) = frame(&ctx, source, width, reply, vec![]);
                    let (texts, _) = shapes(&output);
                    let find = |needle: &str| {
                        texts
                            .iter()
                            .find(|text| text.galley.job.text.contains(needle))
                            .unwrap()
                    };
                    let heading = find("שלום עולם");
                    let edge = right(heading);
                    assert!((edge - (ctx.screen_rect().right() - 8.0)).abs() < 1.0);
                    for needle in ["DNS", "3. שלום", "4. פריט", "مرحبا"] {
                        let text = find(needle);
                        assert!((right(text) - edge).abs() < 1.0, "{needle}: {:?}", text.pos);
                        assert_eq!(text.galley.job.halign, egui::Align::Min);
                    }
                    let english = find("English שלום");
                    assert!((english.pos.x - 8.0).abs() < 1.0);
                    let code = find("שלום code 123");
                    assert!(
                        !code.galley.job.align_baseline,
                        "Code retains its own layout"
                    );
                    assert!(right(code) < edge - 5.0);
                    if reply {
                        let first = find("3. שלום");
                        assert!(first.galley.rows.len() > 1 || width > 270.0);
                        for row in first.galley.rows.iter().skip(1) {
                            assert!(first.pos.x + row.rect().right() < edge - 5.0);
                        }
                        assert!(right(find("פסקה נוספת")) < edge - 5.0);
                        assert!(right(find("•  פריט")) < edge - 5.0);
                        // Marker is visually to the right of the first Hebrew word.
                        let row = &first.galley.rows[0];
                        assert!(row.glyphs[0].pos.x > row.glyphs[3].pos.x);
                    }
                    assert!(rect.right() <= ctx.screen_rect().right() + 1.0, "{rect:?}");
                }
            }
        }
    }
}

#[test]
fn list_items_and_hard_breaks_detect_direction_independently_across_inline_events() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let source = "- שלום\n- English only\n- \u{2067}**שלום**\u{2069} English\n- שלום  \n  English next line\n\n- English first  \n  שלום בשורה הבאה";
    let (output, _) = frame(&ctx, source, 500.0, true, vec![]);
    let (texts, _) = shapes(&output);
    let find = |needle: &str| {
        texts
            .iter()
            .find(|text| text.galley.job.text.contains(needle))
            .unwrap()
    };
    let rtl = find("•  שלום");
    assert!((right(rtl) - 492.0).abs() < 0.1);
    for needle in ["•  English only", "•  \u{2067}שלום\u{2069} English"] {
        let text = find(needle);
        assert_eq!(text.pos.x, 8.0, "{needle}");
        assert_eq!(text.galley.rows[0].pos.x, 0.0, "{needle}");
    }
    let text = find("English next line");
    assert!(text.galley.rows[0].pos.x > 0.0);
    assert_eq!(
        text.galley.rows[1].pos.x, 0.0,
        "Hard break starts an independent LTR paragraph"
    );
    let text = find("English first");
    assert_eq!(text.galley.rows[0].pos.x, 0.0);
    assert!((text.pos.x + text.galley.rows[1].rect().right() - 492.0).abs() < 0.1);
}

#[test]
fn mixed_direction_nested_lists_preserve_each_ancestors_inset_side() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    for (source, parent, child, parent_rtl) in [
        (
            "- English parent\n  - שלום ילד עם הרבה מילים בעברית כדי לבדוק גלישה מעבר לשורה הראשונה",
            "English parent",
            "שלום ילד",
            false,
        ),
        (
            "- הורה בעברית\n  - English child with enough words to wrap across multiple lines without entering its parent marker column",
            "הורה בעברית",
            "English child",
            true,
        ),
    ] {
        let (output, _) = frame(&ctx, source, 270.0, true, vec![]);
        let (texts, _) = shapes(&output);
        let parent = texts
            .iter()
            .find(|text| text.galley.job.text.contains(parent))
            .unwrap();
        let child = texts
            .iter()
            .find(|text| text.galley.job.text.contains(child))
            .unwrap();
        assert!(child.galley.rows.len() > 1);
        if parent_rtl {
            assert!(right(child) < right(parent) - 5.0);
            assert!((child.pos.x - parent.pos.x).abs() < 0.1);
        } else {
            assert!(child.pos.x > parent.pos.x + 5.0);
            assert!((right(child) - 262.0).abs() < 0.1);
        }
        for row in &child.galley.rows {
            let rect = row.rect().translate(child.pos.to_vec2());
            assert!(rect.left() >= child.pos.x - 0.1);
            assert!(rect.right() <= right(child) + 0.1);
        }
    }
}

#[test]
fn rtl_wrapped_links_find_and_native_selection_keep_logical_source() {
    let source = "שלום **עולם** עם `DNS 8.8.8.8:53` וגם [קישור](https://example.test/rtl) ועוד מילים כדי לבדוק גלישה של שורות בעברית.";
    for reply in [true, false] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let active_rect = Arc::new(Mutex::new(None));
        set_find_highlight(
            &ctx,
            Some(FindHighlight {
                query: "קישור".into(),
                active_in_scope: Some(0),
                seen: Arc::new(AtomicUsize::new(0)),
                active_rect: active_rect.clone(),
            }),
        );
        let (output, _) = frame(&ctx, source, 270.0, reply, vec![]);
        let (texts, _) = shapes(&output);
        let text = texts
            .iter()
            .find(|text| text.galley.job.text.contains("DNS"))
            .unwrap();
        assert_eq!(
            text.galley.job.text,
            "שלום עולם עם DNS 8.8.8.8:53 וגם קישור ועוד מילים כדי לבדוק גלישה של שורות בעברית."
        );
        let byte = text.galley.job.text.find("קישור").unwrap();
        let index = text.galley.job.text[..byte].chars().count();
        let expected = text
            .galley
            .pos_from_cursor(egui::text::CCursor::new(index))
            .translate(text.pos.to_vec2());
        assert_eq!(active_rect.lock().unwrap().unwrap(), expected);
        set_find_highlight(&ctx, None);
        let row = text
            .galley
            .rows
            .iter()
            .find(|row| row.text().contains("קישור"))
            .unwrap();
        let offset = row.text().find('ק').unwrap();
        let glyph = &row.glyphs[row.text()[..offset].chars().count()];
        let pos = text.pos + row.pos.to_vec2() + glyph.logical_rect().center().to_vec2();
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
        let mut commands = Vec::new();
        for pressed in [true, false] {
            commands.extend(
                frame(&ctx, source, 270.0, reply, pointer(pos, pressed))
                    .0
                    .platform_output
                    .commands,
            );
        }
        assert!(commands.iter().any(|c| matches!(c, egui::OutputCommand::OpenUrl(url) if url.url == "https://example.test/rtl")));
        let position = |index| {
            text.pos
                + text
                    .galley
                    .pos_from_cursor(egui::text::CCursor::new(index))
                    .center()
                    .to_vec2()
        };
        let end = position(text.galley.job.text.chars().count()) + egui::vec2(0.5, 0.0);
        frame(
            &ctx,
            source,
            270.0,
            reply,
            pointer(position(0) - egui::vec2(0.5, 0.0), true),
        );
        frame(
            &ctx,
            source,
            270.0,
            reply,
            vec![egui::Event::PointerMoved(end)],
        );
        frame(&ctx, source, 270.0, reply, pointer(end, false));
        let (copied, _) = frame(&ctx, source, 270.0, reply, vec![egui::Event::Copy]);
        assert!(copied.platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(value) if value == &text.galley.job.text)), "{:?}", copied.platform_output.commands);
    }
}
