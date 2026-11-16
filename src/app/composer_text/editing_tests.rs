use super::tests::{app, frame, galley, key, select};
use super::*;

#[test]
fn tab_nests_the_item_not_the_caret_and_shift_tab_reverses_it() {
    for source in [
        "1. first\n2. second",
        "* first\n* second",
        "a. first\nb. second",
    ] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.draft = source.into();
        frame(&ctx, &mut app, vec![], 600.0);
        let cursor = source.chars().count();
        select(&ctx, cursor..cursor);
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
            600.0,
        );
        let (first, second) = source.split_once('\n').unwrap();
        let nested = format!("{first}\n    {second}");
        assert_eq!(app.draft, nested);
        assert!(
            list_at_cursor(&ctx, &app.draft),
            "draft={:?} selection={:?} lists={:?} focus={:?}",
            app.draft,
            selection(&ctx, &app.draft),
            Presentation::new(&app.draft, None).lists,
            ctx.memory(|m| m.focused())
        );
        assert!(ctx.memory(|m| m.has_focus(egui::Id::new("composer"))));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)],
            600.0,
        );
        assert_eq!(app.draft, source);
        assert!(app.deliveries.is_empty());
        // A selection is indented as whole lines, never replaced by a tab.
        select(&ctx, 0..cursor);
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
            600.0,
        );
        assert_eq!(app.draft, format!("    {first}\n    {second}"));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)],
            600.0,
        );
        assert_eq!(app.draft, source);
    }
}

#[test]
fn nested_lists_continue_and_lettered_lists_are_not_italic_or_code() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.draft = "a. one\n    b. two".into();
    frame(&ctx, &mut app, vec![], 600.0);
    let end = app.draft.chars().count();
    select(&ctx, end..end);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::SHIFT)],
        600.0,
    );
    assert_eq!(app.draft, "a. one\n    b. two\n    c. ");
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::SHIFT)],
        600.0,
    );
    assert_eq!(app.draft, "a. one\n    b. two\n    ");
    assert!(!Presentation::new(&app.draft, None).styles[app.draft.find("two").unwrap()].code);
}

#[test]
fn formatting_shortcuts_toggle_selections_and_empty_carets_with_undo() {
    for (key_code, open, close) in [
        (egui::Key::B, "**", "**"),
        (egui::Key::I, "*", "*"),
        (egui::Key::U, "<u>", "</u>"),
    ] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.draft = "שלום é".into();
        let original = app.draft.clone();
        frame(&ctx, &mut app, vec![], 600.0);
        select(&ctx, 0..original.chars().count());
        let sidebar = app.prefs.hide_sidebar;
        let inspector = app.prefs.hide_inspector;
        frame(
            &ctx,
            &mut app,
            vec![key(key_code, egui::Modifiers::COMMAND)],
            600.0,
        );
        let formatted = format!("{open}{original}{close}");
        assert_eq!(app.draft, formatted);
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Z, egui::Modifiers::COMMAND)],
            600.0,
        );
        assert_eq!(app.draft, original);
        frame(
            &ctx,
            &mut app,
            vec![key(
                egui::Key::Z,
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            )],
            600.0,
        );
        assert_eq!(app.draft, formatted);
        frame(
            &ctx,
            &mut app,
            vec![key(key_code, egui::Modifiers::COMMAND)],
            600.0,
        );
        assert_eq!(app.draft, original);
        assert_eq!(app.prefs.hide_sidebar, sidebar);
        assert_eq!(app.prefs.hide_inspector, inspector);
        let end = app.draft.chars().count();
        select(&ctx, end..end);
        frame(
            &ctx,
            &mut app,
            vec![key(key_code, egui::Modifiers::COMMAND)],
            600.0,
        );
        frame(&ctx, &mut app, vec![egui::Event::Text("new".into())], 600.0);
        assert_eq!(app.draft, format!("{original}{open}new{close}"));
        assert!(app.deliveries.is_empty());
    }
}

#[test]
fn list_insets_and_code_blocks_are_distinct_in_themes_and_narrow_windows() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        for width in [360.0, 900.0] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            theme.apply(&ctx);
            ctx.set_zoom_factor(1.2);
            app.focus_composer = false;
            app.draft =
                "plain\n1. numbered\na. letter\n* bullet\n`inline` ```block```\nafter".into();
            frame(&ctx, &mut app, vec![], width);
            let out = frame(&ctx, &mut app, vec![], width);
            let g = galley(&out, &app.draft);
            assert!(g.rows[1].pos.x >= g.rows[0].pos.x + 10.0);
            assert!(g.rows[2].pos.x >= g.rows[0].pos.x + 10.0);
            assert!(g.rows[3].pos.x >= g.rows[0].pos.x + 10.0);
            let inline = g.rows.iter().find(|r| r.text().contains("inline")).unwrap();
            let block = g.rows.iter().find(|r| r.text().contains("block")).unwrap();
            assert!(block.height() > inline.height());
            assert!(!block.text().contains("inline"));
            assert_eq!(g.end().index, app.draft.chars().count());
            let styles = Presentation::new(&app.draft, None);
            assert!(!styles.styles[app.draft.find("inline").unwrap()].block);
            assert!(styles.styles[app.draft.find("block").unwrap()].block);
            assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Vec(shapes) if shapes.iter().any(|s| matches!(s, egui::Shape::Rect(r) if r.fill == theme.palette().code_bg)))));
        }
    }
    let source = "```rust\nlet x = 1;\n```\nnormal";
    let presentation = Presentation::new(source, None);
    assert!(presentation.styles[source.find("let").unwrap()].block);
    assert!(!presentation.styles[source.find("normal").unwrap()].block);
}

#[test]
fn italic_shortcut_does_not_strip_bold_delimiters() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.draft = "**bold**".into();
    frame(&ctx, &mut app, vec![], 600.0);
    select(&ctx, 0..8);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::I, egui::Modifiers::COMMAND)],
        600.0,
    );
    assert_eq!(app.draft, "***bold***");
    let p = Presentation::new(&app.draft, None);
    assert!(p.styles[3].bold && p.styles[3].italic);
    app.draft = "**bold**".into();
    select(&ctx, 1..1);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::I, egui::Modifiers::COMMAND)],
        600.0,
    );
    assert_eq!(
        app.draft, "****bold**",
        "caret within a bold delimiter must not delete it"
    );
    app.draft = "**".into();
    select(&ctx, 1..1);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::I, egui::Modifiers::COMMAND)],
        600.0,
    );
    assert!(
        app.draft.is_empty(),
        "an empty italic pair still toggles off"
    );
}

#[test]
fn underline_is_rendered_without_interpreting_literal_code() {
    let source = "<u>hello **bold**</u> `<u>code</u>`";
    let p = Presentation::new(source, None);
    assert!(p.styles[source.find("hello").unwrap()].underline);
    assert!(p.styles[source.find("bold").unwrap()].underline);
    assert!(!p.styles[source.find("code").unwrap()].underline);
    assert!(p.styles[0].hidden);
}

#[test]
fn new_formatting_or_indentation_after_undo_discards_redo_branch() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.draft = "one".into();
    frame(&ctx, &mut app, vec![], 600.0);
    select(&ctx, 0..3);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::B, egui::Modifiers::COMMAND)],
        600.0,
    );
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Z, egui::Modifiers::COMMAND)],
        600.0,
    );
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::I, egui::Modifiers::COMMAND)],
        600.0,
    );
    assert_eq!(app.draft, "*one*");
    frame(
        &ctx,
        &mut app,
        vec![key(
            egui::Key::Z,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        )],
        600.0,
    );
    assert_eq!(app.draft, "*one*", "abandoned Bold redo must not resurrect");
}

#[test]
fn code_background_excludes_preceding_prose_and_nested_underline_works() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.focus_composer = false;
    app.draft = "prose ```code``` after".into();
    frame(&ctx, &mut app, vec![], 600.0);
    let out = frame(&ctx, &mut app, vec![], 600.0);
    let origin = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == app.draft => Some(t.pos),
            _ => None,
        })
        .unwrap();
    let g = galley(&out, &app.draft);
    let code = g.rows.iter().find(|r| r.text().contains("code")).unwrap();
    let top = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| match s {
                egui::Shape::Rect(r) if r.fill == theme::palette(&ctx).code_bg => {
                    Some(r.rect.top())
                }
                _ => None,
            }),
            _ => None,
        })
        .unwrap();
    assert!(top >= origin.y + code.pos.y - 0.5);
    let text = "    a. <u>underlined</u>";
    let p = Presentation::new(text, None);
    assert!(p.styles[text.find("underlined").unwrap()].underline);
    let literal = "```md\n    a. literal\n```";
    assert_eq!(parse_source(literal), literal);
}
