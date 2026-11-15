use super::*;
use std::sync::mpsc::Sender;

fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!("pui-setup-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    root
}
fn task() -> Task {
    let mut task = Task::new(fixture(), "fixture".into(), "fixture".into(), String::new());
    task.workspace = Some(fixture());
    task.isolated = true;
    task.phase = Phase::Blocked;
    task
}
fn npm(root: &Path) {
    fs::write(
        root.join("package.json"),
        r#"{"name":"fixture","devDependencies":{"fixture-only":"1.0.0"}}"#,
    )
    .unwrap();
    fs::write(root.join("package-lock.json"), r#"{"lockfileVersion":3,"packages":{"":{"name":"fixture","devDependencies":{"fixture-only":"1.0.0"}}}}"#).unwrap();
}
fn process(task: &Task, commands: &[&str]) -> (Process, Sender<rpc::Event>, Receiver<Value>) {
    let plan = plan(task)
        .unwrap()
        .with_commands(commands.iter().map(|value| (*value).into()).collect())
        .unwrap();
    plan.validate_current(task).unwrap();
    let (client, events, commands) = rpc::Client::stub_with_records();
    (
        Process::with_client(plan, egui::Context::default(), client).unwrap(),
        events,
        commands,
    )
}
fn initialize(process: &mut Process, events: &Sender<rpc::Event>, commands: &Receiver<Value>) {
    let request = commands.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(request["type"], "get_state");
    events.send(rpc::Event::Record(json!({"type":"response","command":"get_state","id":request["id"],"success":true,"data":{}}))).unwrap();
    process.poll();
    settle(process);
}
fn settle(process: &mut Process) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while process.inspecting.is_some() && Instant::now() < deadline {
        process.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        process.inspecting.is_none(),
        "bounded temporary-fixture inspection"
    );
}
fn response(process: &mut Process, events: &Sender<rpc::Event>, id: &Value, data: Value) {
    events
        .send(rpc::Event::Record(
            json!({"type":"response","command":"bash","id":id,"success":true,"data":data}),
        ))
        .unwrap();
    process.poll();
    settle(process);
}

#[test]
fn plan_is_read_only_supported_frozen_npm_and_before_builder_candidate_is_optional() {
    let task = task();
    npm(task.workspace.as_ref().unwrap());
    let before = serde_json::to_value(&task).unwrap();
    let plan = plan(&task).unwrap();
    assert_eq!(plan.commands, ["npm ci --ignore-scripts"]);
    assert_eq!(plan.binding.candidate_fingerprint, None);
    assert!(plan.explanation.contains("Lifecycle scripts are disabled"));
    assert!(plan.explanation.contains("network"));
    assert!(
        !task
            .workspace
            .as_ref()
            .unwrap()
            .join("node_modules")
            .exists()
    );
    assert_eq!(before, serde_json::to_value(task).unwrap());
    for risk in [
        "network",
        "lifecycle",
        "without an OS sandbox",
        "credential/environment",
        "shared package caches",
    ] {
        assert!(RISKS.contains(risk));
    }
}
#[test]
fn no_lock_mismatched_or_unsupported_manifest_has_no_guessed_install_command() {
    let task = task();
    let root = task.workspace.as_ref().unwrap();
    assert!(plan(&task).unwrap().commands.is_empty());
    npm(root);
    fs::write(
        root.join("package.json"),
        r#"{"devDependencies":{"another":"2.0.0"}}"#,
    )
    .unwrap();
    assert!(plan(&task).unwrap().commands.is_empty());
    fs::write(root.join("package.json"), "invalid").unwrap();
    let plan = plan(&task).unwrap();
    assert!(plan.commands.is_empty());
    assert!(plan.explanation.contains("unknown"));
    assert!(
        plan.with_commands(vec!["user-defined exact setup".into()])
            .is_ok()
    );
}
#[test]
fn typed_commands_reject_disabled_empty_oversized_and_nul_inputs() {
    for commands in [
        vec![],
        vec![" ".into()],
        vec!["a\0b".into()],
        vec!["x".repeat(16001)],
        vec!["x".into(); 9],
    ] {
        assert!(validate_commands(&commands).is_err());
    }
    assert!(validate_commands(&["npm ci".into()]).is_ok());
}
#[test]
fn active_nonisolated_and_original_source_setup_are_disabled() {
    let mut task = task();
    task.phase = Phase::Building;
    assert!(plan(&task).is_err());
    task.phase = Phase::Blocked;
    task.isolated = false;
    assert!(plan(&task).is_err());
    task.isolated = true;
    task.workspace = Some(task.source.clone());
    assert!(plan(&task).is_err());
}
#[test]
fn task_identity_commands_manifest_and_source_are_bound_before_execution() {
    let mut task = task();
    npm(task.workspace.as_ref().unwrap());
    let original = plan(&task).unwrap();
    original.validate_current(&task).unwrap();
    task.id = "foreign-owner-task".into();
    assert!(original.validate_current(&task).is_err());
    task.id = original.binding.task.clone();
    task.objective = "other objective".into();
    assert!(original.validate_current(&task).is_err());
    task.objective.clear();
    fs::write(task.source.join("changed.rs"), "source change").unwrap();
    assert!(original.validate_current(&task).is_err());
}
#[cfg(unix)]
#[test]
fn symlinked_manifest_or_lockfile_never_proposes_or_follows_a_dependency_link() {
    use std::os::unix::fs::symlink;
    let task = task();
    let root = task.workspace.as_ref().unwrap();
    symlink(
        task.source.join("private-credentials"),
        root.join("package.json"),
    )
    .unwrap();
    assert!(plan(&task).is_err());
    assert!(!root.join("node_modules").exists());
}
#[test]
fn no_bash_without_matching_durable_ack_and_initialization_never_prompts_a_model() {
    let task = task();
    let (mut process, events, commands) = process(&task, &["npm ci --ignore-scripts"]);
    assert_eq!(process.record.results.len(), 1);
    assert!(process.record.results[0].finished.is_none());
    assert!(process.needs_persist());
    initialize(&mut process, &events, &commands);
    assert!(
        commands.try_recv().is_err(),
        "initialization is not command consent"
    );
    let mut foreign = process.record.clone();
    foreign.binding.task = "foreign".into();
    assert!(process.persisted(&foreign).is_err());
    assert!(commands.try_recv().is_err());
    process.persisted(&process.record.clone()).unwrap();
    settle(&mut process);
    let bash = commands.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(bash["type"], "bash");
    assert_eq!(bash["command"], "npm ci --ignore-scripts");
    assert_eq!(bash["excludeFromContext"], true);
    process.persisted(&process.record.clone()).unwrap();
    assert!(
        commands.try_recv().is_err(),
        "an ack cannot replay a command"
    );
}
#[test]
fn exact_permission_dialog_is_not_auto_answered_and_cancel_is_preserved() {
    let task = task();
    let (mut process, events, commands) = process(&task, &["npm ci"]);
    initialize(&mut process, &events, &commands);
    process.persisted(&process.record.clone()).unwrap();
    settle(&mut process);
    let bash = commands.recv_timeout(Duration::from_secs(2)).unwrap();
    events.send(rpc::Event::Record(json!({"type":"extension_ui_request","method":"select","id":"permission","title":"Allow bash? npm ci","options":["Allow once","Deny"]}))).unwrap();
    process.poll();
    assert_eq!(process.state.dialogs.len(), 1);
    assert!(commands.try_recv().is_err());
    assert!(
        process
            .answer_dialog("foreign", Some(json!("Allow once")))
            .is_err()
    );
    process.answer_dialog("permission", None).unwrap();
    let answer = commands.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(answer["type"], "extension_ui_response");
    assert_eq!(answer["cancelled"], true);
    response(
        &mut process,
        &events,
        &bash["id"],
        json!({"exitCode":1,"output":"Action not approved.","cancelled":false}),
    );
    assert_eq!(process.record.status, SetupStatus::Cancelled);
    assert!(process.record.results[0].cancelled);
    assert!(process.record.results[0].output.contains("not approved"));
    assert!(commands.try_recv().is_err());
}
#[test]
fn confirm_permission_uses_confirmed_field_and_cancel_never_becomes_approval() {
    for (value, expected) in [
        (Some(json!(true)), Some(true)),
        (Some(json!(false)), Some(false)),
        (None, None),
    ] {
        let task = task();
        let (mut process, events, commands) = process(&task, &["fixture command"]);
        initialize(&mut process, &events, &commands);
        process.persisted(&process.record.clone()).unwrap();
        settle(&mut process);
        commands.recv_timeout(Duration::from_secs(2)).unwrap();
        events.send(rpc::Event::Record(json!({"type":"extension_ui_request","method":"confirm","id":"permission","title":"Allow bash?","message":"fixture"}))).unwrap();
        process.poll();
        process.answer_dialog("permission", value).unwrap();
        let response = commands.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(response["type"], "extension_ui_response");
        assert!(response.get("value").is_none());
        if let Some(expected) = expected {
            assert_eq!(response["confirmed"], expected);
            assert!(response.get("cancelled").is_none());
        } else {
            assert_eq!(response["cancelled"], true);
            assert!(process.record.results[0].cancelled);
        }
    }
}
#[test]
fn success_failure_unknown_and_cancel_never_replay_or_resume_a_workflow() {
    for (data, expected) in [
        (
            json!({"exitCode":0,"output":"installed fixture only","cancelled":false}),
            SetupStatus::Succeeded,
        ),
        (
            json!({"exitCode":1,"output":"generic failure","cancelled":false}),
            SetupStatus::Failed,
        ),
        (
            json!({"output":"unknown","cancelled":false}),
            SetupStatus::Unknown,
        ),
        (
            json!({"exitCode":0,"output":"partial","cancelled":true}),
            SetupStatus::Cancelled,
        ),
    ] {
        let task = task();
        let (mut process, events, commands) = process(&task, &["fixture command"]);
        initialize(&mut process, &events, &commands);
        process.persisted(&process.record.clone()).unwrap();
        settle(&mut process);
        let bash = commands.recv_timeout(Duration::from_secs(2)).unwrap();
        response(&mut process, &events, &bash["id"], data);
        assert_eq!(process.record.status, expected);
        assert!(process.record.results[0].finished.is_some());
        assert!(process.needs_persist());
        assert!(commands.try_recv().is_err());
        process.poll();
        assert!(commands.try_recv().is_err());
    }
}
#[test]
fn every_subsequent_command_requires_its_own_prior_durable_started_record() {
    let task = task();
    let (mut process, events, commands) =
        process(&task, &["first exact command", "second exact command"]);
    initialize(&mut process, &events, &commands);
    process.persisted(&process.record.clone()).unwrap();
    settle(&mut process);
    let first = commands.recv_timeout(Duration::from_secs(2)).unwrap();
    response(
        &mut process,
        &events,
        &first["id"],
        json!({"exitCode":0,"output":"ok"}),
    );
    assert_eq!(process.record.status, SetupStatus::Running);
    assert_eq!(process.record.results.len(), 2);
    assert!(process.record.results[1].finished.is_none());
    assert!(commands.try_recv().is_err());
    process.persisted(&process.record.clone()).unwrap();
    settle(&mut process);
    let second = commands.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(second["command"], "second exact command");
}
#[test]
fn tracked_setup_change_stales_candidate_and_stops_remaining_commands() {
    let task = task();
    let (mut process, events, commands) = process(&task, &["fixture mutation", "must not run"]);
    initialize(&mut process, &events, &commands);
    process.persisted(&process.record.clone()).unwrap();
    settle(&mut process);
    let bash = commands.recv_timeout(Duration::from_secs(2)).unwrap();
    fs::write(
        task.workspace.as_ref().unwrap().join("candidate.rs"),
        "changed source",
    )
    .unwrap();
    response(
        &mut process,
        &events,
        &bash["id"],
        json!({"exitCode":0,"output":"ok"}),
    );
    assert!(process.record.source_changed);
    assert_eq!(process.record.status, SetupStatus::Failed);
    assert_eq!(process.record.results.len(), 1);
    assert!(commands.try_recv().is_err());
}
#[test]
fn ignored_dependencies_do_not_claim_health_or_modify_captured_source() {
    let task = task();
    let root = task.workspace.as_ref().unwrap();
    let (mut process, events, commands) = process(&task, &["fixture install"]);
    initialize(&mut process, &events, &commands);
    process.persisted(&process.record.clone()).unwrap();
    settle(&mut process);
    let bash = commands.recv_timeout(Duration::from_secs(2)).unwrap();
    fs::create_dir(root.join("node_modules")).unwrap();
    fs::write(root.join("node_modules/fixture.txt"), "fixture").unwrap();
    response(
        &mut process,
        &events,
        &bash["id"],
        json!({"exitCode":0,"output":"ok"}),
    );
    assert_eq!(process.record.status, SetupStatus::Succeeded);
    assert!(!process.record.source_changed);
    assert!(process.record.note.contains("health is not established"));
    assert!(
        process
            .record
            .note
            .contains("No workflow stage was resumed")
    );
}
#[test]
fn stale_source_during_durable_save_prevents_any_bash_submission() {
    let task = task();
    let (mut process, events, commands) = process(&task, &["must not run"]);
    initialize(&mut process, &events, &commands);
    fs::write(task.source.join("changed.rs"), "changed after confirmation").unwrap();
    process.persisted(&process.record.clone()).unwrap();
    settle(&mut process);
    assert!(process.finished());
    assert!(process.record.source_changed);
    assert!(process.record.results[0].exit_code.is_none());
    assert!(commands.try_recv().is_err());
}
#[test]
fn result_matching_output_bounds_and_transport_errors_remain_honest() {
    let task = task();
    let (mut process, events, commands) = process(&task, &["fixture command"]);
    initialize(&mut process, &events, &commands);
    process.persisted(&process.record.clone()).unwrap();
    settle(&mut process);
    let bash = commands.recv_timeout(Duration::from_secs(2)).unwrap();
    events.send(rpc::Event::Record(json!({"type":"response","command":"bash","id":"foreign","success":true,"data":{"exitCode":0}}))).unwrap();
    process.poll();
    assert!(process.record.results[0].finished.is_none());
    response(
        &mut process,
        &events,
        &bash["id"],
        json!({"exitCode":1,"output":"界".repeat(25000)}),
    );
    assert_eq!(process.record.status, SetupStatus::Failed);
    assert!(process.record.results[0].truncated);
    assert_eq!(process.record.results[0].output.chars().count(), 24000);
    assert!(commands.try_recv().is_err());
}
#[test]
fn stop_and_transport_loss_preserve_unknown_effects_and_staged_evidence() {
    let task = task();
    let (mut process, _events, commands) = process(&task, &["fixture install"]);
    commands.recv_timeout(Duration::from_secs(2)).unwrap();
    process.cancel();
    assert_eq!(process.record.status, SetupStatus::Cancelled);
    assert!(process.record.results[0].cancelled);
    assert!(process.record.results[0].exit_code.is_none());
    let abort = commands.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(abort["type"], "abort_bash");
    assert!(
        process
            .answer_dialog("permission", Some(json!("Allow once")))
            .is_err()
    );
    let (mut process, events, commands) = self::process(&task, &["fixture install"]);
    initialize(&mut process, &events, &commands);
    process.persisted(&process.record.clone()).unwrap();
    settle(&mut process);
    commands.recv_timeout(Duration::from_secs(2)).unwrap();
    events.send(rpc::Event::Exited).unwrap();
    process.poll();
    assert_eq!(process.record.status, SetupStatus::Unknown);
    assert_eq!(process.record.results.len(), 1);
    assert!(process.record.results[0].error.is_some());
    assert!(commands.try_recv().is_err());
}
