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
fn source() -> String {
    (0..5)
        .map(|n| {
            format!(
                "@@ -{},20 +{},20 @@ fn block_{n}\n-old {n}\n+new {n}\n{}",
                n * 50 + 1,
                n * 50 + 1,
                (0..19)
                    .map(|i| format!(" context {n}-{i}\n"))
                    .collect::<String>()
            )
        })
        .collect()
}
fn frame(
    ctx: &egui::Context,
    doc: &mut Document,
    size: Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let time = ctx.input(|i| i.time) + 0.016;
    ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            context_capture::inputs(ctx);
            assert!(show_document(ctx, doc, true));
        },
    )
}
#[test]
fn actual_buttons_and_alt_shortcuts_scroll_to_hunk_without_copy_capture_or_mutation() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let source = source();
    let mut doc: Document = ("src/file.rs".into(), source.clone()).into();
    let size = Vec2::new(1100.0, 850.0);
    for _ in 0..5 {
        frame(&ctx, &mut doc, size, vec![]);
    }
    let pos = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("diff-next-hunk")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut doc,
            size,
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
    assert_eq!(doc.2.selected, Some(0));
    frame(
        &ctx,
        &mut doc,
        size,
        vec![key(egui::Key::End, egui::Modifiers::ALT)],
    );
    assert_eq!(doc.2.selected, Some(4));
    for _ in 0..100 {
        frame(&ctx, &mut doc, size, vec![]);
    }
    let out = frame(&ctx, &mut doc, size, vec![]);
    let target = doc.2.rows[4] as usize;
    let (clip, text) = out
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                text.galley
                    .job
                    .text
                    .contains("+new 4")
                    .then_some((shape.clip_rect, text))
            } else {
                None
            }
        })
        .unwrap();
    let row = &text.galley.rows[target];
    let rect = row.rect().translate(text.pos.to_vec2());
    assert!(clip.contains(rect.center()), "{clip:?} {rect:?}");
    let first_change = text.galley.rows[target + 1]
        .rect()
        .translate(text.pos.to_vec2());
    assert!(
        clip.contains(first_change.center()),
        "Target must reveal change content, not only caption"
    );
    assert!(out.platform_output.commands.is_empty());
    assert_eq!(doc.1, source);
    frame(
        &ctx,
        &mut doc,
        size,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::ALT)],
    );
    assert_eq!(doc.2.selected, Some(4));
    frame(
        &ctx,
        &mut doc,
        size,
        vec![key(egui::Key::ArrowUp, egui::Modifiers::ALT)],
    );
    assert_eq!(doc.2.selected, Some(3));
    frame(
        &ctx,
        &mut doc,
        size,
        vec![key(egui::Key::Home, egui::Modifiers::ALT)],
    );
    assert_eq!(doc.2.selected, Some(0));
}
#[test]
fn narrow_theme_zoom_controls_stay_inside_window_and_settle_after_navigation() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut doc: Document = ("very/long/path/file.rs".into(), source()).into();
            let size = Vec2::new(420.0, 560.0);
            doc.2.focus(&ctx);
            for _ in 0..5 {
                frame(&ctx, &mut doc, size, vec![]);
            }
            frame(
                &ctx,
                &mut doc,
                size,
                vec![key(egui::Key::End, egui::Modifiers::ALT)],
            );
            for _ in 0..100 {
                frame(&ctx, &mut doc, size, vec![]);
            }
            let out = frame(&ctx, &mut doc, size, vec![]);
            let window = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("diff-review-bounds")))
                .unwrap();
            for id in ["diff-previous-hunk", "diff-next-hunk"] {
                let rect = ctx
                    .data(|d| d.get_temp::<egui::Rect>(egui::Id::new(id)))
                    .unwrap();
                assert!(window.contains_rect(rect), "{window:?} {rect:?}");
            }
            assert!(
                out.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(1)
            );
            assert_eq!(doc.2.selected, Some(4));
        }
    }
}
#[test]
fn application_hunk_shortcuts_preserve_real_approval_running_agent_and_draft() {
    let ctx = egui::Context::default();
    let mut app = files::tests::app(&ctx);
    app.open_diff_review(("file.rs".into(), source()).into(), false);
    app.draft = "Keep draft".into();
    app.state.busy = true;
    let permission = json!({"id":"hunk-permission","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."});
    assert!(prompts::approval(&permission).is_some());
    app.state.dialogs.push_back(permission);
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let messages = app.state.messages.len();
    for _ in 0..4 {
        files::tests::frame(&ctx, &mut app, vec![]);
    }
    files::tests::frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::ArrowDown, egui::Modifiers::ALT),
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
        ],
    );
    assert_eq!(app.changes.review.as_ref().unwrap().2.selected, Some(0));
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.pending_dialog.is_none());
    assert!(app.state.busy);
    assert_eq!(app.draft, "Keep draft");
    assert_eq!(app.state.messages.len(), messages);
    assert!(commands.try_recv().is_err());
}
