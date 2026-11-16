use super::*;
use crate::protocol::ContextBreakdown;

fn painted_labels(shape: &egui::Shape, labels: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(text) => labels.push((
            text.galley.text().to_owned(),
            text.galley.rect.translate(text.pos.to_vec2()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                painted_labels(shape, labels);
            }
        }
        _ => {}
    }
}

#[test]
fn live_context_renders_in_both_themes_at_narrow_widths_and_zoom() {
    for dark in [false, true] {
        for width in [240.0, 320.0] {
            for zoom in [1.0, 1.5] {
                for reported in [Some(4200.0), None] {
                    let ctx = egui::Context::default();
                    crate::theme::install(&ctx);
                    ctx.set_visuals(if dark {
                        egui::Visuals::dark()
                    } else {
                        egui::Visuals::light()
                    });
                    ctx.set_zoom_factor(zoom);
                    let state = State {
                        context_tokens: reported,
                        context_window: 600000.0,
                        compact_percent: Some(85),
                        context_breakdown: Some(ContextBreakdown {
                            system: 100.0,
                            tools: 200.0,
                            mcp_tools: 300.0,
                            user: 400.0,
                            assistant: 500.0,
                            tool_results: 600.0,
                            other: 100.0,
                        }),
                        ..Default::default()
                    };
                    let mut labels = Vec::new();
                    for _ in 0..3 {
                        let output = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    Vec2::new(width, 900.0),
                                )),
                                ..Default::default()
                            },
                            |ctx| {
                                egui::CentralPanel::default().show(ctx, |ui| {
                                    let id = ui.make_persistent_id("message-context");
                                    ui.ctx().data_mut(|d| d.insert_temp(id, true));
                                    show(ui, &state, false);
                                });
                            },
                        );
                        labels.clear();
                        for shape in output.shapes {
                            painted_labels(&shape.shape, &mut labels);
                        }
                    }
                    assert!(labels.iter().any(|(s, _)| s == "Messages"));
                    assert!(labels.iter().any(|(s, _)| s == "You"));
                    assert!(labels.iter().any(|(s, _)| s.contains("Estimated split")));
                    assert!(
                        labels
                            .iter()
                            .any(|(s, _)| s == "Auto-compacts at 85% · 510k tokens")
                    );
                    assert!(!labels.iter().any(|(s, _)| s.contains("not supplied")));
                    for (label, bounds) in &labels {
                        assert!(
                            bounds.right() <= ctx.screen_rect().right() + 1.0,
                            "{label:?} overflows {width}pt at {zoom}x: {bounds:?}"
                        );
                        assert!(
                            bounds.left() >= -1.0,
                            "{label:?} overflows left: {bounds:?}"
                        );
                    }
                }
            }
        }
    }
}
