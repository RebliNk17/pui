use super::*;

fn parser(rows: u16, cols: u16, text: &str) -> vt100::Parser {
    let mut parser = vt100::Parser::new(rows.try_into().unwrap(), cols.try_into().unwrap(), 2000);
    parser.process(text.as_bytes());
    parser
}

#[test]
fn selected_snapshot_matches_terminal_copy_in_both_directions_and_keeps_unicode() {
    let parser = parser(5, 40, "hello 世界 e\u{301}\r\nsecond line\r\nthird");
    for (a, b) in [((0, 6), (1, 6)), ((1, 6), (0, 6)), ((0, 6), (0, 10))] {
        let captured = snapshot(parser.screen(), Some((a, b)), Path::new("/project"));
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        assert_eq!(
            captured.text,
            parser.screen().contents_between(a.0, a.1, b.0, b.1)
        );
        assert!(captured.selection);
        assert!(captured.label.contains("selection"));
    }
    let captured = snapshot(
        parser.screen(),
        Some(((0, 0), (0, 40))),
        Path::new("/project"),
    );
    assert!(captured.text.contains("世界 e\u{301}"));
}

#[test]
fn visible_snapshot_uses_scrolled_viewport_and_soft_wraps_without_inventing_commands() {
    let mut terminal = parser(3, 8, "1234567890\r\nline two\r\nline 3\r\nline 4");
    terminal.screen_mut().set_scrollback(2);
    let captured = snapshot(terminal.screen(), None, Path::new("/project"));
    assert!(captured.text.contains("1234567890"), "{:?}", captured.text);
    assert!(!captured.text.contains("line 4"));
    assert!(captured.label.contains("scrollback offset 2"));
    let zero = snapshot(
        terminal.screen(),
        Some(((0, 1), (0, 1))),
        Path::new("/project"),
    );
    assert!(!zero.selection);
    assert_eq!(zero.text, captured.text);
    let empty = parser(3, 40, "");
    assert!(
        snapshot(empty.screen(), None, Path::new("/project"))
            .text
            .is_empty()
    );
}

#[test]
fn maximum_viewport_capture_is_bounded_and_does_not_join_full_scrollback() {
    let source = (0..2200)
        .map(|_| format!("{}\r\n", "界".repeat(199)))
        .collect::<String>();
    let parser = parser(200, 400, &source);
    let start = std::time::Instant::now();
    let captured = snapshot(
        parser.screen(),
        Some(((u16::MAX, u16::MAX), (0, 0))),
        Path::new("/project"),
    );
    eprintln!(
        "Bounded terminal capture: {} bytes in {:?}",
        captured.text.len(),
        start.elapsed()
    );
    assert!(captured.text.len() > LIMIT && captured.text.len() <= LIMIT + 4);
    let ctx = egui::Context::default();
    context_capture::queue(&ctx, &captured.label, &captured.text);
    let mut app = crate::app::files::tests::app(&ctx);
    app.collect_context_capture();
    assert!(
        app.outgoing_text()
            .contains("Context excerpt limited to 24 KiB")
    );
    assert!(app.outgoing_text().len() < LIMIT + 1024);
}

fn frame(
    ctx: &egui::Context,
    parser: &mut vt100::Parser,
    selection: &mut Selection,
    events: Vec<egui::Event>,
    toolbar: bool,
) -> (egui::FullOutput, egui::Rect, Vec<u8>) {
    let mut rect = egui::Rect::NOTHING;
    let mut bytes = Vec::new();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(390.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = if toolbar {
                    ui.button("Terminal context")
                } else {
                    ui.allocate_response(Vec2::new(340.0, 160.0), Sense::click_and_drag())
                };
                rect = response.rect;
                if !toolbar {
                    if !egui::Popup::is_any_open(ctx) {
                        response.request_focus();
                    }
                    bytes = interact(
                        ui,
                        &response,
                        rect,
                        Vec2::splat(12.0),
                        parser,
                        selection,
                        &mut 0.0,
                    );
                }
                offer(
                    &response,
                    parser.screen(),
                    *selection,
                    Path::new("/project"),
                    toolbar,
                );
            });
        },
    );
    (output, rect, bytes)
}
fn click(
    ctx: &egui::Context,
    parser: &mut vt100::Parser,
    selection: &mut Selection,
    position: egui::Pos2,
    button: egui::PointerButton,
    toolbar: bool,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    for pressed in [true, false] {
        bytes.extend(
            frame(
                ctx,
                parser,
                selection,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                toolbar,
            )
            .2,
        );
    }
    bytes
}

#[test]
fn real_toolbar_and_context_menu_capture_frozen_text_once_without_terminal_input() {
    for toolbar in [false, true] {
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            let mut parser = parser(5, 40, "original text\r\nother line");
            let mut selection = Some(((0, 0), (0, 8)));
            for _ in 0..3 {
                frame(&ctx, &mut parser, &mut selection, vec![], toolbar);
            }
            let rect = frame(&ctx, &mut parser, &mut selection, vec![], toolbar).1;
            let bytes = click(
                &ctx,
                &mut parser,
                &mut selection,
                rect.center(),
                if toolbar {
                    egui::PointerButton::Primary
                } else {
                    egui::PointerButton::Secondary
                },
                toolbar,
            );
            assert!(bytes.is_empty());
            for _ in 0..3 {
                frame(&ctx, &mut parser, &mut selection, vec![], toolbar);
            }
            parser.process(b"\x1b[HCHANGED!");
            let target = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("terminal-context-add")))
                .unwrap()
                .center();
            assert!(
                click(
                    &ctx,
                    &mut parser,
                    &mut selection,
                    target,
                    egui::PointerButton::Primary,
                    toolbar
                )
                .is_empty()
            );
            let mut app = crate::app::files::tests::app(&ctx);
            app.draft = "Keep draft".into();
            app.context_chips.clear_quotes();
            app.state.busy = true;
            let messages = app.state.messages.len();
            app.collect_context_capture();
            assert!(app.outgoing_text().starts_with("Keep draft\n\n"));
            assert!(app.outgoing_text().contains("original"));
            assert!(!app.outgoing_text().contains("CHANGED"));
            assert!(app.outgoing_text().contains("Terminal selection"));
            let draft = app.outgoing_text().clone();
            app.collect_context_capture();
            assert_eq!(app.outgoing_text(), draft);
            assert!(app.state.busy);
            assert_eq!(app.state.messages.len(), messages);
        }
    }
}

#[test]
fn real_pointer_drag_then_secondary_click_captures_the_visible_selection() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut terminal = parser(5, 40, "zero selected after\r\nsecond line");
    let mut selection = None;
    for _ in 0..3 {
        frame(&ctx, &mut terminal, &mut selection, vec![], false);
    }
    let rect = frame(&ctx, &mut terminal, &mut selection, vec![], false).1;
    let start = rect.min + egui::vec2(5.0 * 12.0 + 1.0, 6.0);
    let end = rect.min + egui::vec2(12.0 * 12.0 + 1.0, 6.0);
    frame(
        &ctx,
        &mut terminal,
        &mut selection,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        false,
    );
    frame(
        &ctx,
        &mut terminal,
        &mut selection,
        vec![egui::Event::PointerMoved(end)],
        false,
    );
    frame(
        &ctx,
        &mut terminal,
        &mut selection,
        vec![egui::Event::PointerMoved(end + egui::vec2(0.1, 0.0))],
        false,
    );
    frame(
        &ctx,
        &mut terminal,
        &mut selection,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
        false,
    );
    assert_eq!(selection, Some(((0, 5), (0, 13))));
    click(
        &ctx,
        &mut terminal,
        &mut selection,
        end,
        egui::PointerButton::Secondary,
        false,
    );
    for _ in 0..3 {
        frame(&ctx, &mut terminal, &mut selection, vec![], false);
    }
    let captured = ctx
        .data(|data| data.get_temp::<Capture>(super::key()))
        .unwrap();
    assert!(captured.snapshot.selection);
    assert_eq!(captured.snapshot.text, "selected");
}

#[test]
fn empty_visible_output_disables_capture_and_reopen_refreshes_frozen_snapshot() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut terminal = parser(5, 40, "");
    let mut selection = None;
    for _ in 0..3 {
        frame(&ctx, &mut terminal, &mut selection, vec![], true);
    }
    let rect = frame(&ctx, &mut terminal, &mut selection, vec![], true).1;
    click(
        &ctx,
        &mut terminal,
        &mut selection,
        rect.center(),
        egui::PointerButton::Primary,
        true,
    );
    for _ in 0..3 {
        frame(&ctx, &mut terminal, &mut selection, vec![], true);
    }
    let target = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("terminal-context-add")))
        .unwrap()
        .center();
    click(
        &ctx,
        &mut terminal,
        &mut selection,
        target,
        egui::PointerButton::Primary,
        true,
    );
    let mut app = crate::app::files::tests::app(&ctx);
    app.draft = "unchanged".into();
    app.context_chips.clear_quotes();
    app.collect_context_capture();
    assert_eq!(app.outgoing_text(), "unchanged");
    clear(&ctx);
    terminal.process(b"New visible output");
    click(
        &ctx,
        &mut terminal,
        &mut selection,
        rect.center(),
        egui::PointerButton::Primary,
        true,
    );
    for _ in 0..3 {
        frame(&ctx, &mut terminal, &mut selection, vec![], true);
    }
    let captured = ctx
        .data(|data| data.get_temp::<Capture>(super::key()))
        .unwrap();
    assert_eq!(captured.snapshot.text, "New visible output");
    assert!(!captured.snapshot.selection);
    let target = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("terminal-context-add")))
        .unwrap()
        .center();
    click(
        &ctx,
        &mut terminal,
        &mut selection,
        target,
        egui::PointerButton::Primary,
        true,
    );
    app.collect_context_capture();
    assert!(app.outgoing_text().contains("New visible output"));
    assert!(ctx.data(|data| data.get_temp::<Capture>(super::key()).is_none()));
}

#[test]
fn menu_escape_and_enter_are_not_encoded_as_shell_input() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut parser = parser(5, 40, "safe output");
    let mut selection = None;
    for _ in 0..3 {
        frame(&ctx, &mut parser, &mut selection, vec![], false);
    }
    let rect = frame(&ctx, &mut parser, &mut selection, vec![], false).1;
    click(
        &ctx,
        &mut parser,
        &mut selection,
        rect.center(),
        egui::PointerButton::Secondary,
        false,
    );
    let key = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    let (_, _, bytes) = frame(
        &ctx,
        &mut parser,
        &mut selection,
        vec![
            key(egui::Key::ArrowDown, egui::Modifiers::NONE),
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
        ],
        false,
    );
    assert!(bytes.is_empty());
    let (_, _, bytes) = frame(
        &ctx,
        &mut parser,
        &mut selection,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        false,
    );
    assert!(bytes.is_empty());
    assert!(!egui::Popup::is_any_open(&ctx));
    let (_, _, bytes) = frame(
        &ctx,
        &mut parser,
        &mut selection,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        false,
    );
    assert_eq!(bytes, b"\r", "Shell input resumes only after dismissal");
    assert!(ctx.data(|data| data.get_temp::<Capture>(super::key()).is_none()));
}
