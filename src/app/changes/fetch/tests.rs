use super::*;
#[test]
fn fetch_confirmation_escape_themes_zoom_no_worker_or_network() {
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
            changes.fetch.owner = Some(changes.identity.clone());
            changes.fetch.timeout = 30;
            changes.fetch.review = Some(workspace::fetch::Review {
                root: "/fixture".into(),
                remote: "origin".into(),
                branch: "main".into(),
                endpoint: "https://example.invalid/repo".into(),
                tracking: "refs/remotes/origin/main".into(),
                previous: None,
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
                    |ctx| changes.fetch_dialog(ctx),
                );
            }
            assert!(changes.fetch.review.is_none());
            assert!(changes.fetch.applying.is_none());
        }
    }
}
