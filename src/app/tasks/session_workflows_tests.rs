use super::*;

#[test]
fn workflows_have_one_collapsible_inspector_entry_without_duplicate_button() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let mut app = app();
            theme.apply(&app.ctx);
            app.ctx.set_zoom_factor(zoom);
            proposal(&mut app);
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
            let order = app.prefs.cards.order.clone();
            let output = inspector(&mut app, vec![]);
            let text = labels(&output);
            assert!(!text.iter().any(|(text, _)| text == "Workflows"));
            assert_eq!(
                text.iter()
                    .filter(|(text, _)| text == Card::Workflow.label())
                    .count(),
                1
            );
            assert!(
                ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new((
                    "inspector-header-action",
                    Card::Workflow
                ))))
                .is_some()
            );
            assert!(!app.tasks.open);
            assert_eq!(app.prefs.cards.order, order);
            assert!(app.tasks.processes.is_empty());
            app.state.session_file = Some("/fixture/unrelated-session.jsonl".into());
            let output = inspector(&mut app, vec![]);
            assert!(
                !labels(&output)
                    .iter()
                    .any(|(text, _)| text == "Workflows" || text == Card::Workflow.label())
            );
        }
    }
}
