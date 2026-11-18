use super::*;
#[path = "consent_plan_ui_tests.rs"]
mod ui_tests;

fn planned_request(id: &str) -> Value {
    let mut request = request(id);
    let mut value: Value = serde_json::from_str(request["placeholder"].as_str().unwrap()).unwrap();
    value["plan"] = json!({"steps":["Inspect keyboard focus and preserve the composer draft", "Add a regression and run the existing keyboard test"],
        "checks":{"criteria":[{"id":"C1","description":"Keyboard navigation retains the draft","command":"cargo test keyboard"}]}});
    request["placeholder"] = json!(value.to_string());
    request
}

#[test]
fn concrete_plan_is_visible_before_consent_and_frozen_without_an_extra_planner() {
    let mut app = app();
    let root = std::env::temp_dir().join(format!("pui-reviewed-plan-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    app.prefs.cwd = root;
    let request = planned_request("planned");
    app.control_request(&request);
    let question = app.state.dialogs.front().unwrap()["message"]
        .as_str()
        .unwrap();
    for text in [
        "Implementation plan",
        "Inspect keyboard focus",
        "Verification",
        "cargo test keyboard",
    ] {
        assert!(question.contains(text), "{question}");
    }
    assert!(app.tasks.records.is_empty() && app.tasks.processes.is_empty());
    app.workflow_answer(
        &json!({"type":"extension_ui_response","id":"planned","value":"Use workflow"}),
    );
    let task = app.tasks.current().unwrap();
    assert_eq!(task.execution_plan.as_ref().unwrap().steps.len(), 2);
    assert_eq!(task.proposed.criteria[0].command, "cargo test keyboard");
    // Complete source inspection and its read-only preflight with fixture evidence.
    app.tasks.job = None;
    app.task_job_result(Ok(JobResult::Inspected(domain::workspace::Inspection {
        root: app.prefs.cwd.clone(),
        git: false,
        dirty: false,
        head: String::new(),
    })));
    let job = app.tasks.job.take().unwrap();
    app.task_job_result(job.rx.recv_timeout(Duration::from_secs(10)).unwrap());
    let task = app.tasks.current().unwrap();
    assert_eq!(task.phase, Phase::Preparing);
    assert_eq!(
        task.approved.as_ref().unwrap().criteria[0].command,
        "cargo test keyboard"
    );
    assert!(
        app.tasks.processes.is_empty(),
        "no redundant proposal model call"
    );
    assert!(!task.runs.iter().any(|run| run.role == Role::Proposal));
    assert!(
        app.task_prompt(Role::Builder)
            .contains("Inspect keyboard focus")
    );
    assert!(
        app.tasks.job.is_some(),
        "normal isolated preparation still runs before a builder"
    );
    app.tasks.stop();
}

#[test]
fn invalid_or_cancelled_plan_never_creates_a_workflow_or_spends() {
    for invalid in [
        json!(null),
        json!({"steps":[],"checks":{"criteria":[]}}),
        json!({"steps":["change"],"checks":{"criteria":[{"id":"C1","description":"review","command":""}]}}),
    ] {
        let mut app = app();
        let mut request = planned_request("invalid");
        let mut value: Value =
            serde_json::from_str(request["placeholder"].as_str().unwrap()).unwrap();
        value["plan"] = invalid;
        request["placeholder"] = json!(value.to_string());
        app.control_request(&request);
        assert!(
            app.state.dialogs.is_empty()
                && app.tasks.requests.is_empty()
                && app.tasks.records.is_empty()
        );
        assert!(app.tasks.job.is_none() && app.tasks.processes.is_empty());
    }
    let mut app = app();
    app.control_request(&planned_request("cancel"));
    app.workflow_answer(&json!({"type":"extension_ui_response","id":"cancel","value":"Use workflow","cancelled":true}));
    assert!(app.tasks.records.is_empty() && app.tasks.job.is_none());
}

#[test]
fn plan_mode_review_does_not_execute_or_change_mode_and_legacy_records_load() {
    let mut app = app();
    app.prefs.settings.permission = settings::PermissionMode::Plan;
    app.control_request(&planned_request("plan-mode"));
    assert_eq!(app.tasks.requests.len(), 1);
    assert!(
        app.tasks.records.is_empty() && app.tasks.processes.is_empty() && app.tasks.job.is_none()
    );
    assert!(app.active_permission() == settings::PermissionMode::Plan);
    assert_eq!(app.state.dialogs[0]["title"], "Approve plan and implement?");
    let task = Task::new("/fixture".into(), "p".into(), "m".into(), "high".into());
    let mut value = serde_json::to_value(task).unwrap();
    value.as_object_mut().unwrap().remove("execution_plan");
    let loaded: Task = serde_json::from_value(value).unwrap();
    assert!(loaded.execution_plan.is_none());
}

#[test]
fn altered_checks_cannot_replace_the_reviewed_plan_at_implementation_boundary() {
    let mut app = app();
    app.control_request(&planned_request("altered"));
    app.workflow_answer(
        &json!({"type":"extension_ui_response","id":"altered","value":"Use workflow"}),
    );
    if let Some(job) = app.tasks.job.take() {
        let _ = job.rx.recv_timeout(Duration::from_secs(10));
    }
    app.tasks.live = None;
    let task = app.tasks.current_mut().unwrap();
    task.phase = Phase::AwaitingApproval;
    task.proposed.criteria[0].command = "different command never reviewed".into();
    app.task_start();
    assert!(app.tasks.job.is_none() && app.tasks.processes.is_empty());
    assert!(
        app.tasks
            .error
            .as_deref()
            .unwrap()
            .contains("Checks changed after plan approval")
    );
}

#[test]
fn chat_plan_consent_builder_checks_review_and_truthful_handoff_compose_end_to_end() {
    fn job(app: &mut Pui) {
        let job = app.tasks.job.take().unwrap_or_else(|| {
            panic!(
                "missing job in {:?}: {:?}",
                app.tasks.current().map(|task| (task.phase, &task.note)),
                app.tasks.error
            )
        });
        app.task_job_result(job.rx.recv_timeout(Duration::from_secs(15)).unwrap());
    }
    fn initialize(app: &mut Pui) {
        let id = app
            .tasks
            .processes
            .last()
            .unwrap()
            .pending
            .as_ref()
            .unwrap()
            .id
            .clone();
        let index = app.tasks.processes.len() - 1;
        app.task_event(index, rpc::Event::Record(json!({"type":"response","command":"get_state","id":id,"success":true,
            "data":{"model":{"id":"claude-haiku-5-5","provider":"fixture-provider"},"thinkingLevel":app.tasks.current().unwrap().thinking}})));
    }
    fn assistant_done(app: &mut Pui, text: &str) {
        let index = app.tasks.processes.len() - 1;
        let id = app.tasks.processes[index]
            .pending
            .as_ref()
            .unwrap()
            .id
            .clone();
        app.task_event(index, rpc::Event::Record(json!({"type":"response","command":"prompt","id":id,"success":true,"data":{"disposition":"started"}})));
        app.task_event(index, rpc::Event::Record(json!({"type":"agent_start"})));
        app.task_event(
            index,
            rpc::Event::Record(
                json!({"type":"message_start","message":{"role":"assistant","content":[]}}),
            ),
        );
        app.task_event(index, rpc::Event::Record(json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":text}],"stopReason":"stop"}})));
        app.task_event(
            index,
            rpc::Event::Record(json!({"type":"agent_settled","aborted":false})),
        );
    }
    let mut app = app();
    let root =
        std::env::temp_dir().join(format!("pui-conversational-e2e-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("fixture.rs"), "fn retained() {}\n").unwrap();
    app.prefs.cwd = root;
    let (client, _, replies) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let mut request = planned_request("e2e");
    let mut value: Value = serde_json::from_str(request["placeholder"].as_str().unwrap()).unwrap();
    value["policy"] = json!("pinned");
    value["plan"]["checks"]["criteria"][0]["command"] = json!("printf fixture");
    request["placeholder"] = json!(value.to_string());
    app.control_request(&request);
    assert!(app.tasks.processes.is_empty());
    app.workflow_answer(&json!({"type":"extension_ui_response","id":"e2e","value":"Use workflow"}));
    job(&mut app); // source inspection
    job(&mut app); // planning preflight; exact approved plan skips extra planner
    job(&mut app); // workspace preparation
    job(&mut app); // implementation preflight
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Building);
    assert!(
        !app.tasks
            .current()
            .unwrap()
            .runs
            .iter()
            .any(|run| run.role == Role::Proposal)
    );
    initialize(&mut app);
    assistant_done(
        &mut app,
        "Inspected and retained fixture; meaningful regression ready.",
    );
    job(&mut app); // candidate fingerprint
    job(&mut app); // check preflight
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Checking);
    initialize(&mut app);
    let index = app.tasks.processes.len() - 1;
    let id = app.tasks.processes[index]
        .pending
        .as_ref()
        .unwrap()
        .id
        .clone();
    app.task_event(index, rpc::Event::Record(json!({"type":"response","command":"bash","id":id,"success":true,"data":{"exitCode":0,"output":"fixture check passed","cancelled":false}})));
    job(&mut app); // unchanged candidate after checks
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Verifying);
    initialize(&mut app);
    assistant_done(
        &mut app,
        r#"{"summary":"Independent fixture review","criteria":[{"id":"C1","status":"pass","evidence":"Inspected fixture.rs and completed check evidence"}]}"#,
    );
    job(&mut app); // unchanged candidate after review
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Ready);
    app.finish_workflow_requests();
    let reply = replies.recv_timeout(Duration::from_secs(3)).unwrap();
    let result: Value = serde_json::from_str(reply["value"].as_str().unwrap()).unwrap();
    assert_eq!(result["verified"], true);
    assert_eq!(result["outcome"], "verified_candidate");
    assert_eq!(result["current_evidence"]["checks"][0]["exit_code"], 0);
    assert_eq!(
        result["execution_plan"]["steps"][0],
        "Inspect keyboard focus and preserve the composer draft"
    );
    assert!(app.tasks.requests.is_empty());
}

#[test]
fn explicit_plan_approval_switches_only_owning_chat_to_manual_and_keeps_tool_gates() {
    let mut app = app();
    app.prefs.settings.permission = settings::PermissionMode::Plan;
    app.control_request(&planned_request("handoff"));
    let owner = app.agents.active.clone();
    assert!(app.tasks.records.is_empty());
    app.workflow_answer(&json!({"type":"extension_ui_response","id":"handoff","value":"Approve plan · switch to Manual"}));
    assert!(app.active_permission() == settings::PermissionMode::Manual);
    assert!(
        app.prefs.settings.permission == settings::PermissionMode::Plan,
        "not a global permission change"
    );
    assert!(app.agents.nodes[&owner].permission == Some(settings::PermissionMode::Manual));
    assert!(app.tasks.current().unwrap().execution_plan.is_some());
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Proposing);
    assert!(
        app.tasks.processes.is_empty(),
        "no provider/check execution before normal stages"
    );
    app.tasks.stop();
}
#[test]
fn plan_handoff_cancel_wrong_choice_owner_or_mode_drift_cannot_upgrade() {
    for scenario in ["cancel", "legacy-choice", "other-owner", "mode-drift"] {
        let mut app = app();
        app.prefs.settings.permission = settings::PermissionMode::Plan;
        app.control_request(&planned_request("handoff"));
        if scenario == "other-owner" {
            app.agents.active = "unrelated-chat".into();
        }
        if scenario == "mode-drift" {
            app.set_permission(settings::PermissionMode::Auto);
        }
        app.workflow_answer(&json!({"type":"extension_ui_response","id":"handoff","value":if scenario == "legacy-choice" {"Use workflow"} else {"Approve plan · switch to Manual"}, "cancelled":scenario=="cancel"}));
        assert!(
            app.tasks.records.is_empty()
                && app.tasks.job.is_none()
                && app.tasks.processes.is_empty(),
            "{scenario}"
        );
        assert!(
            app.active_permission() != settings::PermissionMode::Manual,
            "{scenario}"
        );
    }
}

// Match the inline caller's removal of a successfully answered dialog.
fn answer(app: &mut Pui, id: &str, mut response: Value) {
    response["type"] = json!("extension_ui_response");
    response["id"] = json!(id);
    assert!(app.workflow_answer(&response));
    app.state.dialogs.retain(|dialog| dialog["id"] != id);
}

#[test]
fn request_changes_returns_feedback_to_original_chat_call_without_approval_or_execution() {
    let mut app = app();
    app.prefs.settings.permission = settings::PermissionMode::Plan;
    let (client, _, replies) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.control_request(&planned_request("revision-original"));
    assert!(
        app.state.dialogs[0]["options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "Request changes")
    );
    answer(
        &mut app,
        "revision-original",
        json!({"value":"Request changes"}),
    );
    assert!(
        !app.state
            .dialogs
            .iter()
            .any(|d| d["id"] == "revision-original")
    );
    let revision_id = app.state.dialogs.back().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(
        revision_id, "revision-original",
        "old approval must not become revision feedback"
    );
    assert!(!app.workflow_answer(&json!({"type":"extension_ui_response","id":"revision-original","value":"Approve plan · switch to Manual"})));
    assert!(replies.try_recv().is_err());
    answer(
        &mut app,
        &revision_id,
        json!({"value":"Keep public APIs unchanged; add a narrow-window regression."}),
    );
    assert!(app.state.dialogs.is_empty());
    let reply = replies.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(
        reply["id"], "revision-original",
        "reply must settle the original waiting tool call"
    );
    let result: Value = serde_json::from_str(reply["value"].as_str().unwrap()).unwrap();
    assert_eq!(result["approved"], false);
    assert_eq!(result["revision_requested"], true);
    assert!(result["feedback"].as_str().unwrap().contains("public APIs"));
    assert!(
        result["instruction"]
            .as_str()
            .unwrap()
            .contains("Do not implement")
    );
    assert!(app.active_permission() == settings::PermissionMode::Plan);
    assert!(
        app.tasks.requests.is_empty()
            && app.tasks.records.is_empty()
            && app.tasks.job.is_none()
            && app.tasks.processes.is_empty()
    );
}
#[test]
fn revised_plan_requires_a_new_native_approval_and_no_plan_has_no_revision_action() {
    let mut app = app();
    app.prefs.settings.permission = settings::PermissionMode::Plan;
    app.control_request(&planned_request("first"));
    answer(&mut app, "first", json!({"value":"Request changes"}));
    let id = app.state.dialogs[0]["id"].as_str().unwrap().to_owned();
    answer(&mut app, &id, json!({"value":"Preserve the public API"}));
    app.control_request(&planned_request("revised"));
    assert_eq!(app.state.dialogs[0]["id"], "revised");
    assert!(app.active_permission() == settings::PermissionMode::Plan);
    assert!(app.tasks.records.is_empty() && app.tasks.processes.is_empty());
    assert!(!app.workflow_answer(&json!({"type":"extension_ui_response","id":"first","value":"Approve plan · switch to Manual"})));
    answer(&mut app, "revised", json!({"cancelled":true}));
    app.prefs.settings.permission = settings::PermissionMode::Manual;
    app.control_request(&request("without-plan"));
    assert!(
        !app.state.dialogs[0]["options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "Request changes")
    );
    answer(&mut app, "without-plan", json!({"value":"Request changes"}));
    assert!(
        app.tasks.requests.is_empty() && app.tasks.records.is_empty() && app.tasks.job.is_none()
    );
}

#[test]
fn revision_cancel_oversize_context_drift_and_original_call_abort_never_launch() {
    for scenario in [
        "cancel", "empty", "nul", "oversize", "drift", "model", "worker", "owner", "abort",
    ] {
        let mut app = app();
        app.prefs.settings.permission = settings::PermissionMode::Plan;
        let (client, _, replies) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.control_request(&planned_request("revision"));
        app.workflow_answer(
            &json!({"type":"extension_ui_response","id":"revision","value":"Request changes"}),
        );
        let id = app.state.dialogs.back().unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        if scenario == "drift" {
            app.set_permission(settings::PermissionMode::Auto);
        }
        if scenario == "model" {
            app.state.model = "changed-model".into();
        }
        if scenario == "worker" {
            app.agents.nodes.get_mut(&app.agents.active).unwrap().access = "read-only".into();
        }
        if scenario == "owner" {
            let owner = app.agents.active.clone();
            app.agents.active = "different-chat".into();
            assert!(!app.workflow_answer(
                &json!({"type":"extension_ui_response","id":id,"value":"foreign feedback"})
            ));
            assert!(replies.try_recv().is_err());
            app.agents.active = owner;
        }
        if scenario == "abort" {
            app.control_request(&json!({"type":"extension_ui_request","method":"setStatus","statusKey":"pui-workflow","statusText":json!({"closed":"revision"}).to_string()}));
            assert!(!app.workflow_answer(
                &json!({"type":"extension_ui_response","id":id,"value":"old feedback"})
            ));
            assert!(!app.state.dialogs.iter().any(|dialog| dialog["id"] == id));
        } else {
            let feedback = match scenario {
                "oversize" => "x".repeat(4001),
                "empty" => "  ".into(),
                "nul" => "bad\0feedback".into(),
                _ => "feedback".into(),
            };
            app.workflow_answer(&json!({"type":"extension_ui_response","id":id,"value":feedback,"cancelled":matches!(scenario, "cancel" | "owner")}));
            let reply = loop {
                let reply = replies.recv_timeout(Duration::from_secs(2)).unwrap();
                if reply["type"] == "extension_ui_response" {
                    break reply;
                }
            };
            let result: Value = serde_json::from_str(reply["value"].as_str().unwrap()).unwrap();
            assert_ne!(result["approved"], true);
            assert_ne!(result["revision_requested"], true);
        }
        assert!(
            app.tasks.requests.is_empty()
                && app.tasks.records.is_empty()
                && app.tasks.job.is_none()
        );
        assert!(app.active_permission() != settings::PermissionMode::Manual);
    }
}
