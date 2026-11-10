use super::*;
fn labels(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(t) => out.push((
            t.galley.job.text.clone(),
            egui::Rect::from_min_size(t.pos, t.galley.size()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                labels(shape, out);
            }
        }
        _ => {}
    }
}
fn render(
    ctx: &egui::Context,
    size: Vec2,
    events: Vec<egui::Event>,
    choice: &mut Option<Choice>,
) -> egui::FullOutput {
    ctx.run(egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,size)),events,..Default::default()}, |ctx| {
        *choice = Dialog::new("fixture-confirmation", "Quit Pui?", "Agents or terminal shells are still active. Quitting will stop their work in this window.", "Keep working", "Stop all and quit").show(ctx, |ui| note(ui,"Sessions and worktrees will be kept. Running terminal commands will stop, and queued messages will be lost."));
    })
}
#[test]
fn dialogs_are_bounded_wrapped_and_have_distinct_right_aligned_actions() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        for zoom in [0.8, 1.0, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut choice = None;
            for _ in 0..3 {
                render(&ctx, egui::vec2(860.0, 620.0), vec![], &mut choice);
            }
            let output = render(&ctx, egui::vec2(860.0, 620.0), vec![], &mut choice);
            let dialog = ctx
                .read_response(egui::Id::new("fixture-confirmation"))
                .unwrap()
                .rect;
            assert!(
                ctx.screen_rect().contains_rect(dialog),
                "{dialog:?} outside screen at {zoom}"
            );
            assert!(
                dialog.width() <= 470.0,
                "No one-line full-screen dialogs: {dialog:?}"
            );
            let mut text = Vec::new();
            for s in output.shapes {
                labels(&s.shape, &mut text);
            }
            let cancel = text.iter().find(|(s, _)| s == "Keep working").unwrap().1;
            let confirm = text
                .iter()
                .find(|(s, _)| s == "Stop all and quit")
                .unwrap()
                .1;
            assert!(cancel.right() < confirm.left());
            assert!((cancel.center().y - confirm.center().y).abs() < 1.0);
            assert!(choice.is_none());
        }
    }
}
#[test]
fn escape_cancels_and_enter_does_not_default_to_destructive_action() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut choice = None;
    for _ in 0..3 {
        render(&ctx, egui::vec2(860.0, 620.0), vec![], &mut choice);
    }
    let event = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    render(
        &ctx,
        egui::vec2(860.0, 620.0),
        vec![event(egui::Key::Enter)],
        &mut choice,
    );
    assert_ne!(choice, Some(Choice::Confirm));
    render(
        &ctx,
        egui::vec2(860.0, 620.0),
        vec![event(egui::Key::Escape)],
        &mut choice,
    );
    assert_eq!(choice, Some(Choice::Cancel));
}
#[test]
fn long_details_scroll_without_moving_decision_buttons_offscreen() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    ctx.set_zoom_factor(1.4);
    for pass in 0..4 {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(860.0, 620.0),
                )),
                ..Default::default()
            },
            |ctx| {
                Dialog::new(
                    "long-confirmation",
                    "Restore files?",
                    "Review the affected files before continuing.",
                    "Cancel",
                    "Restore files",
                )
                .show(ctx, |ui| {
                    for _ in 0..100 {
                        ui.label("A captured workspace file will be restored.");
                    }
                });
            },
        );
        if pass < 3 {
            continue;
        }
        let mut text = Vec::new();
        for shape in output.shapes {
            labels(&shape.shape, &mut text);
        }
        for title in ["Cancel", "Restore files"] {
            let rect = text.iter().find(|(s, _)| s == title).unwrap().1;
            assert!(ctx.screen_rect().contains_rect(rect));
        }
    }
}
#[test]
fn quit_cancel_keeps_active_work_and_draft() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.state.busy = true;
    app.draft = "Do not lose this".into();
    app.agents.close_prompt = true;
    for _ in 0..3 {
        let _ = ctx.run(Default::default(), |ctx| app.agent_close_guard(ctx));
    }
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| app.agent_close_guard(ctx),
    );
    assert!(!app.agents.close_prompt);
    assert!(!app.agents.exiting);
    assert!(app.state.busy);
    assert_eq!(app.draft, "Do not lose this");
}
