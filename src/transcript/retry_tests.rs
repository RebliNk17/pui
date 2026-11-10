use super::*;
use crate::protocol::{State, parse_history};
use serde_json::{Value, json};

fn labels(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => out.push(text.galley.job.text.clone()),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                labels(shape, out);
            }
        }
        _ => {}
    }
}

fn render(
    messages: &[Message],
    running: bool,
    mode: crate::theme::Theme,
    width: f32,
) -> Vec<String> {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    mode.apply(&ctx);
    let mut gallery = crate::media::Gallery::default();
    let mut rendered = Vec::new();
    for _ in 0..3 {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 1200.0),
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
                        (running, false, true),
                        false,
                        &mut gallery,
                    );
                });
            },
        );
        rendered.clear();
        for shape in output.shapes {
            labels(&shape.shape, &mut rendered);
        }
    }
    rendered
}

fn attempts() -> Vec<Value> {
    let mut raw = vec![
        json!({"role":"assistant","content":[{"type":"text","text":"Working on your task."}],"stopReason":"toolUse"}),
    ];
    raw.extend((0..8).map(|_| json!({"role":"assistant","content":[],"stopReason":"error","errorMessage":"Request timed out."})));
    raw
}

#[test]
fn retries_are_hidden_while_running_and_after_history_reload_recovers() {
    for mode in [crate::theme::Theme::Light, crate::theme::Theme::Dark] {
        for width in [320.0, 900.0] {
            let mut raw = attempts();
            let messages = parse_history(&raw);
            let rendered = render(&messages, true, mode, width);
            assert!(rendered.iter().any(|text| text == "Working on your task."));
            assert!(
                !rendered
                    .iter()
                    .any(|text| text.contains("Request timed out"))
            );

            raw.push(json!({"role":"assistant","content":[{"type":"text","text":"Task completed."}],"stopReason":"stop"}));
            // Raw history remains unchanged, including every failed attempt.
            let messages = parse_history(&raw);
            assert_eq!(messages.iter().filter(|m| m.error.is_some()).count(), 8);
            let rendered = render(&messages, false, mode, width);
            assert!(rendered.iter().any(|text| text == "Task completed."));
            assert!(
                !rendered
                    .iter()
                    .any(|text| text.contains("Request timed out"))
            );
        }
    }
}

#[test]
fn exhausted_retries_show_one_final_error_instead_of_all_failed_attempts() {
    for mode in [crate::theme::Theme::Light, crate::theme::Theme::Dark] {
        for width in [320.0, 900.0] {
            let messages = parse_history(&attempts());
            let rendered = render(&messages, false, mode, width);
            assert_eq!(
                rendered
                    .iter()
                    .filter(|text| text.as_str() == "Request timed out.")
                    .count(),
                1
            );
        }
    }
}

#[test]
fn failed_attempts_with_partial_prose_and_tool_evidence_are_preserved() {
    let messages = parse_history(&[
        json!({"role":"assistant","content":[{"type":"text","text":"Partial answer retained."},{"type":"toolCall","id":"failed-check","name":"bash","arguments":{"command":"cargo test"}}],"stopReason":"error","errorMessage":"Request timed out."}),
        json!({"role":"toolResult","toolCallId":"failed-check","isError":true,"content":[{"type":"text","text":"Test failure evidence"}]}),
        json!({"role":"assistant","content":[{"type":"text","text":"Recovered and fixed."}],"stopReason":"stop"}),
    ]);
    assert!(messages[0].tools[0].failed);
    assert_eq!(messages[0].tools[0].output, "Test failure evidence");
    let rendered = render(&messages, false, crate::theme::Theme::Dark, 900.0);
    assert!(
        rendered
            .iter()
            .any(|text| text == "Partial answer retained.")
    );
    assert!(rendered.iter().any(|text| text == "Recovered and fixed."));
    assert!(
        !rendered
            .iter()
            .any(|text| text.contains("Request timed out"))
    );
}

#[test]
fn live_retry_failure_is_deferred_until_the_run_really_settles() {
    let mut state = State::default();
    state.apply(json!({"type":"agent_start"}));
    let message = json!({"role":"assistant","content":[],"stopReason":"error","errorMessage":"Request timed out."});
    state.apply(json!({"type":"message_start","message":message}));
    state.apply(json!({"type":"message_end","message":message}));
    let rendered = render(
        &state.messages,
        state.busy,
        crate::theme::Theme::Dark,
        900.0,
    );
    assert!(
        !rendered
            .iter()
            .any(|text| text.contains("Request timed out"))
    );
    state.apply(json!({"type":"agent_settled","aborted":false}));
    let rendered = render(
        &state.messages,
        state.busy,
        crate::theme::Theme::Dark,
        900.0,
    );
    assert_eq!(
        rendered
            .iter()
            .filter(|text| text.as_str() == "Request timed out.")
            .count(),
        1
    );
}
