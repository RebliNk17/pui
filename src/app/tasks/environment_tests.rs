use super::*;
#[path = "environment_runtime_tests.rs"]
mod runtime_tests;

fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!("pui-env-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    root
}
fn contract(command: &str) -> Contract {
    Contract {
        criteria: vec![domain::Criterion {
            id: "C1".into(),
            description: "Fixture check".into(),
            command: command.into(),
        }],
    }
}
fn app(root: &Path, phase: Phase) -> Pui {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(egui::Context::default()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    let mut task = Task::new(
        root.into(),
        "fixture-provider".into(),
        "claude-haiku-5-5".into(),
        "high".into(),
    );
    task.initial_approved = true;
    task.workspace = Some(root.into());
    task.phase = phase;
    task.approved = Some(contract("npm test"));
    app.tasks.selected = Some(task.id.clone());
    app.tasks.live = Some(task.id.clone());
    app.tasks.records = vec![task];
    app
}
fn finish_job(app: &mut Pui) {
    let job = app.tasks.job.take().expect("bounded read-only job");
    app.task_job_result(job.rx.recv_timeout(Duration::from_secs(10)).unwrap());
}

#[test]
fn dependency_backed_fixture_reports_concrete_missing_local_deps_without_installing() {
    let source = fixture();
    let candidate = fixture();
    let manifest =
        r#"{"scripts":{"test":"node fixture.cjs"},"devDependencies":{"fixture-only":"1.0.0"}}"#;
    fs::write(source.join("package.json"), manifest).unwrap();
    fs::write(candidate.join("package.json"), manifest).unwrap();
    fs::create_dir(source.join("node_modules")).unwrap();
    let report = inspect(&source, &candidate, Some(&contract("npm test")));
    assert!(
        report
            .blockers
            .iter()
            .any(|s| s.contains("node_modules is absent"))
    );
    assert!(report.note().contains("not an implementation failure"));
    assert!(!candidate.join("node_modules").exists());
    fs::create_dir(candidate.join("node_modules")).unwrap();
    let report = inspect(&source, &candidate, Some(&contract("npm test")));
    assert!(!report.blockers.iter().any(|s| s.contains("node_modules")));
    assert!(
        report
            .evidence
            .iter()
            .any(|s| s.contains("contents and usability are unverified"))
    );
}
#[test]
fn incidental_manifest_and_unknown_compound_commands_do_not_invent_requirements() {
    let root = fixture();
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"unrelated":"1"}}"#,
    )
    .unwrap();
    for command in [
        "cargo test --offline",
        "cd native && cargo test --offline",
        "./custom-tests",
        "npm --version",
    ] {
        let report = inspect(&root, &root, Some(&contract(command)));
        assert!(
            !report.blockers.iter().any(|s| s.contains("node_modules")),
            "{command}"
        );
        assert!(!report.note().contains("environment healthy"));
    }
}
#[cfg(unix)]
#[test]
fn dependency_and_manifest_symlinks_are_not_followed_or_copied() {
    use std::os::unix::fs::symlink;
    let root = fixture();
    let outside = fixture();
    fs::write(root.join("package.json"), r#"{"dependencies":{"x":"1"}}"#).unwrap();
    symlink(&outside, root.join("node_modules")).unwrap();
    let report = inspect(&root, &root, Some(&contract("npm test")));
    assert!(
        report
            .blockers
            .iter()
            .any(|s| s.contains("link was not followed or copied"))
    );
    let other = fixture();
    symlink(outside.join("credentials"), other.join("package.json")).unwrap();
    // Broken links must be rejected via symlink_metadata, never followed.
    assert!(
        package_dependencies(&other)
            .unwrap_err()
            .contains("symlinks")
    );
}
#[test]
fn environment_blocker_before_implementation_or_repair_does_not_consume_attempt() {
    let root = fixture();
    fs::write(
        root.join("package.json"),
        r#"{"devDependencies":{"fixture":"1"}}"#,
    )
    .unwrap();
    let mut app = app(&root, Phase::Preparing);
    app.task_job_result(Ok(JobResult::Prepared(root.clone(), false, None, None)));
    finish_job(&mut app);
    let task = app.tasks.current().unwrap();
    assert_eq!(task.phase, Phase::Blocked);
    assert!(task.attempts.is_empty());
    assert!(task.runs.is_empty());
    assert!(app.tasks.processes.is_empty());
    assert!(
        task.note
            .contains("Automatic dependency/bootstrap setup is not supported")
    );

    app.tasks.live = app.tasks.selected.clone();
    let task = app.tasks.live_mut().unwrap();
    task.phase = Phase::Checking;
    task.attempts.push(Attempt::new(1));
    app.task_job_result(Ok(JobResult::Environment(
        Stage::Repair(2),
        inspect(&root, &root, Some(&contract("npm test"))),
    )));
    assert_eq!(app.tasks.current().unwrap().attempts.len(), 1);
    assert_eq!(app.tasks.current().unwrap().latest().unwrap().number, 1);
    assert!(!app.tasks.current().unwrap().latest().unwrap().invalidated);
}
#[test]
fn cancel_during_preflight_cannot_launch_a_provider_or_create_an_attempt() {
    let root = fixture();
    let mut app = app(&root, Phase::Preparing);
    app.task_job_result(Ok(JobResult::Prepared(root, false, None, None)));
    let job = app.tasks.job.take().unwrap();
    app.tasks.stop();
    app.task_job_result(job.rx.recv_timeout(Duration::from_secs(10)).unwrap());
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Cancelled);
    assert!(app.tasks.current().unwrap().attempts.is_empty());
    assert!(app.tasks.processes.is_empty());
}
#[test]
fn check_prerequisites_are_reinspected_after_candidate_capture() {
    let root = fixture();
    fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"fixture":"1"}}"#,
    )
    .unwrap();
    let mut app = app(&root, Phase::Capturing);
    app.tasks.live_mut().unwrap().attempts.push(Attempt::new(0));
    app.task_job_result(Ok(JobResult::Fingerprint(
        FingerprintStep::Candidate,
        "candidate".into(),
    )));
    finish_job(&mut app);
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Blocked);
    assert_eq!(app.tasks.current().unwrap().attempts.len(), 1);
    assert!(
        app.tasks
            .current()
            .unwrap()
            .latest()
            .unwrap()
            .checks
            .is_empty()
    );
    assert!(app.tasks.processes.is_empty());
}
#[test]
fn only_unsuccessful_requirement_evidence_is_classified_and_never_replayed() {
    let mut check = Check {
        criterion: "C1".into(),
        command: "npm test".into(),
        exit_code: Some(1),
        output: "Error: Cannot find module 'fixture-only'".into(),
        truncated: false,
        cancelled: false,
        duration_ms: 1,
    };
    assert!(
        failed_requirement(&check)
            .unwrap()
            .contains("No verifier or automatic repair")
    );
    check.exit_code = Some(0);
    assert!(failed_requirement(&check).is_none());
    check.exit_code = None;
    assert!(
        failed_requirement(&check).is_none(),
        "uncertain effects cannot be reclassified as safe retries"
    );
    check.exit_code = Some(1);
    check.output = "assertion failed: expected preserved draft".into();
    assert!(failed_requirement(&check).is_none());
}
#[test]
fn recovery_plan_is_scoped_nonmutating_and_does_not_reset_two_repair_limit() {
    let root = fixture();
    let mut app = app(&root, Phase::Interrupted);
    app.tasks.live = None;
    let task = app.tasks.current_mut().unwrap();
    let mut attempt = Attempt::new(2);
    attempt.fingerprint = Some("candidate".into());
    attempt.invalidated = true;
    task.attempts.push(attempt);
    let id = task.id.clone();
    let before = serde_json::to_value(&app.tasks.records).unwrap();
    let plan = app.task_recovery_plan(&id);
    assert!(plan.detail.contains("0 repair round(s)"));
    assert!(
        plan.detail
            .contains("replay interrupted shell/tool effects")
    );
    assert!(plan.detail.contains("fresh objective-bound consent"));
    assert!(
        app.task_recovery_plan("another-task")
            .summary
            .contains("unavailable")
    );
    assert_eq!(before, serde_json::to_value(&app.tasks.records).unwrap());
    assert!(app.tasks.job.is_none());
    assert!(app.tasks.processes.is_empty());
}
