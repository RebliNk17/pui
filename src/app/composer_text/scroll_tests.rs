use super::tests::{app, key, select};
use super::*;

fn metrics(ctx: &egui::Context) -> (egui::Rect, egui::Vec2, f32) {
    ctx.data(|d| d.get_temp(egui::Id::new("composer-scroll-metrics")))
        .unwrap()
}

fn frame(
    ctx: &egui::Context,
    app: &mut Pui,
    events: Vec<egui::Event>,
    size: egui::Vec2,
) -> egui::FullOutput {
    if app.state.messages.is_empty() {
        app.state.messages.push(Message {
            role: "assistant".into(),
            text: "Conversation remains visible".into(),
            ..Default::default()
        });
    }
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
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
        |ctx| app.conversation(ctx),
    )
}

#[test]
fn long_drafts_have_a_bounded_scrolling_editor_and_visible_bottom_controls() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        for zoom in [1.0, 1.5] {
            for size in [egui::vec2(500.0, 500.0), egui::vec2(1100.0, 900.0)] {
                let ctx = egui::Context::default();
                let mut app = app(&ctx);
                theme.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                app.draft = (0..160)
                    .map(|n| {
                        format!("Line {n} — a long draft with **formatting** and code `value`.\n")
                    })
                    .collect();
                let original = app.draft.clone();
                app.focus_composer = false;
                let mut output = frame(&ctx, &mut app, vec![], size);
                for _ in 0..2 {
                    output = frame(&ctx, &mut app, vec![], size);
                }
                let (viewport, content, _) = metrics(&ctx);
                assert!(viewport.height() <= 240.0);
                assert!(viewport.height() <= ctx.screen_rect().height() * 0.3 + 1.0);
                assert!(content.y > viewport.height() * 3.0);
                assert!(
                    app.transcript.viewport > 80.0,
                    "draft must not consume the conversation"
                );
                let send = ctx
                    .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("composer-send-rect")))
                    .expect("send control is laid out");
                assert!(send.bottom() <= ctx.screen_rect().bottom() + 1.0);
                assert!(send.top() > viewport.bottom(), "controls stay below editor");
                assert!(
                    !output.shapes.iter().any(|s| matches!(
                        &s.shape,
                        egui::Shape::Text(t) if t.galley.text().contains("Shift+Enter: new line")
                            || t.galley.text().starts_with("to send")
                    )),
                    "drafts have no footer or empty-state hint"
                );
                assert_eq!(app.draft, original);
                assert!(app.deliveries.is_empty());
            }
        }
    }
}

#[test]
fn typing_at_the_end_reveals_the_caret_and_wheel_scroll_does_not_move_the_chat() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let size = egui::vec2(1000.0, 750.0);
    app.focus_composer = false;
    frame(&ctx, &mut app, vec![], size);
    select(&ctx, 0..0);
    let draft = (0..90)
        .map(|n| format!("Log line {n}\n"))
        .collect::<String>();
    frame(&ctx, &mut app, vec![egui::Event::Paste(draft)], size);
    frame(&ctx, &mut app, vec![], size);
    let (viewport, content, bottom) = metrics(&ctx);
    assert!(bottom > 100.0);
    assert!(
        bottom + viewport.height() >= content.y - 3.0,
        "paste caret must be visible at the bottom: offset={bottom} viewport={viewport:?} content={content:?}"
    );
    let transcript_scroll = app.transcript.scroll;
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(viewport.center()),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 100.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
        size,
    );
    frame(&ctx, &mut app, vec![], size);
    assert!(
        metrics(&ctx).2 < bottom - 10.0,
        "wheel must scroll inside editor"
    );
    assert_eq!(app.transcript.scroll, transcript_scroll);
    let end = app.draft.chars().count();
    select(&ctx, end..end);
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("last line".into())],
        size,
    );
    // Stop the wheel's smoothed tail before testing a fresh typing event.
    for _ in 0..12 {
        frame(&ctx, &mut app, vec![], size);
    }
    frame(&ctx, &mut app, vec![egui::Event::Text("!".into())], size);
    frame(&ctx, &mut app, vec![], size);
    let (viewport, content, offset) = metrics(&ctx);
    assert!(
        offset + viewport.height() >= content.y - 3.0,
        "typing follows caret: offset={offset} viewport={viewport:?} content={content:?}"
    );
    assert!(app.draft.ends_with("last line!"));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::SHIFT)],
        size,
    );
    assert!(app.draft.ends_with("last line!\n"));
    assert!(app.deliveries.is_empty(), "Shift+Enter never sends");
}

#[test]
fn history_restoration_reveals_the_long_draft_caret_without_typing() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let size = egui::vec2(1000.0, 750.0);
    app.state.messages.push(Message {
        role: "user".into(),
        text: "previous short prompt".into(),
        ..Default::default()
    });
    app.draft = "draft line\n".repeat(100);
    let original = app.draft.clone();
    app.focus_composer = false;
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![], size);
    }
    select(&ctx, 0..0);
    frame(&ctx, &mut app, vec![], size);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::ArrowUp, egui::Modifiers::NONE)],
        size,
    );
    assert_eq!(app.draft, "previous short prompt");
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
        size,
    );
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![], size);
    }
    assert_eq!(app.draft, original);
    let (viewport, _, offset) = metrics(&ctx);
    let caret = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("composer-caret-rect")))
        .unwrap();
    assert!(offset > 100.0);
    assert!(
        caret.top() >= viewport.top() - 4.0 && caret.bottom() <= viewport.bottom() + 4.0,
        "restored caret {caret:?} must be visible in {viewport:?}"
    );
    assert!(app.deliveries.is_empty());
}

#[test]
fn short_or_cleared_drafts_shrink_and_scrolled_markdown_is_clipped_to_the_editor() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let size = egui::vec2(1000.0, 750.0);
    app.focus_composer = false;
    app.draft = "```text\n".to_owned() + &"code line\n".repeat(100) + "```";
    let mut output = frame(&ctx, &mut app, vec![], size);
    for _ in 0..2 {
        output = frame(&ctx, &mut app, vec![], size);
    }
    let end = app.draft.chars().count();
    select(&ctx, end..end);
    for _ in 0..3 {
        output = frame(&ctx, &mut app, vec![], size);
    }
    let (viewport, _, offset) = metrics(&ctx);
    assert!(
        offset > 100.0,
        "decoration check must exercise a scrolled editor"
    );
    let code_color = theme::palette(&ctx).code_bg;
    fn code_rect(shape: &egui::Shape, color: Color32) -> bool {
        match shape {
            egui::Shape::Rect(r) => r.fill == color && r.rect.height() > 50.0,
            egui::Shape::Vec(v) => v.iter().any(|s| code_rect(s, color)),
            _ => false,
        }
    }
    let decoration = output
        .shapes
        .iter()
        .find(|s| code_rect(&s.shape, code_color))
        .expect("code decoration painted");
    assert!(
        decoration.clip_rect.top() >= viewport.top() - 4.0,
        "clip={:?} viewport={viewport:?}",
        decoration.clip_rect
    );
    assert!(
        decoration.clip_rect.bottom() <= viewport.bottom() + 4.0,
        "clip={:?} viewport={viewport:?}",
        decoration.clip_rect
    );
    select(&ctx, 0..0);
    app.draft = "short".into();
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![], size);
    }
    let (viewport, content, offset) = metrics(&ctx);
    assert!(
        viewport.height() < 45.0,
        "short draft must not keep max height: {viewport:?}, offset={offset}, content={content:?}, draft={:?}",
        app.draft
    );
    assert_eq!(offset, 0.0);
    app.draft.clear();
    for _ in 0..2 {
        frame(&ctx, &mut app, vec![], size);
    }
    assert!(metrics(&ctx).0.height() < 45.0);
}
