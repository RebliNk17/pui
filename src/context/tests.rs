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
fn collapsed_usage_uses_reported_or_estimated_context_and_bounds_the_bar() {
    let mut state = State {
        context_tokens: Some(209000.0),
        context_window: 600000.0,
        ..Default::default()
    };
    assert_eq!(usage_summary(&state), "35%");
    assert!((usage_fraction(&state).unwrap() - 209000.0 / 600000.0).abs() < 0.0001);
    state.context_tokens = None;
    state.context_breakdown = Some(ContextBreakdown {
        user: 210000.0,
        ..Default::default()
    });
    assert_eq!(usage_summary(&state), "~35%");
    state.context_tokens = Some(0.0);
    assert_eq!(usage_fraction(&state), Some(0.0));
    state.context_tokens = Some(900000.0);
    assert_eq!(usage_fraction(&state), Some(1.0));
    assert_eq!(usage_summary(&state), "150%", "only the bar saturates");
    for window in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        state.context_window = window;
        assert_eq!(usage_fraction(&state), None);
        assert_eq!(usage_summary(&state), "Not reported");
    }
    state.context_window = 600000.0;
    state.context_tokens = Some(f64::NAN);
    assert_eq!(usage_fraction(&state), None);
    state.context_tokens = None;
    state.context_breakdown = None;
    assert_eq!(usage_summary(&state), "Not reported");
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
