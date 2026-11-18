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
                        "fixture",
                        "high",
                        (running, false, false, view),
                        false,
                        &mut gallery,
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
fn action_runs_break_at_thinking_and_prose_in_both_views_and_streaming_states() {
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
                    assert!(pos("REASONING_BOUNDARY") < pos("PROGRESS_BOUNDARY"));
                    assert!(pos("PROGRESS_BOUNDARY") < pos("LAST_THOUGHT"));
                    assert!(pos("LAST_THOUGHT") < pos("FINAL_REPLY"));
                    assert!(!rows.iter().any(|row| row == "Thinking"));
                    if view == View::Normal {
                        assert_eq!(
                            rows.iter().filter(|row| row.starts_with("ran ")).count(),
                            2,
                            "{rows:?}"
                        );
                        assert!(pos("ran 2") < pos("REASONING_BOUNDARY"));
                        assert!(pos("REASONING_BOUNDARY") < pos("ran 1"));
                        assert!(pos("ran 1") < pos("PROGRESS_BOUNDARY"));
                        assert!(!rows.iter().any(|row| row.contains("RESULT_")));
                    } else {
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
fn legacy_reasoning_is_visible_before_reply_and_empty_blocks_do_not_split_actions() {
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
    let thought = rows
        .iter()
        .position(|row| row.contains("LEGACY_THOUGHT"))
        .unwrap();
    let reply = rows
        .iter()
        .position(|row| row.contains("LEGACY_REPLY"))
        .unwrap();
    let actions = rows
        .iter()
        .position(|row| row.contains("read 2 files"))
        .unwrap();
    assert!(thought < reply && reply < actions, "{rows:?}");
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
