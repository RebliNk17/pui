use super::*;
use egui::{text::CCursor, text_selection::CCursorRange};

fn layout(text: &str, width: f32) -> Arc<egui::Galley> {
    let ctx = egui::Context::default();
    install(&ctx);
    let mut galley = None;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        galley = Some(ctx.fonts(|f| {
            f.layout(
                text.into(),
                FontId::proportional(16.0),
                Color32::WHITE,
                width,
            )
        }));
    });
    galley.unwrap()
}
fn visual(row: &egui::epaint::text::Row) -> String {
    let mut glyphs: Vec<_> = row.glyphs.iter().collect();
    glyphs.sort_by(|a, b| a.pos.x.total_cmp(&b.pos.x));
    glyphs.iter().map(|g| g.chr).collect()
}

#[test]
fn hebrew_bidi_is_visual_only_and_mixed_english_numbers_keep_order() {
    for (source, expected) in [
        ("שלום עולם", "םלוע םולש"),
        ("Hello שלום 123 world", "Hello 123 םולש world"),
        ("שלום English 123", "English 123 םולש"),
        ("עברית", "תירבע"),
    ] {
        let galley = layout(source, 1000.0);
        assert_eq!(galley.job.text, source);
        assert_eq!(
            galley.rows[0].text(),
            source,
            "Glyph vectors retain logical order"
        );
        assert_eq!(visual(&galley.rows[0]), expected);
    }
}
#[test]
fn wrapped_hebrew_keeps_logical_row_order_and_each_line_reads_rtl() {
    let source = "שלום עולם שלום עולם שלום עולם";
    let galley = layout(source, 90.0);
    assert!(galley.rows.len() > 1);
    assert_eq!(
        galley.rows.iter().map(|r| r.text()).collect::<String>(),
        source
    );
    for row in &galley.rows {
        assert_eq!(visual(row), row.text().chars().rev().collect::<String>());
    }
}
#[test]
fn bidi_covers_multiple_paragraphs_graphemes_and_ltr_fast_path() {
    let source = "שלום עולם\nEnglish 123\nשלום English 123";
    let galley = layout(source, 1000.0);
    assert_eq!(visual(&galley.rows[0]), "םלוע םולש");
    assert_eq!(visual(&galley.rows[1]), "English 123");
    assert!(galley.rows[1].bidi.is_none());
    assert_eq!(visual(&galley.rows[2]), "English 123 םולש");
    let marked = layout("שָׁלוֹם", 1000.0);
    assert_eq!(
        visual(&marked.rows[0]),
        "םולֹשָׁ",
        "Keep marks attached to their grapheme"
    );
    let mut c = CCursor::new("שלום עולם\n".chars().count());
    for _ in 0..7 {
        c = galley.cursor_right_one_character(&c);
    }
    assert_eq!(c.index, "שלום עולם\nEnglish".chars().count());
    let ltr = layout("English 123 · 中文", 1000.0);
    assert!(ltr.rows[0].bidi.is_none());
    assert_eq!(visual(&ltr.rows[0]), ltr.job.text);
}
#[test]
fn hebrew_caret_clicks_and_arrows_follow_visual_positions() {
    let galley = layout("שלום", 1000.0);
    let row = &galley.rows[0];
    for index in 0..=4 {
        let cursor = CCursor::new(index);
        let pos = galley.pos_from_cursor(cursor).center();
        assert_eq!(galley.cursor_from_pos(pos.to_vec2()).index, index);
        if index < 4 {
            let next = galley.cursor_left_one_character(&cursor);
            assert_eq!(next.index, index + 1);
            assert!(galley.pos_from_cursor(next).left() < pos.x);
        }
    }
    assert_eq!(row.x_offset(4), row.glyphs[3].pos.x);
    assert!(row.x_offset(0) > row.x_offset(4));
}
#[test]
fn wrapped_rtl_arrow_crosses_lines_in_reading_direction() {
    let galley = layout("שלום עולם שלום עולם", 85.0);
    assert!(galley.rows.len() > 1);
    let end_first = galley.rows[0].glyphs.len();
    let cursor = CCursor {
        index: end_first,
        prefer_next_row: false,
    };
    let next = galley.cursor_left_one_character(&cursor);
    assert_eq!(galley.layout_from_cursor(next).row, 1);
    let next = galley.cursor_left_one_character(&next);
    assert_eq!(next.index, end_first + 1);
}
#[test]
fn mixed_hebrew_pointer_targets_and_selection_collapse_use_visual_edges() {
    let ctx = egui::Context::default();
    install(&ctx);
    let mut value = "Hello שלום 123 world".to_owned();
    let (_, galley, _) = edit_frame(&ctx, &mut value, vec![]);
    let row = &galley.rows[0];
    for index in 6..10 {
        let glyph = &row.glyphs[index];
        let x = glyph.pos.x + glyph.advance_width * 0.75;
        assert_eq!(
            row.char_at(x),
            index,
            "Right half of RTL glyph is logically before it"
        );
    }
    value = "שלום".into();
    edit_frame(&ctx, &mut value, vec![]);
    set_caret(&ctx, CCursorRange::two(CCursor::new(0), CCursor::new(4)));
    let (_, _, range) = edit_frame(&ctx, &mut value, vec![key(egui::Key::ArrowLeft)]);
    assert_eq!(range.unwrap().primary.index, 4);
    set_caret(&ctx, CCursorRange::two(CCursor::new(0), CCursor::new(4)));
    let (_, _, range) = edit_frame(&ctx, &mut value, vec![key(egui::Key::ArrowRight)]);
    assert_eq!(range.unwrap().primary.index, 0);
}
#[test]
fn bidi_selection_paints_positive_rects_for_logical_ranges() {
    let mut galley = layout("English שלום 123", 1000.0);
    let row = &galley.rows[0];
    let rects = row.selection_rects(8..12);
    assert!(!rects.is_empty());
    assert!(rects.iter().all(|r| r.width() > 0.0));
    let range = CCursorRange::two(CCursor::new(8), CCursor::new(12));
    let mut painted = Vec::new();
    egui::text_selection::visuals::paint_text_selection(
        &mut galley,
        &egui::Visuals::dark(),
        &range,
        Some(&mut painted),
    );
    assert!(!painted.is_empty());
    assert_eq!(
        galley.job.text.chars().skip(8).take(4).collect::<String>(),
        "שלום"
    );
}
fn edit_frame(
    ctx: &egui::Context,
    value: &mut String,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Arc<egui::Galley>, Option<CCursorRange>) {
    let mut result = None;
    let output = ctx.run(
        egui::RawInput {
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let edit = egui::TextEdit::multiline(value)
                    .id(egui::Id::new("bidi-editor"))
                    .show(ui);
                edit.response.request_focus();
                result = Some((edit.galley, edit.cursor_range));
            });
        },
    );
    let (galley, range) = result.unwrap();
    (output, galley, range)
}
fn set_caret(ctx: &egui::Context, range: CCursorRange) {
    let id = egui::Id::new("bidi-editor");
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap();
    state.cursor.set_char_range(Some(range));
    state.store(ctx, id);
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
fn hebrew_editor_arrows_insertion_backspace_and_copy_use_logical_source() {
    let ctx = egui::Context::default();
    install(&ctx);
    for mode in [Theme::Dark, Theme::Light] {
        mode.apply(&ctx);
        let mut value = "שלום".to_owned();
        edit_frame(&ctx, &mut value, vec![]);
        set_caret(&ctx, CCursorRange::one(CCursor::new(0)));
        let (_, _, range) = edit_frame(&ctx, &mut value, vec![key(egui::Key::ArrowLeft)]);
        assert_eq!(range.unwrap().primary.index, 1);
        edit_frame(&ctx, &mut value, vec![egui::Event::Text("א".into())]);
        assert_eq!(value, "שאלום");
        edit_frame(&ctx, &mut value, vec![key(egui::Key::Backspace)]);
        assert_eq!(value, "שלום");
        set_caret(&ctx, CCursorRange::two(CCursor::new(0), CCursor::new(4)));
        let (output, _, _) = edit_frame(&ctx, &mut value, vec![egui::Event::Copy]);
        assert!(
            output
                .platform_output
                .commands
                .iter()
                .any(|c| matches!(c, egui::OutputCommand::CopyText(s) if s == "שלום"))
        );
        edit_frame(
            &ctx,
            &mut value,
            vec![egui::Event::Paste("עברית English 123".into())],
        );
        assert_eq!(value, "עברית English 123");
        let (_, galley, _) = edit_frame(&ctx, &mut value, vec![]);
        assert_eq!(visual(&galley.rows[0]), "English 123 תירבע");
    }
}
#[test]
fn rtl_brackets_are_mirrored_only_in_mesh_not_source() {
    let galley = layout("שלום (עולם)", 1000.0);
    assert_eq!(galley.job.text, "שלום (עולם)");
    let glyphs = &galley.rows[0].glyphs;
    assert!(glyphs[5].pos.x > glyphs[10].pos.x);
}
