use super::*;
#[test]
fn native_branch_confirmation_escape_themes_zoom_no_ref_write() {
    for switching in [false, true] {
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
                changes.branch_create.review = Some(workspace::branches::create::Review {
                    root: "/fixture".into(),
                    source: "refs/heads/main".into(),
                    head: "a".repeat(40),
                    name: "feature/世界".into(),
                    switch: switching,
                    target: "a".repeat(40),
                    files: vec![workspace::history::files::Entry {
                        status: 'M',
                        path: "src/世界.rs".into(),
                    }],
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
                        |ctx| changes.create_branch_dialog(ctx),
                    );
                }
                assert!(changes.branch_create.review.is_none());
                assert!(changes.branch_create.applying.is_none());
            }
        }
    }
}
