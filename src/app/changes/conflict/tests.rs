use super::*;
#[test]
fn conflict_inspector_escape_themes_zoom_no_write_or_resolution() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut changes = Changes {
                index_writable: true,
                ..Default::default()
            };
            changes.conflict.review = Some(workspace::conflict::Review {
                root: "/fixture".into(),
                path: "untrusted 世界.txt".into(),
                sides: [
                    Some(workspace::conflict::Side {
                        mode: "100644".into(),
                        oid: "a".repeat(40),
                        text: "base\n".into(),
                    }),
                    None,
                    Some(workspace::conflict::Side {
                        mode: "100644".into(),
                        oid: "b".repeat(40),
                        text: "theirs\n".into(),
                    }),
                ],
            });
            for n in 0..4 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 620.0),
                        )),
                        events: if n == 3 {
                            vec![egui::Event::Key {
                                key: egui::Key::Escape,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers: egui::Modifiers::NONE,
                            }]
                        } else {
                            vec![]
                        },
                        ..Default::default()
                    },
                    |ctx| changes.conflict_dialog(ctx),
                );
            }
            assert!(changes.conflict.review.is_none());
            assert!(changes.conflict.pending.is_none());
        }
    }
}
