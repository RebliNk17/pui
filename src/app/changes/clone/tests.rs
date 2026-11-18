use super::*;
#[test]
fn clone_confirmation_escape_themes_zoom_never_contacts_network() {
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
            changes.clone.review = Some(workspace::clone::Review {
                parent: "/fixture".into(),
                name: "dest".into(),
                url: "https://example.invalid/repo".into(),
                branch: Some("main".into()),
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
                    |ctx| changes.clone_dialog(ctx),
                );
            }
            assert!(changes.clone.review.is_none());
            assert!(changes.clone.applying.is_none());
        }
    }
}
