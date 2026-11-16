use super::*;

#[test]
fn explicit_pinned_policy_is_described_before_consent_and_bound_after_approval() {
    let mut app = app();
    app.state.model = "gpt-6.1-sol".into();
    app.state.models[0].id = "gpt-6.1-sol".into();
    let mut request = request("pinned");
    let mut value: Value = serde_json::from_str(request["placeholder"].as_str().unwrap()).unwrap();
    value["policy"] = json!("pinned");
    value["spend_limit"] = json!(1.25);
    request["placeholder"] = json!(value.to_string());
    app.control_request(&request);
    let message = app.state.dialogs[0]["message"].as_str().unwrap();
    assert!(message.contains("Pinned routing"));
    assert!(message.contains("$1.25"));
    assert!(message.contains("not a hard billing cap"));
    assert!(app.tasks.records.is_empty());
    app.workflow_answer(
        &json!({"type":"extension_ui_response","id":"pinned","value":"Use workflow"}),
    );
    let task = app.tasks.current().unwrap();
    assert!(!task.dynamic_routing);
    assert_eq!(task.spend_limit, Some(1.25));
    app.tasks.stop();
}
#[test]
fn policy_question_cancellation_and_effort_or_pricing_drift_create_nothing() {
    for scenario in ["cancel", "effort", "price"] {
        let mut app = app();
        app.control_request(&request("ask"));
        if scenario == "effort" {
            app.state.thinking = "off".into();
        }
        if scenario == "price" {
            app.state.models[0].input_cost += 1.0;
        }
        app.workflow_answer(&json!({"type":"extension_ui_response","id":"ask","value":"Use workflow","cancelled":scenario=="cancel"}));
        assert!(app.tasks.records.is_empty(), "{scenario}");
        assert!(app.tasks.processes.is_empty());
        assert!(app.tasks.job.is_none());
    }
}
#[test]
fn invalid_native_policy_or_spend_never_asks_or_spends() {
    for fields in [
        json!({"policy":"fallback"}),
        json!({"policy":"pinned","spend_limit":0}),
        json!({"policy":"pinned","spend_limit":null}),
    ] {
        let mut app = app();
        let mut request = request("ask");
        let mut value: Value =
            serde_json::from_str(request["placeholder"].as_str().unwrap()).unwrap();
        for (key, field) in fields.as_object().unwrap() {
            value[key] = field.clone();
        }
        request["placeholder"] = json!(value.to_string());
        app.control_request(&request);
        assert!(app.state.dialogs.is_empty());
        assert!(app.tasks.requests.is_empty());
        assert!(app.tasks.records.is_empty());
    }
}
