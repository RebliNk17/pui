use super::*;

#[test]
fn replay_current_pi_wire_fixture() {
    let Ok(path) = std::env::var("PUI_STREAM_FIXTURE") else {
        return;
    };
    let records: Vec<Value> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mut state = State::default();
    for (i, record) in records.into_iter().enumerate() {
        state.apply(record);
        let expected = match i {
            0 | 2 => "Confirmed",
            1 => "",
            _ => "Confirmed: the running app",
        };
        assert_eq!(
            state.messages[0].text, expected,
            "wire event {i} must not duplicate text"
        );
    }
}

fn update(state: &mut State, kind: &str, index: usize, key: &str, value: Value) {
    let mut event =
        json!({"type":"message_update","assistantMessageEvent":{"type":kind,"contentIndex":index}});
    event["assistantMessageEvent"][key] = value;
    state.apply(event);
}
#[test]
fn real_rpc_deltas_do_not_append_to_a_shallow_first_token_seed() {
    let mut state = State::default();
    state.apply(json!({"type":"message_start","message":{"role":"assistant","content":[{"type":"text","text":"Confirmed"}]}}));
    // Current Pi toJsonEvent strips message + partial from message_update.
    update(&mut state, "text_start", 0, "unused", Value::Null);
    assert_eq!(state.messages[0].text, "");
    for (delta, expected) in [
        ("Confirmed", "Confirmed"),
        (": the running app", "Confirmed: the running app"),
        (" is ready", "Confirmed: the running app is ready"),
    ] {
        update(&mut state, "text_delta", 0, "delta", delta.into());
        assert_eq!(
            state.messages[0].text, expected,
            "must be correct before message_end"
        );
        assert!(matches!(&state.messages[0].blocks[..], [Block::Text(t)] if t == expected));
    }
    update(
        &mut state,
        "text_end",
        0,
        "content",
        "Confirmed: corrected block".into(),
    );
    assert_eq!(state.messages[0].text, "Confirmed: corrected block");
    state.apply(json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"Confirmed: final"}]}}));
    assert_eq!(state.messages[0].text, "Confirmed: final");
    assert!(state.stream.is_none());
}
#[test]
fn first_indexed_delta_without_start_replaces_seed_and_keeps_genuine_repetition() {
    let mut state = State::default();
    state.apply(json!({"type":"message_start","message":{"role":"assistant","content":[{"type":"text","text":"very"}]}}));
    for (delta, expected) in [
        ("very", "very"),
        (" very", "very very"),
        (" good", "very very good"),
    ] {
        update(&mut state, "text_delta", 0, "delta", delta.into());
        assert_eq!(state.messages[0].text, expected);
    }
}
#[test]
fn indexed_thinking_text_and_tool_calls_keep_block_order_and_live_tool_results() {
    let mut state = State::default();
    state.apply(json!({"type":"message_start","message":{"role":"assistant","content":[]}}));
    update(&mut state, "thinking_start", 0, "unused", Value::Null);
    update(
        &mut state,
        "thinking_delta",
        0,
        "delta",
        "Think think".into(),
    );
    update(
        &mut state,
        "thinking_end",
        0,
        "content",
        "Reasoning complete".into(),
    );
    update(&mut state, "text_delta", 1, "delta", "Before tool".into());
    state.apply(json!({"type":"message_update","assistantMessageEvent":{"type":"toolcall_start","contentIndex":2,"id":"call","toolName":"read"}}));
    update(
        &mut state,
        "toolcall_delta",
        2,
        "delta",
        "{\"path\":".into(),
    );
    update(
        &mut state,
        "toolcall_delta",
        2,
        "delta",
        "\"file.rs\"}".into(),
    );
    assert_eq!(state.messages[0].tools[0].path, "file.rs");
    update(
        &mut state,
        "toolcall_end",
        2,
        "toolCall",
        json!({"type":"toolCall","id":"call","name":"read","arguments":{"path":"file.rs"}}),
    );
    state.apply(json!({"type":"tool_execution_start","toolCallId":"call","toolName":"read","args":{"path":"file.rs"}}));
    state.apply(json!({"type":"tool_execution_end","toolCallId":"call","result":{"content":[{"type":"text","text":"file bytes"}]}}));
    update(&mut state, "text_start", 3, "unused", Value::Null);
    update(&mut state, "text_delta", 3, "delta", "After tool".into());
    assert_eq!(state.messages[0].text, "Before tool\n\nAfter tool");
    assert_eq!(state.messages[0].thinking, "Reasoning complete");
    assert!(
        matches!(&state.messages[0].blocks[..], [Block::Thinking(_), Block::Text(_), Block::Tool(t), Block::Text(_)] if t == "call")
    );
    assert!(state.messages[0].tools[0].finished);
    assert_eq!(state.messages[0].tools[0].output, "file bytes");
}
#[test]
fn snapshot_adapters_can_transition_to_indexed_deltas_without_losing_prefixes() {
    let mut state = State::default();
    state.apply(json!({"type":"message_start","message":{"role":"assistant","content":[]}}));
    state.apply(json!({"type":"message_update","message":{"role":"assistant","content":[{"type":"text","text":"Hello"}]},"assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"Hello"}}));
    update(&mut state, "text_delta", 0, "delta", " world".into());
    assert_eq!(state.messages[0].text, "Hello world");
    update(&mut state, "text_end", 0, "content", "Hello world!".into());
    assert_eq!(state.messages[0].text, "Hello world!");
}
