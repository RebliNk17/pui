use super::*;

#[test]
fn cargo_environment_patterns_require_specific_prerequisite_diagnostics() {
    let mut check = Check {
        criterion: "C1".into(),
        command: "cargo test --offline".into(),
        exit_code: Some(101),
        output: String::new(),
        truncated: false,
        cancelled: false,
        duration_ms: 1,
    };
    for output in [
        "error: failed to download `fixture v1.0.0`\nCaused by: checksum mismatch",
        "assertion failed: attempting to make an HTTP request, but --offline was specified",
        "error: linker `cc` failed: exit status: 1\nundefined symbol not found",
        "assertion failed: expected linker `cc` not found",
        "error[E0425]: cannot find value `not_found` in this scope",
    ] {
        check.output = output.into();
        assert!(failed_requirement(&check).is_none(), "{output}");
    }
}

#[test]
fn missing_prerequisite_cancel_or_uncertain_check_stops_before_next_command_or_repairs() {
    for (check_command, number, data) in [
        (
            "npm test",
            2,
            json!({"exitCode":1,"output":"Error: Cannot find module 'fixture-only'","cancelled":false}),
        ),
        (
            "npm test",
            2,
            json!({"exitCode":0,"output":"Stopped","cancelled":true}),
        ),
        (
            "npm test",
            2,
            json!({"output":"Outcome unknown","cancelled":false}),
        ),
        (
            "cargo test --offline",
            0,
            json!({"exitCode":101,"output":"error: failed to download `fixture-crate v1.0.0`\n\nCaused by:\n  attempting to make an HTTP request, but --offline was specified","cancelled":false}),
        ),
        (
            "cargo test --offline",
            1,
            json!({"exitCode":101,"output":"error: linker `cc` not found\n  |\n  = note: No such file or directory (os error 2)","cancelled":false}),
        ),
    ] {
        let root = fixture();
        let mut app = app(&root, Phase::Checking);
        let task = app.tasks.live_mut().unwrap();
        task.attempts.push(Attempt::new(number));
        task.approved.as_mut().unwrap().criteria[0].command = check_command.into();
        task.approved
            .as_mut()
            .unwrap()
            .criteria
            .push(domain::Criterion {
                id: "C2".into(),
                description: "Must not run after uncertain prior effect".into(),
                command: "npm run second".into(),
            });
        app.task_launch(Role::Checks);
        let (client, _, sent) = rpc::Client::stub_with_records();
        app.tasks.processes[0].client = client;
        let init_id = app.tasks.processes[0].pending.as_ref().unwrap().id.clone();
        app.task_event(0, rpc::Event::Record(json!({"type":"response","command":"get_state","id":init_id,"success":true,"data":{"model":{"id":"claude-haiku-5-5","provider":"fixture-provider"},"thinkingLevel":"high"}})));
        let command = sent.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(command["command"], check_command);
        let id = app.tasks.processes[0].pending.as_ref().unwrap().id.clone();
        app.task_event(
            0,
            rpc::Event::Record(
                json!({"type":"response","command":"bash","id":id,"success":true,"data":data}),
            ),
        );
        let task = app.tasks.current().unwrap();
        assert_eq!(task.phase, Phase::Blocked);
        assert_eq!(task.attempts.len(), 1);
        assert_eq!(task.latest().unwrap().number, number);
        assert_eq!(task.latest().unwrap().checks.len(), 1);
        assert!(task.latest().unwrap().review.is_none());
        assert!(task.runs.iter().all(|run| run.role == Role::Checks));
        assert!(app.tasks.processes.is_empty());
        assert!(app.tasks.live.is_none());
        assert!(
            sent.try_iter().all(|record| record["type"] != "bash"),
            "no second or replayed command"
        );
    }
}
