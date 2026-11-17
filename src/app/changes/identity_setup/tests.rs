use super::*;
#[test]
fn local_identity_confirmation_escape_themes_zoom_never_writes() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let root = workspace::commit::identity_setup::tests::fixture();
            let mut changes = Changes {
                index_writable: true,
                ..Default::default()
            };
            let inherited = root.join(".git/inherited");
            std::fs::write(
                &inherited,
                "[user]\n name = Inherited\n email = inherited@example.invalid\n",
            )
            .unwrap();
            workspace::tests::git(
                &root,
                &["config", "include.path", inherited.to_str().unwrap()],
            );
            changes.identity_setup.review = Some(
                workspace::commit::identity_setup::inspect_override(
                    &root,
                    "Fixture",
                    "fixture@example.invalid",
                    true,
                    &AtomicBool::new(false),
                )
                .unwrap(),
            );
            let before = std::fs::read(root.join(".git/config")).unwrap();
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
                    |ctx| changes.identity_setup_dialog(ctx),
                );
            }
            assert!(changes.identity_setup.review.is_none());
            assert!(changes.identity_setup.applying.is_none());
            assert_eq!(std::fs::read(root.join(".git/config")).unwrap(), before);
        }
    }
}
