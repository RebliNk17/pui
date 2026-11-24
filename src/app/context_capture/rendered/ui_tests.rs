use super::tests::{pointer, position, range_in, text};
use super::*;
use crate::app::files::tests::{app, frame};

const REPLY: &str = "Prefix **chosen** [link](https://example.test) `literal` suffix.\n\nSecond paragraph not included.";
fn setup(blocks: bool) -> (egui::Context, Pui) {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.prefs.hide_sidebar = true;
    app.prefs.hide_inspector = true;
    let message = Message {
        role: "assistant".into(),
        text: REPLY.into(),
        blocks: if blocks {
            vec![protocol::Block::Text(REPLY.into())]
        } else {
            vec![]
        },
        timestamp: 100,
        ..Default::default()
    };
    app.state.messages = vec![message];
    app.state.prose_revision = app.state.prose_revision.wrapping_add(1);
    app.state.transcript_revision = app.state.transcript_revision.wrapping_add(1);
    app.draft = "Kept draft".into();
    app.state.busy = true;
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app)
}
fn open(ctx: &egui::Context, app: &mut Pui) {
    let output = frame(ctx, app, vec![]);
    let needle = "Prefix chosen link literal suffix.";
    let range = range_in(
        &text(&output, needle).galley.job.text,
        "chosen link literal",
    );
    let start = position(&output, needle, range.start);
    let end = position(&output, needle, range.end);
    frame(ctx, app, pointer(start, egui::PointerButton::Primary, true));
    frame(ctx, app, vec![egui::Event::PointerMoved(end)]);
    frame(ctx, app, pointer(end, egui::PointerButton::Primary, false));
    for pressed in [true, false] {
        frame(
            ctx,
            app,
            pointer(start, egui::PointerButton::Secondary, pressed),
        );
    }
    for _ in 0..3 {
        frame(ctx, app, vec![]);
    }
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
#[test]
fn actual_transcript_block_and_legacy_reply_selection_preserve_approval_run_and_draft() {
    for blocks in [false, true] {
        let (ctx, mut app) = setup(blocks);
        let permission = json!({"id":"quote-permission", "method":"confirm", "title":"Allow bash?", "message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."});
        assert!(prompts::approval(&permission).is_some());
        app.state.dialogs.push_back(permission);
        let (client, _events, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        open(&ctx, &mut app);
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
        );
        assert_eq!(app.outgoing_text(), "Kept draft");
        assert!(app.pending_dialog.is_none());
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert_eq!(app.draft, "Kept draft");
        assert_eq!(
            app.outgoing_text(),
            "Kept draft\n\n[Context: Assistant reply · selected rendered text (not Markdown source)]\nchosen link literal\n[/Context]"
        );
        assert!(app.state.busy);
        assert_eq!(app.state.messages.len(), 1);
        assert!(commands.try_recv().is_err());
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(app.pending_dialog.is_none());
    }
}
#[test]
fn quote_escape_and_session_switch_drop_snapshot_without_stopping_or_sending() {
    let (ctx, mut app) = setup(false);
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    open(&ctx, &mut app);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!egui::Popup::is_any_open(&ctx));
    assert_eq!(app.outgoing_text(), "Kept draft");
    assert!(app.state.busy);
    open(&ctx, &mut app);
    app.leave_session_layout();
    assert!(!egui::Popup::is_any_open(&ctx));
    assert_eq!(app.outgoing_text(), "Kept draft");
    assert!(commands.try_recv().is_err());
}
#[test]
fn markdown_file_preview_capture_labels_rendered_origin_without_claiming_source_lines() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.prefs.hide_sidebar = true;
    app.prefs.hide_inspector = true;
    let root = app.prefs.cwd.clone();
    let path = root.join("capture.md");
    std::fs::write(&path, REPLY).unwrap();
    app.files
        .open_target(&ctx, &root, path.to_str().unwrap())
        .unwrap();
    for _ in 0..5 {
        frame(&ctx, &mut app, vec![]);
    }
    app.draft = "Kept draft".into();
    open(&ctx, &mut app);
    let target = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut app,
            pointer(target, egui::PointerButton::Primary, pressed),
        );
    }
    assert!(app.outgoing_text().contains(&format!(
        "Markdown preview {} · selected rendered text",
        path.display()
    )));
    assert!(
        app.outgoing_text()
            .ends_with("\nchosen link literal\n[/Context]")
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), REPLY);
}

/// Selecting text (no right-click) opens the quote popover; typing a question and Enter
/// quotes + sends when the draft was empty, ⌥Enter only fills the draft, and an existing
/// draft is never sent implicitly.
#[test]
fn selection_popover_asks_sends_or_fills_draft() {
    fn select(ctx: &egui::Context, app: &mut Pui) {
        let output = frame(ctx, app, vec![]);
        let needle = "Prefix chosen link literal suffix.";
        let range = range_in(&text(&output, needle).galley.job.text, "chosen");
        let start = position(&output, needle, range.start);
        let end = position(&output, needle, range.end);
        frame(ctx, app, pointer(start, egui::PointerButton::Primary, true));
        frame(ctx, app, vec![egui::Event::PointerMoved(end)]);
        frame(ctx, app, pointer(end, egui::PointerButton::Primary, false));
        for _ in 0..2 {
            frame(ctx, app, vec![]);
        }
    }
    for (draft, alt, expect_send) in [("", false, true), ("", true, false), ("Keep", false, false)]
    {
        let (ctx, mut app) = setup(true);
        app.demo = false;
        app.connected = true;
        app.ultracode_ready = true;
        app.state.busy = false;
        app.state.session_file = Some("/fixture/chat.jsonl".into());
        app.client = Some(rpc::Client::stub());
        app.draft = draft.into();
        select(&ctx, &mut app);
        assert!(
            egui::Popup::is_any_open(&ctx),
            "selection opens the popover"
        );
        frame(&ctx, &mut app, vec![egui::Event::Text("Why?".into())]);
        let modifiers = if alt {
            egui::Modifiers::ALT
        } else {
            egui::Modifiers::NONE
        };
        frame(&ctx, &mut app, vec![key(egui::Key::Enter, modifiers)]);
        for _ in 0..2 {
            frame(&ctx, &mut app, vec![]);
        }
        let quote = "[Context: Assistant reply · selected rendered text (not Markdown source)]\nchosen\n[/Context]";
        if expect_send {
            assert_eq!(app.deliveries.len(), 1, "Enter with a question sends");
            assert_eq!(app.deliveries[0].message.text, format!("Why?\n\n{quote}"));
            assert!(app.draft.is_empty());
            assert!(
                !app.context_chips.has_quotes(),
                "sent quotes leave the composer"
            );
        } else {
            assert!(
                app.deliveries.is_empty(),
                "nothing sent (alt={alt}, draft={draft:?})"
            );
            let typed = if draft.is_empty() {
                "Why?".to_owned()
            } else {
                format!("{draft}\n\nWhy?")
            };
            assert_eq!(app.draft, typed);
            assert_eq!(app.outgoing_text(), format!("{typed}\n\n{quote}"));
        }
    }
}

#[test]
fn composer_quote_card_removes_with_close_or_backspace_on_empty_composer() {
    let (ctx, mut app) = setup(true);
    app.draft.clear();
    for text in ["first quote", "second quote"] {
        crate::app::context_capture::queue(&ctx, "Quoted assistant reply", text);
        app.collect_context_capture();
    }
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(app.outgoing_text().contains("first quote"));
    assert!(app.draft.is_empty(), "quotes never enter the typed draft");
    // × on the first card.
    let id = app.context_chips.quote_ids()[0];
    let rect = ctx
        .data(|d| {
            d.get_temp::<egui::Rect>(
                egui::Id::new("composer-quote-card")
                    .with(id)
                    .with("remove-rect"),
            )
        })
        .unwrap();
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut app,
            pointer(rect.center(), egui::PointerButton::Primary, pressed),
        );
    }
    assert!(!app.outgoing_text().contains("first quote"));
    assert!(app.outgoing_text().contains("second quote"));
    // Backspace in the empty, focused composer removes the remaining quote.
    ctx.memory_mut(|m| m.request_focus(composer_text::widget_id(&ctx, "composer")));
    frame(&ctx, &mut app, vec![]);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Backspace, egui::Modifiers::NONE)],
    );
    assert!(!app.context_chips.has_quotes());
}
