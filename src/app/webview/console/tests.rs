use super::*;

#[test]
fn bounded_console_records_keep_stable_ids_order_and_shared_strings() {
    let mut state = PageState::default();
    for index in 0..350 {
        push(
            &mut state,
            index % 2 == 0,
            &format!("record {index}"),
            "http://localhost/script.js",
            7,
        );
    }
    assert_eq!(state.console.len(), MAX_LINES);
    assert_eq!(state.console.front().unwrap().text.as_ref(), "record 50");
    let cloned = state.clone();
    let first = state.console.front().unwrap();
    let last = state.console.back().unwrap();
    assert_ne!(first.id, last.id);
    assert!(Arc::ptr_eq(
        &first.text,
        &cloned.console.front().unwrap().text
    ));
    let line = ConsoleLine::new(true, &"界".repeat(100_000), &"src".repeat(100_000), 1);
    assert!(line.limited);
    assert_eq!(
        line.text.chars().count() + line.source.chars().count(),
        MAX_CHARS
    );
    assert!(line.text.len() + line.source.len() <= MAX_CHARS * 4);
    let (label, text) = line.snippet("https://example.test/");
    assert!(label.contains("error") && label.contains("location unavailable"));
    let source = ConsoleLine::new(false, "short", &"界".repeat(10_000), 9);
    assert_eq!(
        source.text.chars().count() + source.source.chars().count(),
        MAX_CHARS
    );
    assert!(source.limited);
    assert!(text.ends_with("limited to 4,000 characters total]"));
    assert!(text.len() < 24 * 1024);
}

#[test]
fn full_console_buffer_retention_is_bounded_and_snapshot_clones_share_payload() {
    let mut state = PageState {
        location: format!("https://localhost/{}", "x".repeat(6000)),
        ..Default::default()
    };
    let message = "界".repeat(MAX_CHARS);
    let started = std::time::Instant::now();
    for _ in 0..MAX_LINES + 50 {
        push(&mut state, false, &message, "source.js", 4);
    }
    let bytes = state
        .console
        .iter()
        .map(|line| line.text.len() + line.source.len() + line.page.len())
        .sum::<usize>();
    assert_eq!(state.console.len(), MAX_LINES);
    assert!(bytes <= MAX_LINES * (MAX_CHARS * 4 + 1024));
    let elapsed = started.elapsed();
    let started = std::time::Instant::now();
    for _ in 0..1000 {
        let cloned = std::hint::black_box(state.clone());
        assert!(Arc::ptr_eq(&state.console[0].text, &cloned.console[0].text));
    }
    eprintln!(
        "300-line console: {bytes} retained UTF-8 bytes, fill {elapsed:?}, 1000 shared snapshots {:?}",
        started.elapsed()
    );
    state.console.clear();
    assert!(state.console.is_empty());
}

#[test]
fn capture_provenance_keeps_literal_message_but_never_attaches_inline_data_document() {
    let mut state = PageState {
        location: "https://localhost/current".into(),
        ..Default::default()
    };
    push(
        &mut state,
        true,
        "literal <b>oops</b> ; run?",
        "https://localhost/app.js",
        42,
    );
    state.location = "https://localhost/after-navigation".into();
    let (label, text) = state.console[0].snippet("https://localhost/");
    assert!(label.contains("Browser console error · https://localhost/current"));
    assert_eq!(
        text,
        "literal <b>oops</b> ; run?\nSource: https://localhost/app.js:42"
    );
    let line = ConsoleLine::new(
        false,
        "message",
        "data:text/html;base64,PRIVATE_DOCUMENT",
        8,
    );
    let (label, text) = line.snippet("/tmp/private.html");
    assert!(label.contains("local HTML preview"));
    assert!(!label.contains("PRIVATE_DOCUMENT"));
    assert_eq!(text, "message\nSource: [inline local HTML]:8");
    assert!(!line.source.contains("PRIVATE_DOCUMENT"));
}

pub(super) fn frame(
    ctx: &egui::Context,
    state: &PageState,
    browser: &mut BrowserView,
    events: Vec<egui::Event>,
    key: &str,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(650.0, 700.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                browser.viewport_with_state(ui, key, state.clone());
            });
        },
    )
}
pub(super) fn pointer(position: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(position),
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
pub(super) fn button(ctx: &egui::Context, key: &str, line: &ConsoleLine) -> egui::Rect {
    ctx.data(|data| {
        data.get_temp::<egui::Rect>(egui::Id::new(("console-context-action", key, line.id)))
    })
    .unwrap()
}
#[test]
fn actual_console_action_appends_only_that_message_once_without_send_or_cef() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let mut app = crate::app::files::tests::app(&ctx);
        theme::install(&ctx);
        mode.apply(&ctx);
        let key = "http://localhost/fixture";
        let mut state = PageState {
            location: "http://localhost/current".into(),
            ..Default::default()
        };
        push(
            &mut state,
            true,
            "CHOSEN error 世界",
            "http://localhost/a.js",
            4,
        );
        push(
            &mut state,
            false,
            "OTHER message",
            "http://localhost/b.js",
            9,
        );
        let mut browser = BrowserView::default();
        browser.options.insert(
            key.into(),
            Options {
                console: true,
                ..Default::default()
            },
        );
        for _ in 0..3 {
            frame(&ctx, &state, &mut browser, vec![], key);
        }
        let output = frame(&ctx, &state, &mut browser, vec![], key);
        let starts: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text)
                    if text.galley.job.text == "CHOSEN error 世界"
                        || text.galley.job.text == "OTHER message" =>
                {
                    Some(text.pos.x)
                }
                _ => None,
            })
            .collect();
        assert_eq!(starts.len(), 2);
        assert!(
            (starts[0] - starts[1]).abs() < 0.5,
            "Console rows must be left aligned"
        );
        app.draft = "Keep draft".into();
        app.context_chips.clear_quotes();
        app.state.busy = true;
        let count = app.state.messages.len();
        let (client, _events, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let target = button(&ctx, key, &state.console[0]).center();
        for pressed in [true, false] {
            frame(&ctx, &state, &mut browser, pointer(target, pressed), key);
        }
        app.collect_context_capture();
        assert!(app.outgoing_text().starts_with(
            "Keep draft\n\n[Context: Browser console error · http://localhost/current"
        ));
        assert!(
            app.outgoing_text()
                .contains("CHOSEN error 世界\nSource: http://localhost/a.js:4")
        );
        assert!(!app.outgoing_text().contains("OTHER"));
        let draft = app.outgoing_text().clone();
        app.collect_context_capture();
        assert_eq!(app.outgoing_text(), draft);
        assert!(app.state.busy);
        assert_eq!(app.state.messages.len(), count);
        assert!(commands.try_recv().is_err());
        assert_eq!(state.console.len(), 2);
    }
}

#[test]
fn focused_console_action_keyboard_add_is_not_an_approval_shortcut() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let key = "http://localhost/keyboard";
    let mut state = PageState {
        location: key.into(),
        ..Default::default()
    };
    push(&mut state, false, "Keyboard message", "a.js", 1);
    let mut browser = BrowserView::default();
    browser.options.insert(
        key.into(),
        Options {
            console: true,
            ..Default::default()
        },
    );
    for _ in 0..3 {
        frame(&ctx, &state, &mut browser, vec![], key);
    }
    let rect = button(&ctx, key, &state.console[0]);
    let response = ctx.memory(|memory| memory.focused());
    assert!(response.is_none());
    // Pointer down claims the icon's real focus; Escape is not needed because no popup exists.
    for pressed in [true, false] {
        frame(
            &ctx,
            &state,
            &mut browser,
            pointer(rect.center(), pressed),
            key,
        );
    }
    let mut app = crate::app::files::tests::app(&ctx);
    app.collect_context_capture();
    app.draft = "Kept".into();
    app.context_chips.clear_quotes();
    let focus = ctx
        .data(|data| {
            data.get_temp::<egui::Id>(egui::Id::new((
                "console-context-focus",
                key,
                state.console[0].id,
            )))
        })
        .unwrap();
    ctx.memory_mut(|memory| memory.request_focus(focus));
    let event = |modifiers| egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    frame(
        &ctx,
        &state,
        &mut browser,
        vec![event(egui::Modifiers::COMMAND)],
        key,
    );
    app.collect_context_capture();
    assert_eq!(app.outgoing_text(), "Kept");
    ctx.memory_mut(|memory| memory.request_focus(focus));
    frame(
        &ctx,
        &state,
        &mut browser,
        vec![event(egui::Modifiers::NONE)],
        key,
    );
    app.collect_context_capture();
    assert!(app.outgoing_text().contains("Keyboard message"));
    app.draft = "Kept".into();
    app.context_chips.clear_quotes();
    let permission = json!({"id":"console-permission", "method":"confirm", "title":"Allow bash?", "message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."});
    assert!(prompts::approval(&permission).is_some());
    app.state.dialogs.push_back(permission);
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    ctx.memory_mut(|memory| memory.request_focus(focus));
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(650.0, 700.0),
            )),
            events: vec![event(egui::Modifiers::COMMAND)],
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                browser.viewport_with_state(ui, key, state.clone());
                app.agent_question(ui);
            });
        },
    );
    app.collect_context_capture();
    assert_eq!(app.outgoing_text(), "Kept");
    assert!(app.pending_dialog.is_none());
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(commands.try_recv().is_err());
}

#[test]
fn identical_messages_and_rotating_bounded_queue_do_not_retarget_pointer_actions() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let key = "http://localhost/fixture";
    let mut state = PageState::default();
    push(&mut state, false, "duplicate", "a.js", 1);
    push(&mut state, false, "duplicate", "b.js", 2);
    let mut browser = BrowserView::default();
    browser.options.insert(
        key.into(),
        Options {
            console: true,
            ..Default::default()
        },
    );
    for _ in 0..3 {
        frame(&ctx, &state, &mut browser, vec![], key);
    }
    let first = button(&ctx, key, &state.console[0]);
    let second = button(&ctx, key, &state.console[1]);
    assert!(first.bottom() <= second.top());
    frame(
        &ctx,
        &state,
        &mut browser,
        pointer(first.center(), true),
        key,
    );
    state.console.pop_front();
    push(&mut state, false, "replacement", "c.js", 3);
    frame(
        &ctx,
        &state,
        &mut browser,
        pointer(first.center(), false),
        key,
    );
    let mut app = crate::app::files::tests::app(&ctx);
    app.draft = "Kept".into();
    app.context_chips.clear_quotes();
    app.collect_context_capture();
    assert_eq!(
        app.outgoing_text(),
        "Kept",
        "A removed row cannot transfer its press to another message"
    );
}
