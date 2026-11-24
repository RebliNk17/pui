use super::*;
use crate::app::files::tests::{app, frame};

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
fn click(ctx: &egui::Context, app: &mut Pui, pos: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            ctx,
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}
fn remove_rect(ctx: &egui::Context, id: u64) -> egui::Rect {
    ctx.data(|d| {
        d.get_temp::<egui::Rect>(
            egui::Id::new("composer-quote-card")
                .with(id)
                .with("remove-rect"),
        )
    })
    .unwrap()
}

#[test]
fn cards_render_instead_of_raw_blocks_and_remove_without_sending() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.busy = true;
    app.draft = "Keep my draft".into();
    for (label, text) in [
        ("src/frozen.rs:4–5 · selected source", "chosen frozen 世界"),
        ("Terminal · visible viewport · /demo", "$ example output"),
    ] {
        queue(&ctx, label, text);
        app.collect_context_capture();
    }
    let mut texts = Vec::new();
    for _ in 0..4 {
        let out = frame(&ctx, &mut app, vec![]);
        texts.clear();
        fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
                _ => {}
            }
        }
        for s in &out.shapes {
            collect(&s.shape, &mut texts);
        }
    }
    assert_eq!(app.draft, "Keep my draft");
    assert!(texts.iter().any(|t| t == "src/frozen.rs:4–5"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "chosen frozen 世界"));
    assert!(
        !texts.iter().any(|t| t.contains("[Context:")),
        "no raw blocks in the composer"
    );
    let ids = app.context_chips.quote_ids();
    click(&ctx, &mut app, remove_rect(&ctx, ids[0]).center());
    assert_eq!(app.context_chips.quote_ids(), vec![ids[1]]);
    assert_eq!(app.draft, "Keep my draft");
    assert!(app.state.busy);
    assert!(app.deliveries.is_empty());
    assert!(commands.try_recv().is_err());
}

#[test]
fn backspace_removes_last_card_only_from_an_empty_focused_composer() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.draft = "x".into();
    for text in ["first", "second"] {
        queue(&ctx, "console", text);
        app.collect_context_capture();
    }
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let composer = composer_text::widget_id(&ctx, "composer");
    ctx.memory_mut(|m| m.request_focus(composer));
    frame(&ctx, &mut app, vec![]);
    // Non-empty draft: Backspace edits text, cards stay.
    frame(&ctx, &mut app, vec![key(egui::Key::Backspace)]);
    assert_eq!(app.context_chips.quote_ids().len(), 2);
    assert!(app.draft.is_empty());
    frame(&ctx, &mut app, vec![key(egui::Key::Backspace)]);
    assert_eq!(app.context_chips.labels().len(), 1);
    assert!(app.outgoing_text().contains("first"));
    assert!(!app.outgoing_text().contains("second"));
}

#[test]
fn explicit_send_includes_cards_exactly_once_then_clears_them() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.demo = false;
    app.connected = true;
    app.ultracode_ready = true;
    app.state = State::default();
    app.state.session_file = Some("/fixture/chat.jsonl".into());
    app.client = Some(rpc::Client::stub());
    app.draft = "Explain".into();
    queue(&ctx, "src/a.rs:1–1 · selected source", "fn a() {}");
    app.collect_context_capture();
    app.submit(&ctx);
    assert_eq!(app.deliveries.len(), 1);
    assert_eq!(
        app.deliveries[0].message.text,
        "Explain\n\n[Context: src/a.rs:1–1 · selected source]\nfn a() {}\n[/Context]"
    );
    assert!(app.draft.is_empty());
    assert!(!app.context_chips.has_quotes());
}

#[test]
fn restart_checkpoint_keeps_cards_and_restores_them_as_cards() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.demo = false;
    app.new_chat = true;
    app.state = State::default();
    app.draft = "Keep checkpoint draft".into();
    queue(&ctx, "console", "private literal capture");
    app.collect_context_capture();
    app.checkpoint_chats();
    let json = serde_json::to_string(&app.prefs.restart_checkpoints).unwrap();
    assert_eq!(json.matches("private literal capture").count(), 1);
    app.restart_pending = app.prefs.restart_checkpoints.clone();
    app.draft.clear();
    app.context_chips.clear_quotes();
    app.restore_blank_checkpoint();
    assert_eq!(app.draft, "Keep checkpoint draft");
    assert_eq!(app.context_chips.labels(), vec!["console"]);
    assert!(app.deliveries.is_empty());
}

#[test]
fn long_paste_becomes_a_card_and_short_paste_stays_text() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.draft.clear();
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    ctx.memory_mut(|m| m.request_focus(composer_text::widget_id(&ctx, "composer")));
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![egui::Event::Paste("short".into())]);
    assert_eq!(app.draft, "short");
    assert!(!app.context_chips.has_quotes());
    let long = (1..=9)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\r\n");
    frame(&ctx, &mut app, vec![egui::Event::Paste(long.clone())]);
    for _ in 0..2 {
        frame(&ctx, &mut app, vec![]);
    }
    assert_eq!(app.draft, "short", "long paste never enters the text box");
    assert_eq!(
        app.context_chips.labels(),
        vec!["Pasted text · 9 lines · 62 chars"]
    );
    assert_eq!(
        app.outgoing_text(),
        format!(
            "short\n\n[Context: Pasted text · 9 lines · 62 chars]\n{}\n[/Context]",
            long.replace("\r\n", "\n")
        )
    );
}
