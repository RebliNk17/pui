use super::*;
fn app() -> Pui {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.state = State {
        model: "fixture-model".into(),
        provider: "fixture-provider".into(),
        thinking: "high".into(),
        ..Default::default()
    };
    app
}
fn contract() -> Contract {
    Contract {
        criteria: vec![domain::Criterion {
            id: "C1".into(),
            description: "Draft survives chat switching".into(),
            command: "cargo test task_draft_survives_chat_switch".into(),
        }],
    }
}
fn live(app: &mut Pui, phase: Phase) {
    let mut t = Task::new(
        app.prefs.cwd.clone(),
        "fixture-provider".into(),
        "fixture-model".into(),
        "high".into(),
    );
    t.objective = "Preserve draft".into();
    t.workspace = Some(app.prefs.cwd.clone());
    t.phase = phase;
    t.proposed = contract();
    t.approved = Some(contract());
    t.attempts.push(Attempt::new(0));
    app.tasks.live = Some(t.id.clone());
    app.tasks.selected = Some(t.id.clone());
    app.tasks.records = vec![t];
}
fn process(app: &mut Pui, role: Role) {
    let (client, _) = rpc::Client::stub_with_events();
    app.tasks.live_mut().unwrap().runs.push(Run {
        id: "run".into(),
        role,
        attempt: 0,
        status: "Working".into(),
        session: None,
        summary: String::new(),
        cost: 0.0,
        permission_cost: 0.0,
    });
    app.tasks.processes.push(Process {
        run: "run".into(),
        role,
        client,
        state: State::default(),
        pending: Some(Pending {
            id: "prompt".into(),
            command: "prompt".into(),
            started: std::time::Instant::now(),
            observed: false,
            accepted: false,
            settled: false,
            aborted: false,
        }),
        initializing: false,
        ended: false,
        final_error: false,
    });
}
fn event(app: &mut Pui, v: Value) {
    app.task_event(0, rpc::Event::Record(v));
}
fn assistant(app: &mut Pui, text: &str) {
    event(
        app,
        json!({"type":"message_start","message":{"role":"assistant","content":[]}}),
    );
    event(
        app,
        json!({"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":text}]}}),
    );
}
fn passed_attempt(t: &mut Task) {
    let a = t.attempts.last_mut().unwrap();
    a.fingerprint = Some("fingerprint".into());
    a.checks = vec![Check {
        criterion: "C1".into(),
        command: contract().criteria[0].command.clone(),
        exit_code: Some(0),
        output: "one test passed".into(),
        truncated: false,
        cancelled: false,
        duration_ms: 1,
    }];
    a.review = Some(Review {
        summary: "Checked the interaction test and implementation".into(),
        criteria: vec![domain::Finding {
            id: "C1".into(),
            status: Verdict::Pass,
            evidence: "Regression test switches chats and compares exact draft".into(),
        }],
    });
}
#[test]
fn task_draft_survives_chat_switch() {
    let mut app = app();
    app.new_task();
    app.tasks.current_mut().unwrap().objective = "Keep this task draft".into();
    let id = app.tasks.selected.clone();
    app.new_session();
    assert!(!app.tasks.open);
    assert_eq!(app.tasks.selected, id);
    assert_eq!(
        app.tasks.current().unwrap().objective,
        "Keep this task draft"
    );
    app.tasks.open = true;
    assert_eq!(
        app.tasks.current().unwrap().objective,
        "Keep this task draft"
    );
    assert!(app.tasks.live.is_none());
}
#[test]
fn new_task_never_prompts_and_keeps_chat_draft() {
    let mut app = app();
    app.draft = "ordinary chat draft".into();
    app.new_task();
    assert_eq!(app.draft, "ordinary chat draft");
    assert!(app.tasks.processes.is_empty());
    assert!(app.tasks.job.is_none());
    assert!(app.tasks.open);
    assert!(app.tasks.current().unwrap().approved.is_none());
}
#[test]
fn agent_end_and_unobserved_settle_cannot_complete_a_task() {
    let mut app = app();
    live(&mut app, Phase::Proposing);
    process(&mut app, Role::Proposal);
    event(&mut app, json!({"type":"agent_end"}));
    assert_eq!(app.tasks.live_task().unwrap().phase, Phase::Proposing);
    event(&mut app, json!({"type":"agent_settled","aborted":false}));
    assert_eq!(app.tasks.live_task().unwrap().phase, Phase::Proposing);
}
#[test]
fn fast_settle_before_prompt_ack_waits_for_acceptance() {
    let mut app = app();
    live(&mut app, Phase::Proposing);
    app.tasks.live_mut().unwrap().approved = None;
    process(&mut app, Role::Proposal);
    event(&mut app, json!({"type":"agent_start"}));
    assistant(&mut app, &serde_json::to_string(&contract()).unwrap());
    event(&mut app, json!({"type":"agent_settled","aborted":false}));
    assert_eq!(app.tasks.live_task().unwrap().phase, Phase::Proposing);
    event(
        &mut app,
        json!({"type":"response","id":"prompt","command":"prompt","success":true,"data":{"disposition":"started"}}),
    );
    assert!(app.tasks.live.is_none());
    assert_eq!(app.tasks.current().unwrap().phase, Phase::AwaitingApproval);
    assert!(app.tasks.current().unwrap().approved.is_none()); // A proposal is not execution approval.
}
#[test]
fn fixture_workflow_runs_builder_checks_and_fresh_verifier_end_to_end() {
    let mut app = app();
    live(&mut app, Phase::Building);
    let root = std::env::temp_dir().join(format!("pui-task-e2e-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("draft.rs"), "fn preserve_draft() {}").unwrap();
    app.tasks.live_mut().unwrap().workspace = Some(root);
    process(&mut app, Role::Builder);
    event(&mut app, json!({"type":"agent_start"}));
    event(
        &mut app,
        json!({"type":"response","id":"prompt","command":"prompt","success":true,"data":{"disposition":"started"}}),
    );
    assistant(
        &mut app,
        "Implemented draft preservation and regression coverage.",
    );
    event(&mut app, json!({"type":"agent_end"}));
    assert_eq!(app.tasks.live_task().unwrap().phase, Phase::Building);
    event(&mut app, json!({"type":"agent_settled","aborted":false}));
    fn job(app: &mut Pui) {
        let job = app.tasks.job.take().expect("off-thread fingerprint job");
        let result = job.rx.recv_timeout(Duration::from_secs(3)).unwrap();
        app.task_job_result(result);
    }
    fn init(app: &mut Pui) {
        let id = app.tasks.processes[0].pending.as_ref().unwrap().id.clone();
        event(
            app,
            json!({"type":"response","command":"get_state","id":id,"success":true,"data":{"model":{"id":"fixture-model","provider":"fixture-provider"},"thinkingLevel":"high","sessionFile":"/fixture/task-session"}}),
        );
    }
    job(&mut app);
    assert_eq!(app.tasks.live_task().unwrap().phase, Phase::Checking);
    init(&mut app);
    let id = app.tasks.processes[0].pending.as_ref().unwrap().id.clone();
    event(
        &mut app,
        json!({"type":"response","command":"bash","id":id,"success":true,"data":{"exitCode":0,"output":"test task_draft_survives_chat_switch ... ok","cancelled":false}}),
    );
    job(&mut app);
    assert_eq!(app.tasks.live_task().unwrap().phase, Phase::Verifying);
    assert!(app.tasks.processes[0].state.messages.is_empty());
    init(&mut app);
    let id = app.tasks.processes[0].pending.as_ref().unwrap().id.clone();
    event(
        &mut app,
        json!({"type":"response","command":"prompt","id":id,"success":true,"data":{"disposition":"started"}}),
    );
    event(&mut app, json!({"type":"agent_start"}));
    assistant(
        &mut app,
        r#"{"summary":"Regression and code inspected","criteria":[{"id":"C1","status":"pass","evidence":"draft.rs and recorded interaction-test result"}]}"#,
    );
    event(&mut app, json!({"type":"agent_settled","aborted":false}));
    job(&mut app);
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Ready);
    assert!(app.tasks.live.is_none());
    assert_eq!(app.tasks.current().unwrap().runs.len(), 3);
    assert!(
        app.tasks
            .current()
            .unwrap()
            .latest()
            .unwrap()
            .passed(&contract())
    );
}
#[test]
fn check_exit_status_is_correlated_and_not_inferred_from_prose() {
    let mut app = app();
    live(&mut app, Phase::Checking);
    process(&mut app, Role::Checks);
    app.tasks.processes[0].pending.as_mut().unwrap().command = "bash".into();
    event(
        &mut app,
        json!({"type":"response","id":"unrelated","command":"bash","success":true,"data":{"exitCode":0,"output":"passed"}}),
    );
    assert!(
        app.tasks
            .live_task()
            .unwrap()
            .latest()
            .unwrap()
            .checks
            .is_empty()
    );
    event(
        &mut app,
        json!({"type":"response","id":"prompt","command":"bash","success":true,"data":{"exitCode":1,"output":"looks good but failed","cancelled":false}}),
    );
    assert!(!app.tasks.live_task().unwrap().latest().unwrap().checks[0].passed());
}
#[test]
fn source_change_refuses_acceptance_and_marks_evidence_stale() {
    let mut app = app();
    live(&mut app, Phase::Ready);
    passed_attempt(app.tasks.live_mut().unwrap());
    app.task_job_result(Ok(JobResult::Fingerprint(
        FingerprintStep::Accept,
        "changed".into(),
    )));
    let t = app.tasks.current().unwrap();
    assert_eq!(t.phase, Phase::Stale);
    assert!(t.latest().unwrap().invalidated);
    assert!(app.tasks.live.is_none());
}
#[test]
fn matching_candidate_requires_explicit_acceptance_after_checks() {
    let mut app = app();
    live(&mut app, Phase::Verifying);
    passed_attempt(app.tasks.live_mut().unwrap());
    app.task_finish_attempt();
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Ready);
    assert!(app.tasks.live.is_none());
    app.tasks.live = app.tasks.selected.clone();
    app.task_job_result(Ok(JobResult::Fingerprint(
        FingerprintStep::Accept,
        "fingerprint".into(),
    )));
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Accepted);
}
#[test]
fn repair_is_bounded_to_initial_plus_two_attempts() {
    let mut app = app();
    live(&mut app, Phase::Verifying);
    passed_attempt(app.tasks.live_mut().unwrap());
    for number in 0..=2 {
        let t = app.tasks.live_mut().unwrap();
        let a = t.attempts.last_mut().unwrap();
        a.number = number;
        a.review.as_mut().unwrap().criteria[0].status = Verdict::Fail;
        app.task_finish_attempt();
        if number < 2 {
            assert_eq!(app.tasks.live_task().unwrap().phase, Phase::Building);
            assert_eq!(
                app.tasks.live_task().unwrap().latest().unwrap().number,
                number + 1
            );
            app.tasks.retire();
            passed_attempt(app.tasks.live_mut().unwrap());
        }
    }
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Failed);
    assert_eq!(app.tasks.current().unwrap().attempts.len(), 3);
    assert!(app.tasks.live.is_none());
}
#[test]
fn cancel_and_disconnect_never_advance_or_resend() {
    let mut app = app();
    live(&mut app, Phase::Building);
    process(&mut app, Role::Builder);
    app.tasks.stop();
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Cancelled);
    assert!(app.tasks.processes.is_empty());
    assert!(app.tasks.live.is_none());
    live(&mut app, Phase::Checking);
    process(&mut app, Role::Checks);
    app.task_event(0, rpc::Event::Exited);
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Blocked);
    assert!(app.tasks.processes.is_empty());
}
#[test]
fn fresh_verifier_receives_checks_not_builder_rationale() {
    let mut app = app();
    live(&mut app, Phase::Verifying);
    passed_attempt(app.tasks.live_mut().unwrap());
    app.tasks.live_mut().unwrap().runs.push(Run {
        id: "builder".into(),
        role: Role::Builder,
        attempt: 0,
        status: "Finished".into(),
        session: None,
        summary: "BUILDER_RATIONALE_MUST_NOT_LEAK".into(),
        cost: 0.0,
        permission_cost: 0.0,
    });
    let prompt = app.task_prompt(Role::Verifier);
    assert!(prompt.contains("one test passed"));
    assert!(!prompt.contains("BUILDER_RATIONALE_MUST_NOT_LEAK"));
}
#[test]
fn model_mismatch_blocks_before_prompt_and_abort_cannot_pass() {
    let mut app = app();
    live(&mut app, Phase::Building);
    process(&mut app, Role::Builder);
    app.tasks.processes[0].initializing = true;
    event(
        &mut app,
        json!({"type":"response","id":"prompt","command":"get_state","success":true,"data":{"model":{"id":"other","provider":"fixture-provider"},"thinkingLevel":"high"}}),
    );
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Blocked);
    assert!(app.tasks.processes.is_empty());
    live(&mut app, Phase::Building);
    process(&mut app, Role::Builder);
    event(&mut app, json!({"type":"agent_start"}));
    event(
        &mut app,
        json!({"type":"response","id":"prompt","command":"prompt","success":true,"data":{"disposition":"started"}}),
    );
    assistant(&mut app, "done");
    event(
        &mut app,
        json!({"type":"message_end","message":{"role":"assistant","stopReason":"aborted","content":[]}}),
    );
    event(&mut app, json!({"type":"agent_settled","aborted":false}));
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Blocked);
    assert!(app.tasks.job.is_none());
}
#[test]
fn blocked_review_does_not_spend_on_repairs() {
    let mut app = app();
    live(&mut app, Phase::Verifying);
    passed_attempt(app.tasks.live_mut().unwrap());
    app.tasks.live_mut().unwrap().attempts[0]
        .review
        .as_mut()
        .unwrap()
        .criteria[0]
        .status = Verdict::Blocked;
    app.task_finish_attempt();
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Blocked);
    assert_eq!(app.tasks.current().unwrap().attempts.len(), 1);
}
#[test]
fn workflow_nodes_open_agent_transcripts_and_check_evidence() {
    let mut app = app();
    app.demo = true;
    app.demo_task();
    let ctx = app.ctx.clone();
    fn locate(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(t) if t.galley.job.text == label => {
                Some(t.pos + t.galley.size() / 2.0)
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| locate(s, label)),
            _ => None,
        }
    }
    for (label, expected) in [
        ("Builder", Some("demo-Builder")),
        ("Fresh verifier", Some("demo-Verifier")),
        ("Check runner", None),
        ("Result & evidence", None),
    ] {
        let mut position = None;
        for _ in 0..3 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1480.0, 960.0),
                    )),
                    ..Default::default()
                },
                |ctx| app.inspector(ctx),
            );
            position = output.shapes.iter().find_map(|s| locate(&s.shape, label));
        }
        let pos = position.unwrap_or_else(|| panic!("Missing workflow node {label}"));
        for pressed in [true, false] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1480.0, 960.0),
                    )),
                    events: vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ctx| app.inspector(ctx),
            );
        }
        assert_eq!(app.tasks.selected_run.as_deref(), expected);
        assert!(app.tasks.open);
    }
}
fn texts(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
        egui::Shape::Vec(v) => {
            for s in v {
                texts(s, out)
            }
        }
        _ => {}
    }
}
#[test]
fn right_side_workflow_and_demo_show_actual_roles_without_a_provider() {
    let mut app = app();
    app.demo = true;
    app.demo_task();
    let ctx = app.ctx.clone();
    let mut labels = vec![];
    for _ in 0..3 {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1480.0, 960.0),
                )),
                ..Default::default()
            },
            |ctx| {
                app.inspector(ctx);
                app.task_panel(ctx);
            },
        );
        labels.clear();
        for s in output.shapes {
            texts(&s.shape, &mut labels);
        }
    }
    for label in [
        "Workflow",
        "Acceptance checks",
        "Builder",
        "Check runner",
        "Fresh verifier",
        "Result & evidence",
    ] {
        assert!(
            labels.iter().any(|s| s == label),
            "missing {label}: {labels:?}"
        );
    }
    assert!(app.tasks.processes.is_empty());
    assert!(app.tasks.job.is_none());
    assert!(app.tasks.store.is_none());
}
