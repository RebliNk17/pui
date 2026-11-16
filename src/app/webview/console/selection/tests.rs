use super::*;
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
fn pointer(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> Vec<egui::Event> {
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
fn position(output: &egui::FullOutput, text: &str, character: usize) -> egui::Pos2 {
    let shape = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(shape) if shape.galley.job.text == text => Some(shape),
            _ => None,
        })
        .unwrap();
    shape.pos
        + shape
            .galley
            .pos_from_cursor(egui::text::CCursor::new(character))
            .center()
            .to_vec2()
}
fn frame(ctx: &egui::Context, line: &ConsoleLine, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(500.0, 550.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            context_capture::inputs(ctx);
            egui::CentralPanel::default().show(ctx, |ui| show(ui, "https://example.test/", line));
        },
    )
}
fn select(ctx: &egui::Context, line: &ConsoleLine, from: usize, to: usize) {
    for _ in 0..3 {
        frame(ctx, line, vec![]);
    }
    let output = frame(ctx, line, vec![]);
    let a = position(&output, &line.text, from);
    let b = position(&output, &line.text, to);
    frame(ctx, line, pointer(a, egui::PointerButton::Primary, true));
    frame(ctx, line, vec![egui::Event::PointerMoved(b)]);
    frame(ctx, line, pointer(b, egui::PointerButton::Primary, false));
    for pressed in [true, false] {
        frame(
            ctx,
            line,
            pointer(a, egui::PointerButton::Secondary, pressed),
        );
    }
    for _ in 0..3 {
        frame(ctx, line, vec![]);
    }
}
#[test]
fn unicode_exact_character_ranges_keep_message_time_provenance_without_full_fallback() {
    let mut state = PageState {
        location: "https://example.test/message-time".into(),
        ..Default::default()
    };
    push(
        &mut state,
        true,
        "prefix 界e\u{301}\nselected suffix",
        "a.js",
        7,
    );
    let line = &state.console[0];
    let (label, text) = selected(line, "https://example.test/current", 7..11).unwrap();
    assert_eq!(text, "界e\u{301}\n\nSource: a.js:7");
    assert!(label.contains("message-time") && label.contains("characters 8–12 (end exclusive)"));
    assert!(selected(line, "k", 0..0).is_none());
    assert!(selected(line, "k", 0..9999).is_none());
    assert!(selected(line, "k", usize::MAX..usize::MAX).is_none());
    let inline = ConsoleLine::new(false, "chosen", "data:text/html;base64,PRIVATE_DOCUMENT", 5);
    let (_, text) = selected(&inline, "/tmp/local.html", 0..6).unwrap();
    assert!(!text.contains("PRIVATE_DOCUMENT"));
    assert!(text.contains("[inline local HTML]:5"));
}
#[test]
fn actual_forward_and_reverse_drag_add_only_selected_console_message_text_to_draft() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for reverse in [false, true] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            theme.apply(&ctx);
            let mut app = crate::app::files::tests::app(&ctx);
            app.draft = "keep".into();
            let line = ConsoleLine::new(true, "prefix chosen suffix", "source.js", 3);
            select(
                &ctx,
                &line,
                if reverse { 13 } else { 7 },
                if reverse { 7 } else { 13 },
            );
            let target = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
                .unwrap()
                .center();
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &line,
                    pointer(target, egui::PointerButton::Primary, pressed),
                );
            }
            app.collect_context_capture();
            assert!(app.draft.starts_with("keep\n\n"));
            assert!(
                app.draft
                    .contains("\nchosen\nSource: source.js:3\n[/Context]")
            );
            assert!(!app.draft.contains("prefix") && !app.draft.contains("suffix"));
            assert!(!egui::Popup::is_any_open(&ctx));
        }
    }
}
#[test]
fn console_selection_menu_owns_approval_chords_and_escape_without_sending() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut app = crate::app::files::tests::app(&ctx);
    app.draft = "keep".into();
    app.state.busy = true;
    app.state.dialogs.push_back(json!({"id":"console-selection","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo safe\"}\n\nApprove this call only."}));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let line = ConsoleLine::new(false, "prefix chosen suffix", "source.js", 2);
    select(&ctx, &line, 7, 13);
    for (code, modifiers) in [
        (egui::Key::Enter, egui::Modifiers::COMMAND),
        (egui::Key::Backspace, egui::Modifiers::COMMAND),
        (egui::Key::Escape, egui::Modifiers::NONE),
    ] {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 900.0),
                )),
                events: vec![key(code, modifiers)],
                ..Default::default()
            },
            |ctx| {
                context_capture::inputs(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    show(ui, "https://example.test/", &line);
                    app.agent_question(ui);
                });
            },
        );
    }
    app.collect_context_capture();
    assert_eq!(app.draft, "keep");
    assert!(app.state.busy);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(commands.try_recv().is_err());
    assert!(!egui::Popup::is_any_open(&ctx));
}
#[test]
fn newly_arrived_structured_question_cannot_reuse_console_popup_activation_or_text() {
    for code in [egui::Key::Enter, egui::Key::Space, egui::Key::Num1] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut app = crate::app::files::tests::app(&ctx);
        app.draft = "keep".into();
        app.state.busy = true;
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let line = ConsoleLine::new(false, "prefix chosen suffix", "source.js", 1);
        select(&ctx, &line, 7, 13);
        let mut dialog = crate::app::questions::demo_dialog();
        let mut payload: Value =
            serde_json::from_str(dialog["placeholder"].as_str().unwrap()).unwrap();
        payload["questions"].as_array_mut().unwrap().truncate(1);
        dialog["placeholder"] = Value::String(payload.to_string());
        app.state.dialogs.push_back(dialog);
        let text = if code == egui::Key::Space { " " } else { "1" };
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 1000.0),
                )),
                events: vec![
                    key(code, egui::Modifiers::NONE),
                    egui::Event::Text(text.into()),
                ],
                ..Default::default()
            },
            |ctx| {
                context_capture::inputs(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    show(ui, "https://example.test/", &line);
                    app.agent_question(ui);
                });
            },
        );
        app.collect_context_capture();
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(app.pending_dialog.is_none());
        assert!(commands.try_recv().is_err());
        assert!(app.state.busy);
        let (label, text) = selected(&line, "https://example.test/", 7..13).unwrap();
        let captured = format!("keep\n\n[Context: {label}]\n{text}\n[/Context]");
        assert!(
            app.draft == "keep" || app.draft == captured,
            "unexpected draft: {:?}",
            app.draft
        );
        if code == egui::Key::Num1 {
            assert_eq!(app.draft, "keep");
        }
    }
}

#[test]
fn secondary_opening_frame_cannot_approve_or_deny_pending_permission() {
    for code in [egui::Key::Enter, egui::Key::Backspace] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let line = ConsoleLine::new(false, "prefix chosen suffix", "source.js", 1);
        select(&ctx, &line, 7, 13);
        frame(
            &ctx,
            &line,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        let mut app = crate::app::files::tests::app(&ctx);
        app.state.busy = true;
        app.draft = "keep".into();
        app.state.dialogs.push_back(json!({"id":"console-opening","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo safe\"}\n\nApprove this call only."}));
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let output = frame(&ctx, &line, vec![]);
        let pos = position(&output, &line.text, 9);
        frame(
            &ctx,
            &line,
            pointer(pos, egui::PointerButton::Secondary, true),
        );
        let mut events = pointer(pos, egui::PointerButton::Secondary, false);
        events.push(key(code, egui::Modifiers::COMMAND));
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 1000.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                context_capture::inputs(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    show(ui, "https://example.test/", &line);
                    app.agent_question(ui);
                });
            },
        );
        app.collect_context_capture();
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(app.pending_dialog.is_none());
        assert!(commands.try_recv().is_err());
        assert_eq!(app.draft, "keep");
        assert!(app.state.busy);
    }
}

#[test]
fn maximum_retained_message_selection_stays_below_shared_context_budget() {
    let line = ConsoleLine::new(
        false,
        &"界".repeat(MAX_CHARS + 1),
        "data:text/html;base64,PRIVATE",
        8,
    );
    let started = std::time::Instant::now();
    for _ in 0..1000 {
        let (label, text) = selected(&line, "https://example.test/", 0..MAX_CHARS).unwrap();
        assert!(text.len() < 24 * 1024);
        assert!(text.contains("limited to 4,000"));
        assert!(label.len() < 512);
        assert!(!text.contains("PRIVATE"));
    }
    eprintln!(
        "1000 maximum4000char console mappings {:?}; one snapshot only, no full-body duplicate state",
        started.elapsed()
    );
}

#[test]
fn empty_selection_is_disabled_and_new_message_id_cannot_retarget_frozen_excerpt() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let line = ConsoleLine::new(false, "prefix chosen suffix", "source.js", 1);
    select(&ctx, &line, 7, 13);
    let replacement = ConsoleLine::new(false, "different message", "other.js", 2);
    frame(&ctx, &replacement, vec![]);
    let mut app = crate::app::files::tests::app(&ctx);
    app.collect_context_capture();
    assert!(!egui::Popup::is_any_open(&ctx));
    assert!(!app.draft.contains("chosen") && !app.draft.contains("different message"));
    for _ in 0..3 {
        frame(&ctx, &replacement, vec![]);
    }
    let output = frame(&ctx, &replacement, vec![]);
    let pos = position(&output, &replacement.text, 2);
    for pressed in [true, false] {
        frame(
            &ctx,
            &replacement,
            pointer(pos, egui::PointerButton::Secondary, pressed),
        );
    }
    for _ in 0..2 {
        frame(&ctx, &replacement, vec![]);
    }
    let out = frame(&ctx, &replacement, vec![]);
    assert!(out.shapes.iter().all(|shape|!matches!(&shape.shape,egui::Shape::Text(t)if t.galley.job.text=="Add file excerpt to chat")));
}
