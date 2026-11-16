use super::*;
#[test]
fn remote_creation_confirmation_cancel_themes_zoom_never_writes() {
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
            changes.push.owner = Some(changes.identity.clone());
            changes.push.create.name = "origin".into();
            changes.push.create.url = "https://example.invalid/repo".into();
            changes.push.create.confirm = true;
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
                    |ctx| changes.push_dialog(ctx),
                );
            }
            assert!(!changes.push.create.confirm);
            assert!(changes.push.create.pending.is_none());
        }
    }
}
