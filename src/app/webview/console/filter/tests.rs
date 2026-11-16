use super::*;
#[test]
fn native_filter_switch_themes_zoom_preserves_buffer_and_never_captures() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut state = PageState::default();
            push(&mut state, false, "ordinary", "source", 1);
            push(&mut state, true, "error", "source", 2);
            let ids = state.console.iter().map(|l| l.id).collect::<Vec<_>>();
            let mut errors_only = false;
            let draw = |filter: &mut bool, events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 620.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .show(ctx, |ui| show(ui, filter, &state.console));
                    },
                )
            };
            for _ in 0..3 {
                draw(&mut errors_only, vec![]);
            }
            for label in ["Errors", "All"] {
                let (_, rect) = ctx
                    .data(|d| {
                        d.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new((
                            "console-filter",
                            label,
                        )))
                    })
                    .unwrap();
                for pressed in [true, false] {
                    let out = draw(
                        &mut errors_only,
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
                    assert!(out.platform_output.commands.is_empty());
                }
                assert_eq!(errors_only, label == "Errors");
            }
            assert_eq!(ids, state.console.iter().map(|l| l.id).collect::<Vec<_>>());
            let mut app = crate::app::files::tests::app(&ctx);
            app.draft = "Kept".into();
            app.collect_context_capture();
            assert_eq!(app.draft, "Kept");
        }
    }
}
