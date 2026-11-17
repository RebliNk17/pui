use super::*;

fn app(ctx: &egui::Context) -> Pui {
    Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_notifications: true,
            ..Default::default()
        },
    )
}

fn frame(ctx: &egui::Context, app: &mut Pui, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(860.0, 620.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| app.top_bar(ctx),
    )
}

fn labels(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    fn collect(shape: &egui::Shape, labels: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => labels.push((
                t.galley.job.text.clone(),
                egui::Rect::from_min_size(t.pos, t.galley.size()),
            )),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, labels);
                }
            }
            _ => {}
        }
    }
    let mut labels = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, &mut labels);
    }
    labels
}

fn settle(ctx: &egui::Context, app: &mut Pui) -> egui::FullOutput {
    for _ in 0..3 {
        frame(ctx, app, vec![]);
    }
    frame(ctx, app, vec![])
}

fn click(ctx: &egui::Context, app: &mut Pui, pos: egui::Pos2) {
    frame(ctx, app, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            ctx,
            app,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
}

#[test]
fn notifications_separate_session_updates_from_connection_help() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let output = settle(&ctx, &mut app);
    let text = labels(&output);
    for expected in [
        "Chat notices",
        "Session update",
        "Connection warning",
        "MCP connection help",
        "/mcp",
        "Clear all",
    ] {
        assert!(
            text.iter().any(|(s, _)| s == expected),
            "Missing {expected}"
        );
    }
    assert!(text.iter().any(|(s, _)| s == "2"));

    app.state.diagnostics.clear();
    let output = settle(&ctx, &mut app);
    let text = labels(&output);
    assert!(text.iter().any(|(s, _)| s == "Session update"));
    assert!(!text.iter().any(|(s, _)| s == "MCP connection help"));

    app.state.add_diagnostic("Extension failed to load".into());
    let text = labels(&settle(&ctx, &mut app));
    assert!(text.iter().any(|(s, _)| s == "Pi / extensions"));
    assert!(!text.iter().any(|(s, _)| s == "MCP connection help"));
}

#[test]
fn clear_all_leaves_an_accessible_empty_state_and_does_not_close_the_panel() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let output = settle(&ctx, &mut app);
    let clear = labels(&output)
        .into_iter()
        .find(|(s, _)| s == "Clear all")
        .unwrap()
        .1;
    click(&ctx, &mut app, clear.center());
    assert!(app.diagnostics_open);
    assert!(app.state.diagnostics.is_empty());
    assert!(app.state.notice.is_none());
    assert!(
        labels(&settle(&ctx, &mut app))
            .iter()
            .any(|(s, _)| s == "No chat notices")
    );
}

#[test]
fn individual_dismiss_does_not_clear_other_notifications() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let output = settle(&ctx, &mut app);
    let title = labels(&output)
        .into_iter()
        .find(|(s, _)| s == "Session update")
        .unwrap()
        .1;
    let popup = ctx
        .read_response(egui::Id::new("conversation-notifications"))
        .unwrap()
        .rect;
    // Card inset: popup padding + scrollbar clearance + card padding + half the close target.
    click(
        &ctx,
        &mut app,
        egui::pos2(popup.right() - 48.0, title.center().y),
    );
    assert!(app.state.notice.is_none());
    assert_eq!(app.state.diagnostics.len(), 1);
    assert!(app.diagnostics_open);
}

#[test]
fn escape_and_outside_click_close_without_dismissing_notifications() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    settle(&ctx, &mut app);
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(!app.diagnostics_open);
    assert!(app.state.notice.is_some());
    assert_eq!(app.state.diagnostics.len(), 1);

    app.diagnostics_open = true;
    settle(&ctx, &mut app);
    click(&ctx, &mut app, egui::pos2(20.0, 580.0));
    assert!(!app.diagnostics_open);
    assert!(app.state.notice.is_some());
    assert_eq!(app.state.diagnostics.len(), 1);
}

#[test]
fn long_notifications_stay_bounded_in_both_themes_and_at_large_scale() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        for zoom in [0.8, 1.0, 1.4] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            app.state.diagnostics = (0..20)
                .map(|i| format!("MCP server {i}: {}", "Connection unavailable. ".repeat(100)))
                .collect();
            let output = settle(&ctx, &mut app);
            let popup = ctx
                .read_response(egui::Id::new("conversation-notifications"))
                .unwrap()
                .rect;
            let screen = ctx.screen_rect();
            assert!(
                popup.left() >= screen.left(),
                "{popup:?} outside {screen:?}"
            );
            assert!(
                popup.right() <= screen.right() + 1.0,
                "{popup:?} outside {screen:?}"
            );
            assert!(
                popup.bottom() <= screen.bottom() + 1.0,
                "{popup:?} outside {screen:?}"
            );
            let clear = labels(&output)
                .into_iter()
                .find(|(s, _)| s == "Clear all")
                .unwrap()
                .1;
            assert!(popup.contains_rect(clear));
        }
    }
}
