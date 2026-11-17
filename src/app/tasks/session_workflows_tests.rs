use super::*;

#[test]
fn workflows_action_lives_in_session_info_not_top_toolbar() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let mut app = app();
            theme.apply(&app.ctx);
            app.ctx.set_zoom_factor(zoom);
            proposal(&mut app);
            let id = app.tasks.selected.clone();
            app.tasks.open = false;
            let ctx = app.ctx.clone();
            let top = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1100.0, 800.0),
                    )),
                    ..Default::default()
                },
                |ctx| app.top_bar(ctx),
            );
            assert!(!labels(&top).iter().any(|(text, _)| text == "Workflows"));
            for _ in 0..4 {
                inspector(&mut app, vec![]);
            }
            let (rect, enabled) = ctx
                .data(|data| {
                    data.get_temp::<(egui::Rect, bool)>(egui::Id::new("session-workflows-button"))
                })
                .unwrap();
            assert!(enabled);
            assert!(ctx.screen_rect().contains_rect(rect));
            let order = app.prefs.cards.order.clone();
            for pressed in [true, false] {
                inspector(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::PointerButton {
                            pos: rect.center(),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert!(app.tasks.open);
            assert_eq!(app.tasks.selected, id);
            assert_eq!(app.prefs.cards.order, order);
            assert!(
                app.tasks.processes.is_empty(),
                "Opening evidence never starts a workflow"
            );
            app.tasks.open = false;
            app.state.session_file = Some("/fixture/unrelated-session.jsonl".into());
            for _ in 0..4 {
                inspector(&mut app, vec![]);
            }
            let (_, enabled) = ctx
                .data(|data| {
                    data.get_temp::<(egui::Rect, bool)>(egui::Id::new("session-workflows-button"))
                })
                .unwrap();
            assert!(
                !enabled,
                "Another session must not expose the previous workflow"
            );
        }
    }
}
