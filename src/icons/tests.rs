use super::*;

#[test]
fn refresh_is_a_smooth_scaled_vector_arc_in_both_themes() {
    for mode in [crate::theme::Theme::Light, crate::theme::Theme::Dark] {
        for scale in [0.75, 1.0, 1.4, 2.0] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            mode.apply(&ctx);
            let mut center = Pos2::ZERO;
            let output = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let rect = Rect::from_center_size(Pos2::new(60.0, 60.0), Vec2::splat(28.0));
                    center = rect.center();
                    draw_scaled(ui, rect, Icon::Refresh, Color32::RED, scale);
                });
            });
            let paths: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Path(path)
                        if path.stroke.color == egui::epaint::ColorMode::Solid(Color32::RED) =>
                    {
                        Some(path)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(paths.len(), 2);
            assert_eq!(paths[0].points.len(), 49);
            assert_eq!(paths[1].points.len(), 3);
            assert!(!paths[0].closed && !paths[1].closed);
            for point in &paths[0].points {
                assert!(((point.distance(center) / scale) - 6.0).abs() < 0.001);
            }
            assert!(
                paths[0]
                    .points
                    .windows(2)
                    .all(|points| points[0].distance(points[1]) / scale < 0.6)
            );
            assert!(paths[0].points.last().unwrap().distance(paths[1].points[1]) / scale < 0.02);
        }
    }
}
