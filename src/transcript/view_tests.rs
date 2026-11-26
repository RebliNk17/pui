use super::*;
use crate::protocol::Tool;

fn labels(shape: &egui::Shape, rows: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => rows.push(text.galley.job.text.clone()),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                labels(shape, rows);
            }
        }
        _ => {}
    }
}

fn draw(
    ctx: &egui::Context,
    messages: &[Message],
    view: View,
    running: bool,
    width: f32,
) -> Vec<String> {
    let mut rows = Vec::new();
    let mut gallery = crate::media::Gallery::default();
    for _ in 0..3 {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 2400.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    assistant(
                        ui,
                        messages,
                        0,
                        "fixture",
                        "high",
                        (running, false, false, view),
                        false,
                        &mut gallery,
                        None,
                        &std::sync::Arc::new(std::sync::Mutex::new(None)),
                    );
                });
            },
        );
        rows.clear();
        for shape in output.shapes {
            labels(&shape.shape, &mut rows);
        }
    }
    rows
}

#[test]
fn normal_groups_actions_across_hidden_thinking_while_verbose_preserves_chronology() {
    for theme in [crate::theme::Theme::Light, crate::theme::Theme::Dark] {
        for width in [320.0, 800.0] {
            for running in [false, true] {
                let ctx = egui::Context::default();
                crate::theme::install(&ctx);
                theme.apply(&ctx);
                let tools = (0..3)
                    .map(|i| Tool {
                        id: format!("tool-{i}"),
                        name: "bash".into(),
                        command: format!("echo ACTION_{i}"),
                        output: format!("RESULT_{i}"),
                        finished: true,
                        ..Default::default()
                    })
                    .collect();
                let messages = vec![
                    Message {
                        role: "assistant".into(),
                        tools,
                        blocks: vec![
                            Block::Tool("tool-0".into()),
                            Block::Tool("tool-1".into()),
                            Block::Thinking("REASONING_BOUNDARY".into()),
                            Block::Tool("tool-2".into()),
                            Block::Text("PROGRESS_BOUNDARY".into()),
                        ],
                        ..Default::default()
                    },
                    Message {
                        role: "assistant".into(),
                        blocks: vec![
                            Block::Thinking("LAST_THOUGHT".into()),
                            Block::Text("FINAL_REPLY".into()),
                        ],
                        ..Default::default()
                    },
                ];
                // Switch twice in the same context: verbose must not overwrite normal disclosures.
                for view in [View::Normal, View::Verbose, View::Normal] {
                    let rows = draw(&ctx, &messages, view, running, width);
                    let pos = |needle: &str| {
                        rows.iter()
                            .position(|row| row.contains(needle))
                            .unwrap_or_else(|| panic!("missing {needle}: {rows:?}"))
                    };
                    assert!(pos("PROGRESS_BOUNDARY") < pos("FINAL_REPLY"));
                    if view == View::Normal {
                        for hidden in ["Thinking", "REASONING_BOUNDARY", "LAST_THOUGHT"] {
                            assert!(!rows.iter().any(|row| row.contains(hidden)), "{rows:?}");
                        }
                        assert_eq!(
                            rows.iter().filter(|row| row.starts_with("ran ")).count(),
                            1,
                            "{rows:?}"
                        );
                        assert!(pos("ran 3 commands") < pos("PROGRESS_BOUNDARY"));
                        assert!(!rows.iter().any(|row| row.contains("RESULT_")));
                    } else {
                        assert!(pos("REASONING_BOUNDARY") < pos("PROGRESS_BOUNDARY"));
                        assert!(pos("PROGRESS_BOUNDARY") < pos("LAST_THOUGHT"));
                        assert!(pos("LAST_THOUGHT") < pos("FINAL_REPLY"));
                        assert_eq!(rows.iter().filter(|row| *row == "Thinking").count(), 2);
                        assert!(!rows.iter().any(|row| row.starts_with("ran ")));
                        assert!(pos("RESULT_0") < pos("RESULT_1"));
                        assert!(pos("RESULT_1") < pos("REASONING_BOUNDARY"));
                        assert!(pos("REASONING_BOUNDARY") < pos("RESULT_2"));
                        assert!(pos("RESULT_2") < pos("PROGRESS_BOUNDARY"));
                    }
                }
            }
        }
    }
}

#[test]
fn normal_hides_legacy_reasoning_without_hiding_reply_or_splitting_actions() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let messages = vec![
        Message {
            role: "assistant".into(),
            thinking: "LEGACY_THOUGHT".into(),
            text: "LEGACY_REPLY".into(),
            tools: vec![Tool {
                id: "a".into(),
                name: "read".into(),
                finished: true,
                ..Default::default()
            }],
            ..Default::default()
        },
        Message {
            role: "assistant".into(),
            blocks: vec![
                Block::Thinking(String::new()),
                Block::Text(String::new()),
                Block::Tool("b".into()),
            ],
            tools: vec![Tool {
                id: "b".into(),
                name: "read".into(),
                finished: true,
                ..Default::default()
            }],
            ..Default::default()
        },
    ];
    let rows = draw(&ctx, &messages, View::Normal, false, 800.0);
    assert!(!rows.iter().any(|row| row.contains("LEGACY_THOUGHT")));
    assert!(!rows.iter().any(|row| row == "Thinking"));
    let reply = rows
        .iter()
        .position(|row| row.contains("LEGACY_REPLY"))
        .unwrap();
    let actions = rows
        .iter()
        .position(|row| row.contains("read 2 files"))
        .unwrap();
    assert!(reply < actions, "{rows:?}");
    let verbose = draw(&ctx, &messages, View::Verbose, false, 800.0);
    let thought = verbose
        .iter()
        .position(|row| row.contains("LEGACY_THOUGHT"))
        .unwrap();
    let reply = verbose
        .iter()
        .position(|row| row.contains("LEGACY_REPLY"))
        .unwrap();
    assert!(thought < reply, "{verbose:?}");
}

#[test]
fn normal_summarizes_recorded_actions_not_reasoning_and_preserves_failures_and_replies() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let tools = vec![
        Tool {
            id: "read".into(),
            name: "read".into(),
            finished: true,
            ..Default::default()
        },
        Tool {
            id: "edit".into(),
            name: "edit".into(),
            finished: true,
            ..Default::default()
        },
        Tool {
            id: "test".into(),
            name: "bash".into(),
            finished: true,
            failed: true,
            ..Default::default()
        },
    ];
    let source = "PRIVATE_REASONING ".repeat(2000);
    let messages = vec![Message {
        role: "assistant".into(),
        thinking: source.clone(),
        tools,
        blocks: vec![
            Block::Tool("read".into()),
            Block::Thinking(source.clone()),
            Block::Tool("edit".into()),
            Block::Thinking(source.clone()),
            Block::Tool("test".into()),
            Block::Text("FINAL_ANSWER_WITH_BLOCKER".into()),
        ],
        ..Default::default()
    }];
    for width in [280.0, 800.0] {
        let rows = draw(&ctx, &messages, View::Normal, false, width);
        assert!(!rows.iter().any(|row| row.contains("PRIVATE_REASONING")));
        assert_eq!(
            rows.iter()
                .filter(|row| row.starts_with("read 1 file"))
                .count(),
            1
        );
        assert!(
            rows.iter()
                .any(|row| row.contains("edited 1 file") && row.contains("ran 1 command"))
        );
        assert!(rows.iter().any(|row| row == "1 failed"));
        assert!(rows.iter().any(|row| row == "FINAL_ANSWER_WITH_BLOCKER"));
    }
    assert_eq!(messages[0].thinking, source);
    assert!(matches!(&messages[0].blocks[1], Block::Thinking(text) if text == &source));
}

#[test]
fn thinking_only_stream_stays_quiet_in_normal_and_short_progress_is_not_hidden() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    for length in [1, 100, 2000] {
        let messages = vec![Message {
            role: "assistant".into(),
            blocks: vec![Block::Thinking("HIDDEN_THOUGHT ".repeat(length))],
            ..Default::default()
        }];
        let rows = draw(&ctx, &messages, View::Normal, true, 320.0);
        assert!(
            !rows.iter().any(|row| row.contains("HIDDEN_THOUGHT")
                || row == "Thinking"
                || row == "Show more")
        );
    }
    let messages = vec![Message {
        role: "assistant".into(),
        blocks: vec![Block::Text(
            "Let me think. This is a progress update.".into(),
        )],
        ..Default::default()
    }];
    let rows = draw(&ctx, &messages, View::Normal, true, 320.0);
    assert!(
        rows.iter()
            .any(|row| row.contains("This is a progress update."))
    );
}

#[test]
fn verbose_expands_nested_tools_and_read_results_without_changing_normal_state() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let messages = vec![Message {
        role: "assistant".into(),
        tools: vec![Tool {
            id: "script".into(),
            name: "codemode".into(),
            code: "SCRIPT_SOURCE".into(),
            output: "SCRIPT_RESULT".into(),
            finished: true,
            children: vec![Tool {
                id: "child".into(),
                name: "read".into(),
                args: "READ_ARGUMENTS".into(),
                output: "READ_CONTENTS".into(),
                finished: true,
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    }];
    let rows = draw(&ctx, &messages, View::Verbose, false, 800.0);
    for expected in [
        "SCRIPT_SOURCE",
        "SCRIPT_RESULT",
        "READ_ARGUMENTS",
        "READ_CONTENTS",
    ] {
        assert!(rows.iter().any(|row| row.contains(expected)), "{rows:?}");
    }
    let rows = draw(&ctx, &messages, View::Normal, false, 800.0);
    assert!(!rows.iter().any(|row| row.contains("READ_CONTENTS")));
}

#[test]
fn collapsed_group_summary_distinguishes_blocked_calls_from_ordinary_errors() {
    use crate::protocol::DenialKind;

    for theme in [crate::theme::Theme::Light, crate::theme::Theme::Dark] {
        for width in [320.0, 800.0] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            theme.apply(&ctx);

            // Case 1: reads + successful command + Auto-blocked call -> no red "failed" count.
            let auto_only = vec![Message {
                role: "assistant".into(),
                tools: vec![
                    Tool {
                        id: "r1".into(),
                        name: "read".into(),
                        path: "src/a.rs".into(),
                        finished: true,
                        ..Default::default()
                    },
                    Tool {
                        id: "r2".into(),
                        name: "read".into(),
                        path: "src/b.rs".into(),
                        finished: true,
                        ..Default::default()
                    },
                    Tool {
                        id: "c1".into(),
                        name: "bash".into(),
                        command: "cargo check".into(),
                        finished: true,
                        ..Default::default()
                    },
                    Tool {
                        id: "b1".into(),
                        name: "bash".into(),
                        command: "rm -rf /etc/hosts".into(),
                        finished: true,
                        failed: true,
                        denial_kind: Some(DenialKind::AutoMode),
                        denial_reason: Some("Built-in safety rule: destructive action.".into()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }];
            let rows = draw(&ctx, &auto_only, View::Normal, false, width);
            assert!(
                rows.iter()
                    .any(|r| r.contains("read 2 files · ran 1 command · 1 blocked by Auto")),
                "missing Auto blocked group summary (theme={theme:?}, width={width}): {rows:?}"
            );
            assert!(
                !rows.iter().any(|r| r.contains("failed")),
                "Auto blocked group should not show ordinary failed label: {rows:?}"
            );

            // Case 2: sandbox-blocked call alongside an ordinary command failure.
            let mixed = vec![Message {
                role: "assistant".into(),
                tools: vec![
                    Tool {
                        id: "r3".into(),
                        name: "read".into(),
                        path: "src/lib.rs".into(),
                        finished: true,
                        ..Default::default()
                    },
                    Tool {
                        id: "s1".into(),
                        name: "bash".into(),
                        command: "cat ~/.ssh/id_rsa".into(),
                        finished: true,
                        failed: true,
                        denial_kind: Some(DenialKind::Sandbox),
                        denial_reason: Some(
                            "sandbox-exec: /bin/bash: Operation not permitted".into(),
                        ),
                        ..Default::default()
                    },
                    Tool {
                        id: "f1".into(),
                        name: "bash".into(),
                        command: "cargo test".into(),
                        output: "test result: FAILED.".into(),
                        finished: true,
                        failed: true,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }];
            let rows = draw(&ctx, &mixed, View::Normal, false, width);
            assert!(
                rows.iter()
                    .any(|r| r.contains("read 1 file · ran 1 command · 1 blocked by sandbox")),
                "missing sandbox blocked group summary (theme={theme:?}, width={width}): {rows:?}"
            );
            assert!(
                rows.iter().any(|r| r == "1 failed"),
                "ordinary command failure should still show '1 failed' alongside blocked count: {rows:?}"
            );

            // Case 3: classifier unavailable + policy mode blocked, zero ordinary failures.
            let unavail_and_mode = vec![Message {
                role: "assistant".into(),
                tools: vec![
                    Tool {
                        id: "u1".into(),
                        name: "bash".into(),
                        command: "curl https://example.com".into(),
                        finished: true,
                        failed: true,
                        denial_kind: Some(DenialKind::ClassifierUnavailable),
                        denial_reason: Some(
                            "Classifier check unavailable — google/flash: timeout.".into(),
                        ),
                        ..Default::default()
                    },
                    Tool {
                        id: "p1".into(),
                        name: "edit".into(),
                        path: "src/main.rs".into(),
                        finished: true,
                        failed: true,
                        denial_kind: Some(DenialKind::PolicyRule),
                        denial_reason: Some("Plan mode is read-only.".into()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }];
            let rows = draw(&ctx, &unavail_and_mode, View::Normal, false, width);
            assert!(
                rows.iter()
                    .any(|r| r.contains("1 classifier unavailable · 1 blocked by mode")),
                "missing classifier/mode group summary (theme={theme:?}, width={width}): {rows:?}"
            );
            assert!(
                !rows.iter().any(|r| r.contains("failed")),
                "denied-only group should not show ordinary failed label: {rows:?}"
            );
        }
    }
}
