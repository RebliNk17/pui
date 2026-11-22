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
fn resource_rows_allow_native_tab_to_leave_in_both_directions() {
    for mode in [crate::theme::Theme::Light, crate::theme::Theme::Dark] {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        mode.apply(&ctx);
        let state = State::default();
        let detail = demo_detail();
        let mut ends = None;
        let mut frame = |events| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(360.0, 900.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let before = ui.button("Before resources").id;
                        show_loaded_resources(ui, &state, std::path::Path::new(""), Some(&detail));
                        let after = ui.button("After resources").id;
                        ends = Some((before, after));
                    });
                },
            );
            ends.unwrap()
        };
        let (before, after) = frame(vec![]);
        for (start, target, modifiers) in [
            (before, after, egui::Modifiers::NONE),
            (after, before, egui::Modifiers::SHIFT),
        ] {
            ctx.memory_mut(|memory| memory.request_focus(start));
            for _ in 0..24 {
                frame(vec![egui::Event::Key {
                    key: egui::Key::Tab,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }]);
                if ctx.memory(|memory| memory.has_focus(target)) {
                    break;
                }
            }
            assert!(
                ctx.memory(|memory| memory.has_focus(target)),
                "Resource disclosures must not trap native Tab focus"
            );
        }
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
fn grouped_usage_summary_only_shows_reported_session_totals() {
    let mut state = State::default();
    assert_eq!(session_token_summary(&state), "");
    state.tokens =
        serde_json::json!({"input":12977,"output":1526,"cacheRead":339181,"cacheWrite":0});
    assert_eq!(session_token_summary(&state), "354k · session");
    state.tokens = serde_json::json!({"total":12});
    assert_eq!(session_token_summary(&state), "12 · session");
    state.tokens = serde_json::json!({"input":100,"output":200});
    assert_eq!(
        session_token_summary(&state),
        "",
        "Do not invent missing cache figures"
    );
    state.tokens = serde_json::json!({"total":-1});
    assert_eq!(session_token_summary(&state), "");
}

#[test]
fn grouped_usage_keeps_session_and_last_response_scopes_separate_when_expanded() {
    for mode in [crate::theme::Theme::Light, crate::theme::Theme::Dark] {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        mode.apply(&ctx);
        let state = State {
            tokens: serde_json::json!({"input":12977,"output":1526,"cacheRead":339181,"cacheWrite":0}),
            context_tokens: Some(4200.0),
            context_window: 600000.0,
            ..Default::default()
        };
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_width(320.0);
                let id = ui.make_persistent_id(egui::Id::new("usage-details"));
                let mut open =
                    egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, true);
                open.set_open(true);
                open.store(ctx);
                show(ui, &state, false);
            });
        });
        let mut labels = Vec::new();
        for shape in output.shapes {
            painted_labels(&shape.shape, &mut labels);
        }
        for expected in [
            "Usage details",
            "354k · session",
            "Prompt cache · last response",
            "This session",
            "Input · uncached",
            "Cache read",
            "Cache write",
            "Total · session",
        ] {
            assert!(
                labels.iter().any(|(text, _)| text == expected),
                "Missing {expected}: {labels:?}"
            );
        }
        assert!(
            !labels
                .iter()
                .any(|(text, _)| text == "Tokens and prompt cache")
        );
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
                            ..Default::default()
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

#[test]
fn detailed_breakdown_shows_every_category_deferred_rows_and_named_lists() {
    use crate::protocol::{ContextDetail, ContextItem};
    let item = |name: &str, tokens: f64| ContextItem {
        name: name.into(),
        tokens,
    };
    for dark in [false, true] {
        for width in [240.0, 320.0] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            let state = State {
                context_tokens: Some(3000.0),
                context_window: 600000.0,
                compact_percent: Some(85),
                context_breakdown: Some(ContextBreakdown {
                    system: 100.0,
                    tools: 200.0,
                    mcp_tools: 300.0,
                    user: 400.0,
                    assistant: 500.0,
                    tool_results: 600.0,
                    skills: 300.0,
                    memory: 200.0,
                    mcp_instructions: 250.0,
                    agents: 150.0,
                    detail: ContextDetail {
                        mcp_tools: vec![item("mcp__docs__search_pages", 300.0)],
                        mcp_tools_count: 1,
                        system_tools: vec![item("read", 200.0)],
                        skills: vec![item("deploy", 300.0)],
                        memory: vec![item("~/.claude/CLAUDE.md", 200.0)],
                        agents: vec![item("agents", 150.0)],
                        deferred_mcp: 6000.0,
                        deferred_system: 900.0,
                        ..Default::default()
                    },
                    ..Default::default()
                }),
                ..Default::default()
            };
            let mut labels = Vec::new();
            for _ in 0..3 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, 1400.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            for key in [
                                "ctx-mcp-tools",
                                "ctx-system-tools",
                                "ctx-skills",
                                "ctx-memory",
                                "ctx-agents",
                            ] {
                                let id = ui.make_persistent_id(key);
                                ui.ctx().data_mut(|d| d.insert_temp(id, true));
                            }
                            show(ui, &state, false);
                        });
                    },
                );
                labels.clear();
                for shape in output.shapes {
                    painted_labels(&shape.shape, &mut labels);
                }
            }
            for expected in [
                "Messages",
                "MCP tools",
                "System tools",
                "System prompt",
                "Skills",
                "MCP server instructions",
                "Memory files",
                "Custom agents",
                "Autocompact buffer",
                "Free space",
                "MCP tools (deferred)",
                "System tools (deferred)",
                "docs · search pages",
                "read",
                "deploy",
                "~/.claude/CLAUDE.md",
                "agents",
            ] {
                assert!(
                    labels.iter().any(|(text, _)| text == expected),
                    "Missing {expected} at {width}pt: {labels:?}"
                );
            }
            // Deferred tools are outside the active context, so they have no share of the window.
            assert!(labels.iter().any(|(text, _)| text == "—"));
            for (label, bounds) in &labels {
                assert!(
                    bounds.right() <= ctx.screen_rect().right() + 1.0 && bounds.left() >= -1.0,
                    "{label:?} overflows {width}pt: {bounds:?}"
                );
            }
        }
    }
}
