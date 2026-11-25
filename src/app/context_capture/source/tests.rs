use super::*;

#[test]
fn character_ranges_are_unicode_safe_exact_and_line_numbered() {
    let path = Path::new("src/quoted.rs");
    let source = "before\r\nשלום 世界 e\u{301}\r\nafter";
    let start = "before\r\n".chars().count();
    let text = "שלום 世界 e\u{301}";
    let captured = selected(source, start..start + text.chars().count(), path, 4).unwrap();
    assert_eq!(captured.text, text);
    assert_eq!(captured.label, "src/quoted.rs:5–5 · selected source");
    let captured = selected(source, 0..source.chars().count(), path, 1).unwrap();
    assert_eq!(captured.text, source);
    assert!(captured.label.contains(":1–3"));
    let cr = selected("one\rtwo\rthree", 4..7, path, 1).unwrap();
    assert!(cr.label.contains(":2–2"));
    assert!(selected(source, 10..10, path, 1).is_none());
    assert!(selected(source, 0..1000, path, 1).is_none());
    assert!(selected(source, 1000..1001, path, 1).is_none());
    let source = "界".repeat(100_000);
    let start = std::time::Instant::now();
    let captured = selected(&source, 10..90_000, path, 1).unwrap();
    eprintln!(
        "Bounded selected source: {} bytes in {:?}",
        captured.text.len(),
        start.elapsed()
    );
    assert!(captured.text.len() > MAX_BYTES && captured.text.len() <= MAX_BYTES + 4);
    let ctx = egui::Context::default();
    enqueue(&ctx, &captured);
    let mut app = crate::app::files::tests::app(&ctx);
    app.collect_context_capture();
    assert_eq!(
        app.outgoing_text()
            .matches("Context excerpt limited to 24 KiB")
            .count(),
        1
    );
}

fn frame(
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    source: &str,
    other: bool,
) -> (egui::FullOutput, egui::Response) {
    let mut response = None;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(620.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            inputs(ctx);
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.push_id(
                    if other {
                        "other-file.rs"
                    } else {
                        "source-file.rs"
                    },
                    |ui| {
                        let label = ui.add(
                            egui::Label::new(RichText::new(source).monospace().size(14.0))
                                .extend()
                                .selectable(true),
                        );
                        offer(
                            &label,
                            Path::new(if other { "other.rs" } else { "source.rs" }),
                            source,
                            1,
                        );
                        response = Some(label);
                    },
                );
            });
        },
    );
    (output, response.unwrap())
}
pub(super) fn pos(output: &egui::FullOutput, source: &str, character: usize) -> egui::Pos2 {
    let text = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == source => Some(text),
            _ => None,
        })
        .unwrap();
    text.pos
        + text
            .galley
            .pos_from_cursor(egui::text::CCursor::new(character))
            .center()
            .to_vec2()
}
pub(super) fn pointer(
    pos: egui::Pos2,
    button: egui::PointerButton,
    pressed: bool,
) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
pub(super) fn select(ctx: &egui::Context, source: &str, range: Range<usize>) -> egui::Response {
    for _ in 0..3 {
        frame(ctx, vec![], source, false);
    }
    let output = frame(ctx, vec![], source, false).0;
    let start = pos(&output, source, range.start);
    let end = pos(&output, source, range.end);
    frame(
        ctx,
        pointer(start, egui::PointerButton::Primary, true),
        source,
        false,
    );
    frame(ctx, vec![egui::Event::PointerMoved(end)], source, false);
    frame(
        ctx,
        pointer(end, egui::PointerButton::Primary, false),
        source,
        false,
    )
    .1
}

#[test]
fn real_drag_secondary_menu_captures_only_selected_text_in_both_directions() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for backwards in [false, true] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            let source = "prefix literal \"quote\" ; echo no-send\nsuffix";
            let start = 7;
            let end = 22;
            let response = select(
                &ctx,
                source,
                if backwards { end..start } else { start..end },
            );
            let range = egui::text_selection::LabelSelectionState::load(&ctx)
                .range_for_response(&response)
                .unwrap();
            assert_eq!(range.slice_str(source), "literal \"quote\"");
            let output = frame(&ctx, vec![], source, false).0;
            let target = pos(&output, source, start + 2);
            for pressed in [true, false] {
                frame(
                    &ctx,
                    pointer(target, egui::PointerButton::Secondary, pressed),
                    source,
                    false,
                );
            }
            for _ in 0..3 {
                frame(&ctx, vec![], source, false);
            }
            let menu = ctx.data(|data| data.get_temp::<SourceMenu>(key())).unwrap();
            assert_eq!(menu.selected.as_ref().unwrap().text, "literal \"quote\"");
            let target = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
                .unwrap()
                .center();
            for pressed in [true, false] {
                frame(
                    &ctx,
                    pointer(target, egui::PointerButton::Primary, pressed),
                    source,
                    false,
                );
            }
            let mut app = crate::app::files::tests::app(&ctx);
            app.draft = "Keep draft".into();
            app.context_chips.clear_quotes();
            app.state.busy = true;
            let count = app.state.messages.len();
            app.collect_context_capture();
            assert_eq!(
                app.outgoing_text(),
                "Keep draft\n\n[Context: source.rs:1–1 · selected source]\nliteral \"quote\"\n[/Context]"
            );
            assert!(app.state.busy);
            assert_eq!(count, app.state.messages.len());
            let draft = app.outgoing_text().clone();
            app.collect_context_capture();
            assert_eq!(app.outgoing_text(), draft);
            assert!(ctx.data(|data| data.get_temp::<SourceMenu>(key()).is_none()));
        }
    }
}

#[test]
fn unicode_crlf_source_selection_uses_real_character_cursors_and_keyboard_add() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let source = "prefix\r\nhello 世界 e\u{301} tail\r\nend";
    let start = "prefix\r\nhello ".chars().count();
    let end = start + "世界 e\u{301}".chars().count();
    select(&ctx, source, start..end);
    let target = pos(&frame(&ctx, vec![], source, false).0, source, start);
    for pressed in [true, false] {
        frame(
            &ctx,
            pointer(target, egui::PointerButton::Secondary, pressed),
            source,
            false,
        );
    }
    for _ in 0..3 {
        frame(&ctx, vec![], source, false);
    }
    let menu = ctx.data(|data| data.get_temp::<SourceMenu>(key())).unwrap();
    assert_eq!(menu.selected.as_ref().unwrap().text, "世界 e\u{301}");
    assert_eq!(
        menu.selected.as_ref().unwrap().label,
        "source.rs:2–2 · selected source"
    );
    frame(
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        source,
        false,
    );
    let mut app = crate::app::files::tests::app(&ctx);
    app.draft = "Kept".into();
    app.context_chips.clear_quotes();
    app.collect_context_capture();
    assert_eq!(
        app.outgoing_text(),
        "Kept\n\n[Context: source.rs:2–2 · selected source]\n世界 e\u{301}\n[/Context]"
    );
}

#[test]
fn no_selection_disables_exact_action_but_explicit_excerpt_still_works() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let source = "whole file excerpt";
    for _ in 0..3 {
        frame(&ctx, vec![], source, false);
    }
    let target = frame(&ctx, vec![], source, false).1.rect.center();
    for pressed in [true, false] {
        frame(
            &ctx,
            pointer(target, egui::PointerButton::Secondary, pressed),
            source,
            false,
        );
    }
    for _ in 0..3 {
        frame(&ctx, vec![], source, false);
    }
    let menu = ctx.data(|data| data.get_temp::<SourceMenu>(key())).unwrap();
    assert!(menu.selected.is_none());
    let target = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        frame(
            &ctx,
            pointer(target, egui::PointerButton::Primary, pressed),
            source,
            false,
        );
    }
    let mut app = crate::app::files::tests::app(&ctx);
    app.draft = "Kept".into();
    app.context_chips.clear_quotes();
    app.collect_context_capture();
    assert_eq!(app.outgoing_text(), "Kept");
    // A disabled-row click closes the standard menu; reopen explicitly for the excerpt action.
    for pressed in [true, false] {
        frame(
            &ctx,
            pointer(
                frame(&ctx, vec![], source, false).1.rect.center(),
                egui::PointerButton::Secondary,
                pressed,
            ),
            source,
            false,
        );
    }
    for _ in 0..3 {
        frame(&ctx, vec![], source, false);
    }
    let output = frame(&ctx, vec![], source, false).0;
    let target = pos(&output, "Add file excerpt to chat", 4);
    for pressed in [true, false] {
        frame(
            &ctx,
            pointer(target, egui::PointerButton::Primary, pressed),
            source,
            false,
        );
    }
    app.collect_context_capture();
    assert_eq!(
        app.outgoing_text(),
        "Kept\n\n[Context: source.rs:1 · file excerpt]\nwhole file excerpt\n[/Context]"
    );
}

#[test]
fn selection_in_another_widget_cannot_capture_and_cancel_never_queues() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let source = "selected text";
    select(&ctx, source, 0..8);
    let (_, other) = frame(&ctx, vec![], source, true);
    assert!(
        egui::text_selection::LabelSelectionState::load(&ctx)
            .range_for_response(&other)
            .is_none()
    );
    for pressed in [true, false] {
        frame(
            &ctx,
            pointer(other.rect.center(), egui::PointerButton::Secondary, pressed),
            source,
            true,
        );
    }
    let menu = ctx.data(|data| data.get_temp::<SourceMenu>(key())).unwrap();
    assert!(menu.selected.is_none());
    let escape = egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    frame(&ctx, vec![escape], source, true);
    assert!(!egui::Popup::is_any_open(&ctx));
    let mut app = crate::app::files::tests::app(&ctx);
    app.draft = "unchanged".into();
    app.context_chips.clear_quotes();
    app.collect_context_capture();
    assert_eq!(app.outgoing_text(), "unchanged");
}
