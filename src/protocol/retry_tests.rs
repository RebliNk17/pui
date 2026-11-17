use super::*;

fn failure() -> Value {
    json!({"role":"assistant","content":[],"stopReason":"error","errorMessage":"Request timed out."})
}

#[test]
fn automatic_retries_have_live_status_without_repeated_errors_until_settled() {
    let mut state = State::default();
    state.apply(json!({"type":"agent_start"}));
    for attempt in 1..=3 {
        let message = failure();
        state.apply(json!({"type":"message_start","message":message}));
        state.apply(json!({"type":"message_end","message":message}));
        state.apply(json!({"type":"agent_end"}));
        state.apply(json!({"type":"auto_retry_start","attempt":attempt,"maxAttempts":3,"errorMessage":"Request timed out."}));
        assert!(state.busy);
        assert!(state.retrying);
        assert!(state.error.is_none());
        assert!(state.notice.is_none());
    }
    let success = json!({"role":"assistant","content":[{"type":"text","text":"Recovered answer"}],"stopReason":"stop"});
    state.apply(json!({"type":"message_start","message":success}));
    state.apply(json!({"type":"message_end","message":success}));
    assert!(!state.retrying, "A response started after the retry");
    state.apply(json!({"type":"auto_retry_end","success":true,"attempt":3}));
    state.apply(json!({"type":"agent_settled","aborted":false}));
    assert!(state.ready());
    assert!(!state.retrying);
    assert!(state.error.is_none());
    assert!(state.notice.is_none());
    assert_eq!(state.messages.len(), 4);
    assert_eq!(
        state.messages[0].error.as_deref(),
        Some("Request timed out.")
    );
    assert_eq!(state.messages.last().unwrap().text, "Recovered answer");
}

#[test]
fn exhausted_retries_do_not_duplicate_the_final_transcript_error() {
    let mut state = State::default();
    let message = failure();
    state.apply(json!({"type":"agent_start"}));
    state.apply(json!({"type":"message_start","message":message}));
    state.apply(json!({"type":"message_end","message":message}));
    state.apply(json!({"type":"auto_retry_end","success":false,"finalError":"Request timed out."}));
    state.apply(json!({"type":"agent_settled","aborted":false}));
    assert!(state.ready());
    assert!(state.error.is_none());
    assert!(state.notice.is_none());
    assert_eq!(
        state.messages.last().unwrap().error.as_deref(),
        Some("Request timed out.")
    );
}

#[test]
fn terminal_retry_error_without_an_assistant_message_is_still_reported() {
    let mut state = State::default();
    state.apply(
        json!({"type":"auto_retry_end","success":false,"finalError":"Provider unavailable"}),
    );
    assert_eq!(state.error.as_deref(), Some("Provider unavailable"));
    assert!(state.notice.is_none());
}

#[test]
fn cancelling_retry_is_not_a_failure_or_a_permission_to_retry_again() {
    let mut state = State::default();
    state.apply(json!({"type":"auto_retry_start","errorMessage":"Request timed out."}));
    state.apply(json!({"type":"auto_retry_end","success":false,"finalError":"Retry cancelled"}));
    state.apply(json!({"type":"agent_settled","aborted":true}));
    assert!(state.ready());
    assert!(state.error.is_none());
    assert!(state.notice.is_none());
}

#[test]
fn retry_events_do_not_clear_unrelated_errors_or_session_notices() {
    let mut state = State {
        error: Some("File unavailable".into()),
        notice: Some("Session changed".into()),
        ..Default::default()
    };
    state.apply(json!({"type":"auto_retry_start","errorMessage":"Request timed out."}));
    state.apply(json!({"type":"auto_retry_end","success":true}));
    assert_eq!(state.error.as_deref(), Some("File unavailable"));
    assert_eq!(state.notice.as_deref(), Some("Session changed"));
}

#[test]
fn retry_status_roundtrips_for_mirrors_and_clears_on_every_terminal_boundary() {
    let mut state = State::default();
    state.apply(json!({"type":"agent_start"}));
    state.apply(
        json!({"type":"auto_retry_start","attempt":2,"errorMessage":"secret provider details"}),
    );
    assert!(state.retrying);
    assert!(state.busy);
    let snapshot = serde_json::to_value(&state).unwrap();
    assert_eq!(snapshot["retrying"], true);
    assert!(!snapshot.to_string().contains("secret provider details"));
    let restored: State = serde_json::from_value(snapshot).unwrap();
    assert!(restored.retrying);
    assert!(!serde_json::from_value::<State>(json!({})).unwrap().retrying);
    for event in [
        json!({"type":"auto_retry_end","success":true}),
        json!({"type":"auto_retry_end","success":false,"finalError":"Retry cancelled"}),
        json!({"type":"auto_retry_end","success":false,"finalError":"Provider unavailable"}),
        json!({"type":"agent_settled","aborted":true}),
        json!({"type":"agent_settled","aborted":false}),
        json!({"type":"message_start","message":{"role":"assistant","content":[]}}),
        json!({"type":"compaction_start"}),
    ] {
        let mut current = state.clone();
        current.apply(event.clone());
        assert!(!current.retrying, "{event}");
    }
    state.apply(json!({"type":"response","command":"get_state","success":true,"data":{"sessionFile":"/new-chat","isStreaming":false}}));
    // A late get_state response cannot mark an observed run idle.
    assert!(state.retrying);
    state.run_observed = false;
    state.apply(json!({"type":"response","command":"get_state","success":true,"data":{"sessionFile":"/new-chat","isStreaming":false}}));
    assert!(!state.retrying);
}
