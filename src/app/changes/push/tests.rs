use super::*;
#[test]
fn native_review_narrow_themes_displays_exact_target_and_never_starts_worker() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut changes = Changes::default();
            changes.push.snapshot = Some(workspace::push::Snapshot {
                root: "/fixture".into(),
                branch: "refs/heads/main".into(),
                head: "a".repeat(40),
                remote: "origin".into(),
                destination: "refs/heads/master".into(),
                endpoint: "git@example.invalid:team/repo".into(),
            });
            let workspace = Workspace {
                root: "/fixture".into(),
                is_git: true,
                ..Default::default()
            };
            for _ in 0..3 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 620.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .show(ctx, |ui| changes.push_review(ui, &workspace, false));
                    },
                );
                assert!(output.platform_output.commands.is_empty());
                assert!(output.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.job.text.contains("Destination: refs/heads/master")&&t.galley.job.text.contains("No push performed"))));
            }
            assert!(changes.push.pending.is_none());
        }
    }
}
