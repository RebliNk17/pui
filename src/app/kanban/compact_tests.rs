use super::*;

fn fixture(status: Status, long: bool) -> AgentCard {
    AgentCard {
        target: Target::Current,
        title: if long {
            "A long agent objective that should take at most two lines. ".repeat(12)
        } else {
            "Review UI interactions".into()
        },
        project: PathBuf::from("/workspace/pui"),
        status,
        activity: if long {
            "bash · export PATH=/some/very/long/path\ncommand output and more arguments ".repeat(20)
        } else {
            "Working…".into()
        },
        model: "GPT-6.1 Sol".into(),
        parent: None,
        action: "Review approval",
        path: None,
    }
}

#[test]
fn kanban_cards_have_bounded_compact_heights_and_preserve_parent_spacing() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        for width in [230.0, 360.0] {
            for (status, long, parent) in [
                (Status::Working, false, false),
                (Status::Idle, false, false),
                (Status::Working, true, true),
                (Status::Waiting, true, true),
                (Status::Error, true, false),
            ] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                theme.apply(&ctx);
                let mut card = fixture(status, long);
                if parent {
                    card.parent = Some("Parent task with a very long objective".repeat(10));
                }
                let _ = ctx.run(egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width + 16.0, 800.0))),
                    ..Default::default()
                }, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.set_width(width);
                        let spacing = ui.spacing().clone();
                        let top = ui.cursor().top();
                        agent_card(ui, &card, Duration::from_secs(3));
                        let height = ui.cursor().top() - top - spacing.item_spacing.y;
                        let limit = if matches!(status, Status::Waiting | Status::Error) { 150.0 } else if long { 130.0 } else { 95.0 };
                        assert!(height <= limit, "{theme:?} {status:?} width={width}, long={long}, height={height} > {limit}");
                        assert_eq!(ui.spacing().item_spacing, spacing.item_spacing);
                        assert_eq!(ui.spacing().interact_size, spacing.interact_size);
                        assert!(ui.min_rect().width() <= width + 1.0, "Card must not widen its column");
                    });
                });
            }
        }
    }
}

#[test]
fn compact_attention_card_is_still_clickable_without_approving_anything() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let card = fixture(Status::Waiting, true);
    let mut rect = egui::Rect::NOTHING;
    let mut clicked = false;
    let mut frame = |events| {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(320.0, 400.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let top = ui.cursor().top();
                    clicked = agent_card(ui, &card, Duration::ZERO);
                    rect = egui::Rect::from_min_max(
                        egui::pos2(ui.min_rect().left(), top),
                        egui::pos2(
                            ui.min_rect().right(),
                            ui.cursor().top() - ui.spacing().item_spacing.y,
                        ),
                    );
                });
            },
        );
        (rect, clicked)
    };
    frame(vec![]);
    let (rect, _) = frame(vec![]);
    let pos = rect.center();
    frame(vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
    ]);
    assert!(
        frame(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }])
        .1
    );
}
