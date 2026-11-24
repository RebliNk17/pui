//! Narrow inspector connector/action geometry and conditional tooltip regressions.
use super::*;

#[test]
fn connectors_keep_shared_right_edge_and_never_overlap_when_narrow() {
    for dark in [false, true] {
        for zoom in [0.8, 1.4] {
            for width in [180.0, 240.0, 360.0] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                ctx.set_visuals(if dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
                ctx.set_zoom_factor(zoom);
                let mut app = crate::app::files::tests::app(&ctx);
                let connectors =
                    ["LaunchDarkly_feature_management", "slack"].map(|name| McpConnector {
                        name: name.into(),
                        error: None,
                    });
                let mut right = 0.0;
                for _ in 0..3 {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width + 16.0, 900.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                ui.set_width(width);
                                right = ui.cursor().left() + ui.available_width();
                                app.connectors_card(ui, &connectors);
                            });
                        },
                    );
                }
                let mut actions = vec![];
                for connector in &connectors {
                    let (row, name, action, elided) = ctx
                        .data(|data| {
                            data.get_temp::<(egui::Rect, egui::Rect, egui::Rect, bool)>(
                                egui::Id::new(("connector-row", connector.name.as_str())),
                            )
                        })
                        .unwrap();
                    assert!(
                        name.right() + 7.0 <= action.left(),
                        "name {name:?}, action {action:?}"
                    );
                    assert!(row.contains_rect(action));
                    assert!((action.right() - right).abs() < 1.0);
                    if width == 180.0 && connector.name.starts_with("LaunchDarkly") {
                        assert!(
                            elided,
                            "expected truncated long connector: row {row:?}, name {name:?}, width {width}"
                        );
                    }
                    if connector.name == "slack" {
                        assert!(!elided);
                    }
                    actions.push(action);
                }
                assert_eq!(actions[0].right(), actions[1].right());
            }
        }
    }
}
