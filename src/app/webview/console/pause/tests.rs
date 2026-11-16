use super::*;
#[test]
fn frozen_ring_survives_eviction_retains_provenance_and_resumes_without_backlog() {
    let ctx = egui::Context::default();
    let mut state = PageState {
        location: "https://old.test/page".into(),
        ..Default::default()
    };
    push(&mut state, true, "original", "source.js", 7);
    let id = state.console[0].id;
    let mut paused = None;
    toggle(&mut paused, &state.console, &ctx);
    let frozen = paused.as_ref().unwrap();
    assert!(Arc::ptr_eq(&frozen[0].text, &state.console[0].text));
    state.location = "https://new.test/page".into();
    for _ in 0..MAX_LINES + 1 {
        push(&mut state, false, "new", "other.js", 1);
    }
    assert_eq!(state.console.len(), MAX_LINES);
    assert!(state.console.iter().all(|l| l.id != id));
    let frozen = paused.as_ref().unwrap();
    assert_eq!(frozen.len(), 1);
    assert_eq!(frozen[0].id, id);
    let (label, body) = frozen[0].snippet("https://new.test/page");
    assert!(label.contains("https://old.test/page"));
    assert!(body.contains("original"));
    toggle(&mut paused, &state.console, &ctx);
    assert!(paused.is_none());
    assert_eq!(state.console.len(), MAX_LINES);
}
#[test]
fn native_pointer_pause_resume_themes_zoom_never_captures() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut state = PageState::default();
            push(&mut state, true, "error", "source", 1);
            let mut paused = None;
            let draw = |paused: &mut Option<std::collections::VecDeque<ConsoleLine>>, events| {
                ctx.run(
                    egui::RawInput {
                        events,
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 620.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            if show(ui, "page", paused.is_some()) {
                                toggle(paused, &state.console, ctx);
                            }
                        });
                    },
                )
            };
            for _ in 0..3 {
                draw(&mut paused, vec![]);
            }
            for expected in [true, false] {
                let (_, rect, enabled) = ctx
                    .data(|d| {
                        d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new(
                            "console-pause-control",
                        ))
                    })
                    .unwrap();
                assert!(enabled);
                assert!(rect.right() <= 420.0);
                for pressed in [true, false] {
                    let out = draw(
                        &mut paused,
                        vec![
                            egui::Event::PointerMoved(rect.center()),
                            egui::Event::PointerButton {
                                pos: rect.center(),
                                pressed,
                                button: egui::PointerButton::Primary,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    );
                    assert!(out.platform_output.commands.is_empty());
                }
                assert_eq!(paused.is_some(), expected);
                for _ in 0..2 {
                    draw(&mut paused, vec![]);
                }
            }
            let mut app = crate::app::files::tests::app(&ctx);
            app.draft = "kept".into();
            app.collect_context_capture();
            assert_eq!(app.draft, "kept");
        }
    }
}
