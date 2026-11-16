use super::*;
#[test]
fn literal_unicode_source_filter_preserves_order_ids_and_error_batch() {
    let mut state = PageState::default();
    push(&mut state, false, "literal [.*] 世界", "/a.js", 1);
    push(&mut state, true, "failure", "/world.js", 2);
    push(&mut state, true, "FAILURE", "/b.js", 3);
    let before = state.console.iter().map(|l| l.id).collect::<Vec<_>>();
    assert_eq!(
        state
            .console
            .iter()
            .filter(|l| matches(l, "[.*] 世界"))
            .count(),
        1
    );
    assert_eq!(
        state
            .console
            .iter()
            .filter(|l| matches(l, "/world"))
            .count(),
        1
    );
    assert_eq!(
        state
            .console
            .iter()
            .filter(|l| l.error && matches(l, "failure"))
            .count(),
        1
    );
    assert_eq!(
        state
            .console
            .iter()
            .filter(|l| matches(l, ".*世界"))
            .count(),
        0
    );
    assert_eq!(state.console.iter().filter(|l| matches(l, "")).count(), 3);
    assert_eq!(
        before,
        state.console.iter().map(|l| l.id).collect::<Vec<_>>()
    );
}
#[test]
fn native_search_typing_clear_and_escape_themes_narrow_zoom_no_capture() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut state = PageState::default();
            push(&mut state, true, "世界 failure", "/source", 1);
            let mut query = String::new();
            let draw = |query: &mut String, events| {
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
                            show(ui, "https://example.test", query, &state.console, false)
                        });
                    },
                )
            };
            for _ in 0..3 {
                draw(&mut query, vec![]);
            }
            let (id, _, enabled) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new(
                        "console-search-input",
                    ))
                })
                .unwrap();
            assert!(enabled);
            ctx.memory_mut(|m| m.request_focus(id));
            draw(&mut query, vec![]);
            let out = draw(&mut query, vec![egui::Event::Text("世界".into())]);
            assert_eq!(query, "世界");
            assert!(out.platform_output.commands.is_empty());
            let (_, rect, enabled) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new(
                        "console-search-clear",
                    ))
                })
                .unwrap();
            assert!(enabled);
            assert!(rect.right() <= 420.0);
            for pressed in [true, false] {
                draw(
                    &mut query,
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
            }
            assert!(query.is_empty());
            query = "kept".into();
            ctx.memory_mut(|m| m.request_focus(id));
            draw(&mut query, vec![]);
            draw(
                &mut query,
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    physical_key: None,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert!(query.is_empty());
            let mut app = crate::app::files::tests::app(&ctx);
            app.draft = "kept".into();
            app.collect_context_capture();
            assert_eq!(app.draft, "kept");
            assert_eq!(state.console.len(), 1);
        }
    }
}
#[test]
fn pasted_query_is_bounded_and_foreign_modal_disables_controls() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut query = String::new();
    let state = PageState::default();
    for n in 0..4 {
        let _ = ctx.run(
            egui::RawInput {
                events: if n == 3 {
                    vec![egui::Event::Paste("世".repeat(300))]
                } else {
                    vec![]
                },
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    show(ui, "page", &mut query, &state.console, false)
                });
                let id = ctx
                    .data(|d| {
                        d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new(
                            "console-search-input",
                        ))
                    })
                    .unwrap()
                    .0;
                ctx.memory_mut(|m| m.request_focus(id));
            },
        );
    }
    assert_eq!(query.chars().count(), 128);
    for _ in 0..3 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            prompt_ui::modal(ctx, egui::Id::new("foreign-search-test"), 300.0, |ui| {
                ui.label("foreign");
            });
            egui::CentralPanel::default().show(ctx, |ui| {
                show(ui, "page", &mut query, &state.console, false)
            });
        });
    }
    assert!(
        !ctx.data(
            |d| d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new("console-search-input"))
        )
        .unwrap()
        .2
    );
}
