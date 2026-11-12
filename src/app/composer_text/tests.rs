use super::*;
use egui::text::{CCursor, CCursorRange};

fn style(source: &str, needle: &str) -> Style {
    Presentation::new(source, None).styles[source.find(needle).unwrap()]
}

#[test]
fn inline_markdown_is_rich_and_editing_reveals_its_syntax() {
    let source = "**bold** *italic* `code` [Pi](https://pi.dev) https://example.com";
    assert!(style(source, "bold").bold);
    assert!(style(source, "italic").italic);
    assert!(style(source, "code").code);
    assert!(style(source, "Pi").link);
    assert!(style(source, "https://example").link);
    let rich = Presentation::new(source, None);
    assert!(rich.styles[0].hidden);
    assert!(rich.styles[source.find("https://pi").unwrap()].hidden);
    let edit = Presentation::new(source, Some(3..3));
    assert!(
        !edit.styles[0].hidden,
        "caret inside bold reveals both delimiters"
    );
    assert!(edit.styles[source.find('`').unwrap()].hidden);
    let selected = Presentation::new(source, Some(1..source.len()));
    assert!(!selected.styles.iter().any(|s| s.hidden));
    assert!(safe_link("https://pi.dev"));
    assert!(safe_link("mailto:someone@example.com"));
    assert!(!safe_link("javascript:alert(1)"));
    assert!(!safe_link("file:///etc/passwd"));
}

#[test]
fn nested_formatting_unicode_escapes_and_code_remain_literal_where_needed() {
    let source =
        "***שלום*** **é *nested*** \\*literal* `**raw**`\n```md\n* not a list\n```\n---\n* ";
    let nested = style(source, "nested");
    assert!(nested.bold && nested.italic);
    assert!(style(source, "שלום").bold && style(source, "שלום").italic);
    assert!(!style(source, "literal").italic);
    let code = style(source, "raw");
    assert!(code.code && !code.bold);
    assert!(!style(source, "* not").bullet);
    assert!(!style(source, "---").bullet);
    assert!(Presentation::new(source, None).styles[source.len() - 2].bullet);
    let rich = Presentation::new("[**Pi**](https://pi.dev)", None);
    assert!(rich.styles[3].bold && rich.styles[3].link);
    let nested_label = "[a [nested] label](https://pi.dev)";
    let rich = Presentation::new(nested_label, None);
    assert!(!rich.styles[nested_label.find("label").unwrap()].hidden);
    assert!(rich.styles[nested_label.find("](https").unwrap()].hidden);
    let code_label = "[a `]` b](https://pi.dev)";
    let rich = Presentation::new(code_label, None);
    assert!(!rich.styles[code_label.find(']').unwrap()].hidden);
    assert!(rich.styles[code_label.find(']').unwrap()].code);
    assert!(!rich.styles[code_label.find(" b").unwrap() + 1].hidden);
    assert!(rich.styles[code_label.find("](https").unwrap()].hidden);
}

fn newline(source: &str, index: usize) -> (String, usize) {
    let mut text = source.to_owned();
    let mut cursor = CCursor::new(index);
    Buffer {
        text: &mut text,
        smart_newline: true,
    }
    .insert_text_at(&mut cursor, "\n", usize::MAX);
    (text, cursor.index)
}

#[test]
fn lists_continue_split_and_exit_without_touching_other_text() {
    for (source, expected) in [
        ("* item", "* item\n* "),
        ("* `item`", "* `item`\n* "),
        ("1. `item`", "1. `item`\n2. "),
        ("- item", "- item\n- "),
        ("+ item", "+ item\n+ "),
        ("9. item", "9. item\n10. "),
        ("2) item", "2) item\n3) "),
        ("  * nested", "  * nested\n  * "),
        ("שלום\n* é", "שלום\n* é\n* "),
        ("* ", ""),
        ("  3. ", "  "),
        ("paragraph", "paragraph\n"),
        ("*not a list", "*not a list\n"),
        ("```md\n* literal", "```md\n* literal\n"),
        ("    * code", "    * code\n"),
        ("---", "---\n"),
    ] {
        let (text, index) = newline(source, source.chars().count());
        assert_eq!(text, expected, "source={source:?}");
        assert_eq!(index, expected.chars().count());
    }
    assert_eq!(
        newline("* first second", 8),
        ("* first \n* second".into(), 11)
    );
    assert_eq!(newline("* \nkeep", 2), ("\nkeep".into(), 0));
    let mut text = String::new();
    Buffer {
        text: &mut text,
        smart_newline: false,
    }
    .insert_text_at(&mut CCursor::new(0), "* pasted\n1. exact\n", usize::MAX);
    assert_eq!(text, "* pasted\n1. exact\n");
}

fn app(ctx: &egui::Context) -> Pui {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.connected = true;
    app.ultracode_ready = true;
    app.state = State::default();
    app.state.session_file = Some("/fixture/rich-composer.jsonl".into());
    app.client = Some(rpc::Client::stub());
    app
}
fn frame(
    ctx: &egui::Context,
    app: &mut Pui,
    events: Vec<egui::Event>,
    width: f32,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 850.0),
            )),
            modifiers: events
                .iter()
                .find_map(|e| match e {
                    egui::Event::Key { modifiers, .. }
                    | egui::Event::PointerButton { modifiers, .. } => Some(*modifiers),
                    _ => None,
                })
                .unwrap_or_default(),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.composer(ui, ctx));
        },
    )
}
fn select(ctx: &egui::Context, range: Range<usize>) {
    let id = egui::Id::new("composer");
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap();
    state.cursor.set_char_range(Some(CCursorRange::two(
        CCursor::new(range.start),
        CCursor::new(range.end),
    )));
    state.store(ctx, id);
    ctx.memory_mut(|m| m.request_focus(id));
}
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}
fn galley(output: &egui::FullOutput, source: &str) -> Arc<egui::Galley> {
    output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == source => Some(t.galley.clone()),
            _ => None,
        })
        .expect("composer galley")
}

#[test]
fn live_layout_preserves_source_indices_and_renders_bullets_in_both_themes() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        for width in [360.0, 900.0] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            theme.apply(&ctx);
            ctx.set_zoom_factor(1.25);
            app.draft =
                "* **שלום é** and *italic* with `inline code`\n2. [Pi](https://pi.dev)".into();
            app.focus_composer = false;
            frame(&ctx, &mut app, vec![], width);
            let out = frame(&ctx, &mut app, vec![], width);
            let g = galley(&out, &app.draft);
            let glyphs: Vec<_> = g.rows.iter().flat_map(|r| &r.glyphs).collect();
            assert_eq!(g.end().index, app.draft.chars().count());
            assert_eq!(glyphs[0].chr, '•');
            assert_eq!(glyphs[2].chr, '\u{200b}');
            assert!(g.rows.iter().all(|row| row.rect().right() <= width));
            for section in &g.job.sections {
                assert!(app.draft.is_char_boundary(section.byte_range.start));
                assert!(app.draft.is_char_boundary(section.byte_range.end));
            }
            select(&ctx, 5..5);
            let out = frame(&ctx, &mut app, vec![], width);
            let g = galley(&out, &app.draft);
            assert_eq!(
                g.rows[0].glyphs[2].chr, '*',
                "caret reveals editable syntax"
            );
            assert!(app.draft.starts_with("* **שלום é**"));
        }
    }
}

#[test]
fn actual_typing_continues_lists_pastes_and_sends_markdown_unchanged() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    frame(&ctx, &mut app, vec![], 900.0);
    select(&ctx, 0..0);
    frame(&ctx, &mut app, vec![egui::Event::Text("* ".into())], 900.0);
    let out = frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("**one**".into())],
        900.0,
    );
    assert_eq!(galley(&out, &app.draft).rows[0].glyphs[0].chr, '•');
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::SHIFT)],
        900.0,
    );
    assert_eq!(app.draft, "* **one**\n* ");
    assert!(app.deliveries.is_empty());
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::SHIFT)],
        900.0,
    );
    assert_eq!(app.draft, "* **one**\n");
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Paste(
            "[Pi](https://pi.dev)\n1. exact\n".into(),
        )],
        900.0,
    );
    let expected = "* **one**\n[Pi](https://pi.dev)\n1. exact";
    assert_eq!(app.draft.trim(), expected);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        900.0,
    );
    assert!(app.draft.is_empty());
    assert_eq!(app.deliveries.len(), 1);
    assert_eq!(app.deliveries[0].message.text, expected);
}

#[test]
fn selection_copy_replacement_undo_and_redo_use_markdown_not_presentation() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.draft = "**שלום** and `é`".into();
    let original = app.draft.clone();
    frame(&ctx, &mut app, vec![], 900.0);
    select(&ctx, 0..original.chars().count());
    let out = frame(&ctx, &mut app, vec![egui::Event::Copy], 900.0);
    assert!(
        out.platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == &original))
    );
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("1. replacement".into())],
        900.0,
    );
    assert_eq!(app.draft, "1. replacement");
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::SHIFT)],
        900.0,
    );
    assert_eq!(app.draft, "1. replacement\n2. ");
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Z, egui::Modifiers::COMMAND)],
        900.0,
    );
    assert!(
        !app.draft.contains("\n2. "),
        "list continuation participates in undo"
    );
    frame(
        &ctx,
        &mut app,
        vec![key(
            egui::Key::Z,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        )],
        900.0,
    );
    assert_eq!(app.draft, "1. replacement\n2. ");
}

#[test]
fn ime_and_unicode_edits_preserve_markdown_and_cursor_positions() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.draft = "**שלום** ".into();
    frame(&ctx, &mut app, vec![], 900.0);
    let end = app.draft.chars().count();
    select(&ctx, end..end);
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Ime(egui::ImeEvent::Enabled)],
        900.0,
    );
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Ime(egui::ImeEvent::Preedit("に".into()))],
        900.0,
    );
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Ime(egui::ImeEvent::Commit("日本".into()))],
        900.0,
    );
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Ime(egui::ImeEvent::Disabled)],
        900.0,
    );
    assert_eq!(app.draft, "**שלום** 日本");
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Backspace, egui::Modifiers::NONE)],
        900.0,
    );
    assert_eq!(app.draft, "**שלום** 日");
    let state = egui::TextEdit::load_state(&ctx, egui::Id::new("composer")).unwrap();
    assert_eq!(
        state.cursor.char_range().unwrap().primary.index,
        app.draft.chars().count()
    );
}

#[test]
fn links_only_open_with_command_click_and_never_open_unsafe_schemes() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.draft = "[Pi](https://pi.dev)".into();
    app.focus_composer = false;
    frame(&ctx, &mut app, vec![], 900.0);
    let out = frame(&ctx, &mut app, vec![], 900.0);
    let pos = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == app.draft => {
                Some(t.pos + egui::vec2(4.0, 7.0))
            }
            _ => None,
        })
        .unwrap();
    for command in [false, true] {
        let modifiers = if command {
            egui::Modifiers::COMMAND
        } else {
            egui::Modifiers::NONE
        };
        let mut opened = false;
        for pressed in [true, false] {
            let out = frame(
                &ctx,
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers,
                    },
                ],
                900.0,
            );
            opened |=
                out.platform_output.commands.iter().any(
                    |c| matches!(c, egui::OutputCommand::OpenUrl(u) if u.url == "https://pi.dev"),
                );
        }
        assert_eq!(opened, command);
        assert_eq!(app.draft, "[Pi](https://pi.dev)");
    }
}
