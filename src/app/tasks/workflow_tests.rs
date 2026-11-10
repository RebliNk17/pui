use super::*;

fn labels(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    fn collect(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => out.push((
                t.galley.job.text.clone(),
                t.galley.rect.translate(t.pos.to_vec2()),
            )),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    for shape in &output.shapes {
        collect(&shape.shape, &mut out);
    }
    out
}

#[test]
fn no_workflow_renders_nothing_even_with_saved_sessions() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo_agent_tree();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.task_workflow(ui));
    });
    assert!(
        labels(&output).is_empty(),
        "no marketing or session-list empty state"
    );
    app.toggle_chat_tasks();
    assert!(!app.tasks.open);
    assert!(app.tasks.records.is_empty());
    assert!(app.tasks.processes.is_empty());
}

#[test]
fn workflow_is_bounded_and_stable_with_long_names_in_both_states_and_themes() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        for width in [240.0, 350.0] {
            for selected in [false, true] {
                let ctx = egui::Context::default();
                let mut app = Pui::new(
                    &eframe::CreationContext::_new_kittest(ctx.clone()),
                    Options {
                        demo: true,
                        theme: Some(mode),
                        ..Default::default()
                    },
                );
                app.demo_agent_tree();
                for node in app.agents.nodes.values_mut() {
                    node.name = format!(
                        "{} distinctive suffix {}",
                        "UnbrokenName".repeat(30),
                        node.id
                    );
                }
                if selected {
                    app.demo_task();
                    let t = app.tasks.current_mut().unwrap();
                    t.objective = "VeryLongObjective".repeat(40);
                    t.model = "VeryLongModel".repeat(40);
                }
                let mut layout = CardLayout {
                    hidden: Card::ALL
                        .into_iter()
                        .filter(|c| *c != Card::Workflow)
                        .collect(),
                    ..Default::default()
                };
                let mut widths = vec![];
                for _ in 0..7 {
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(900.0, 1000.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::SidePanel::left("workflow-test-panel")
                                .exact_width(width)
                                .frame(egui::Frame::new().inner_margin(8))
                                .show(ctx, |ui| {
                                    inspector::stack(
                                        ui,
                                        &mut layout,
                                        |_| String::new(),
                                        Some((Card::Workflow, "Open")),
                                        |ui, _| app.task_workflow(ui),
                                    );
                                    widths.push(ui.min_rect().width());
                                });
                        },
                    );
                    for (text, rect) in labels(&output) {
                        assert!(
                            rect.left() >= 7.0 && rect.right() <= width + 9.0,
                            "{mode:?}/{width}/{selected} {text}: {rect:?}"
                        );
                    }
                }
                assert!(
                    widths.iter().all(|w| *w <= width + 1.0),
                    "Growing card: {widths:?}"
                );
                assert!(
                    widths[2..]
                        .windows(2)
                        .all(|pair| (pair[0] - pair[1]).abs() <= 1.0)
                );
            }
        }
    }
}

#[test]
fn workflow_never_lists_current_or_other_sessions_but_keeps_progress_navigation() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo_agent_tree();
    app.demo_task();
    for node in app.agents.nodes.values_mut() {
        node.name = format!("Session list fixture {}", node.id);
    }
    let active = app.agents.active.clone();
    let render = |app: &mut Pui, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(350.0, 900.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.task_workflow(ui));
            },
        )
    };
    for _ in 0..3 {
        render(&mut app, vec![]);
    }
    let text = labels(&render(&mut app, vec![]));
    assert!(text.iter().any(|(s, _)| s == "Acceptance checks"));
    assert!(text.iter().any(|(s, _)| s == "Result & evidence"));
    for (label, _) in &text {
        assert!(!label.contains("CURRENT SESSION"));
        assert!(!label.contains("Other sessions"));
        assert!(!label.contains("Session list fixture"));
    }
    let pos = text
        .iter()
        .find(|(s, _)| s == "Result & evidence")
        .unwrap()
        .1
        .center();
    for pressed in [true, false] {
        render(
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(app.tasks.open);
    assert!(app.tasks.selected_run.is_none());
    assert_eq!(app.agents.active, active);
    assert!(app.tasks.processes.is_empty());
}

#[test]
fn task_session_detection_is_scoped_not_a_global_json_formatter() {
    let app = Pui::new(
        &eframe::CreationContext::_new_kittest(egui::Context::default()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    let mut state = State::default();
    assert_eq!(app.task_session(&state), None);
    state.session_name = "Task 581055d7 · Acceptance proposal · 1".into();
    assert_eq!(app.task_session(&state), Some(Role::Proposal));
    state.session_name = "Task 581055d7 · Fresh verifier · 1".into();
    assert_eq!(app.task_session(&state), Some(Role::Verifier));
    state.session_name = "Explain JSON acceptance criteria".into();
    assert_eq!(app.task_session(&state), None);
}

#[test]
fn transcript_formats_only_opted_in_task_responses_and_preserves_source() {
    let json = r#"{"criteria":[{"id":"C1","description":"Inspector stays bounded","command":"cargo test workflow"}]}"#;
    let messages = vec![Message {
        role: "assistant".into(),
        text: json.into(),
        ..Default::default()
    }];
    for formatted in [false, true] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut transcript = Transcript::default();
        transcript.task_output = formatted;
        let mut output = None;
        for _ in 0..3 {
            output = Some(ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1000.0, 800.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        transcript.show(
                            ui,
                            &messages,
                            "example-model",
                            "high",
                            false,
                            "fixture-task",
                        )
                    });
                },
            ));
        }
        let text = labels(&output.unwrap());
        assert_eq!(
            text.iter().any(|(s, _)| s == "Proposed acceptance checks"),
            formatted
        );
        assert_eq!(text.iter().any(|(s, _)| s == json), !formatted);
        assert_eq!(messages[0].text, json);
    }
}

#[test]
fn structured_proposal_is_readable_and_raw_json_stays_in_a_disclosure() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let json = r#"{"criteria":[{"id":"C1","description":"Inspector stays bounded","command":"cargo test workflow"}]}"#;
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| assert!(crate::task_output::show(ui, json)));
    });
    let text = labels(&output);
    for expected in [
        "Proposed acceptance checks",
        "C1",
        "Inspector stays bounded",
        "cargo test workflow",
        "Raw response",
    ] {
        assert!(
            text.iter().any(|(s, _)| s == expected),
            "Missing {expected}"
        );
    }
    assert!(!text.iter().any(|(s, _)| s == json));
    assert!(
        text.iter()
            .any(|(s, _)| s.contains("not execution results"))
    );
}
