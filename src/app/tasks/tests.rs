use super::*;
#[path = "reliability_tests.rs"]
mod reliability_tests;
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
    let owner = app.ensure_agent();
    app.tasks.parents.insert(t.id.clone(), owner);
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
        selection: None,
        routing_for: None,
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
        routing_for: None,
        route_prompt: None,
    });
}
#[test]
fn kanban_lists_live_task_agents_and_acceptance_approvals() {
    let mut app = app();
    live(&mut app, Phase::Building);
    process(&mut app, Role::Builder);
    let mut cards = Vec::new();
    app.task_board_cards(&mut cards);
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].status, kanban::Status::Working);
    app.tasks.processes[0]
        .state
        .dialogs
        .push_back(questions::demo_dialog());
    cards.clear();
    app.task_board_cards(&mut cards);
    assert_eq!(cards[0].status, kanban::Status::Waiting);
    let kanban::Target::Task { task, run } = cards.remove(0).target else {
        panic!()
    };
    app.open_task_from_board(task.clone(), run);
    assert!(app.tasks.open);
    assert_eq!(app.tasks.selected.as_ref(), Some(&task));
    assert_eq!(app.tasks.selected_run.as_deref(), Some("run"));
    assert_eq!(app.tasks.processes[0].state.dialogs.len(), 1);
    app.tasks.processes[0].ended = true;
    app.tasks.live_mut().unwrap().phase = Phase::AwaitingApproval;
    cards.clear();
    app.task_board_cards(&mut cards);
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].status, kanban::Status::Waiting);
    assert_eq!(cards[0].action, "View workflow");
    app.tasks.live = None;
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
fn initially_approved_proposal_starts_without_manual_check_approval() {
    let mut app = app();
    live(&mut app, Phase::Proposing);
    let root = std::env::temp_dir().join(format!("pui-workflow-auto-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let t = app.tasks.live_mut().unwrap();
    t.source = root;
    t.initial_approved = true;
    t.dirty_source = true;
    t.approved = None;
    process(&mut app, Role::Proposal);
    event(&mut app, json!({"type":"agent_start"}));
    event(
        &mut app,
        json!({"type":"response","id":"prompt","command":"prompt","success":true,"data":{"disposition":"started"}}),
    );
    assistant(&mut app, &serde_json::to_string(&contract()).unwrap());
    event(&mut app, json!({"type":"agent_settled","aborted":false}));
    assert_eq!(app.tasks.live_task().unwrap().phase, Phase::Preparing);
    assert!(app.tasks.live_task().unwrap().approved.is_some());
    assert!(app.tasks.job.is_some());
    app.tasks.stop();
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
        let result = job.rx.recv_timeout(Duration::from_secs(30)).unwrap();
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
fn source_change_marks_completed_workflow_evidence_stale() {
    let mut app = app();
    live(&mut app, Phase::Ready);
    passed_attempt(app.tasks.live_mut().unwrap());
    app.task_job_result(Ok(JobResult::Fingerprint(
        FingerprintStep::Revalidate,
        "changed".into(),
    )));
    let t = app.tasks.current().unwrap();
    assert_eq!(t.phase, Phase::Stale);
    assert!(t.latest().unwrap().invalidated);
    assert!(app.tasks.live.is_none());
}
#[test]
fn matching_candidate_completes_without_manual_acceptance() {
    let mut app = app();
    live(&mut app, Phase::Verifying);
    passed_attempt(app.tasks.live_mut().unwrap());
    app.task_finish_attempt();
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Ready);
    assert!(app.tasks.live.is_none());
    app.tasks.live = app.tasks.selected.clone();
    app.task_job_result(Ok(JobResult::Fingerprint(
        FingerprintStep::Revalidate,
        "fingerprint".into(),
    )));
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Ready);
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
        selection: None,
        routing_for: None,
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
fn task_deletion_is_ordered_after_saves_and_cannot_resurrect_from_stale_saves() {
    let mut app = app();
    let home = std::env::temp_dir().join(format!("pui-task-delete-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    let transcript = home.join("run.jsonl");
    std::fs::write(&transcript, "{\"type\":\"session\",\"cwd\":\"/fixture\"}\n").unwrap();
    live(&mut app, Phase::Cancelled);
    process(&mut app, Role::Builder);
    app.tasks.processes.clear();
    app.tasks.live = None;
    let task = app.tasks.current_mut().unwrap();
    task.runs[0].session = Some(transcript.clone());
    let stale = task.clone();
    let id = task.id.clone();
    let (store, rx) = mpsc::channel();
    let (errors, _) = mpsc::channel();
    app.tasks.store = Some(store.clone());
    app.tasks.save();
    let result = app
        .tasks
        .delete_saved(&id, vec![transcript.clone()], vec![])
        .unwrap();
    store.send(StoreCommand::Save(Box::new(stale))).unwrap();
    app.tasks.store.take();
    drop(store);
    run_store(&home, rx, errors, &app.ctx);
    result.recv().unwrap().unwrap();
    app.tasks.forget_deleted(&id);
    assert!(!transcript.exists());
    assert!(!home.join(format!("{id}.json")).exists());
    assert!(domain::store::load(&home).unwrap().is_empty());
    assert!(app.tasks.records.is_empty());
    assert!(!app.tasks.open);
    assert!(app.tasks.selected.is_none());
}
#[test]
fn task_run_session_is_persisted_as_soon_as_pi_reports_it() {
    let mut app = app();
    live(&mut app, Phase::Building);
    process(&mut app, Role::Builder);
    let (store, stored) = mpsc::channel();
    app.tasks.store = Some(store);
    app.task_event(0,rpc::Event::Record(json!({"type":"response","command":"get_state","success":true,"data":{"sessionFile":"/fixture/task-worker.jsonl","model":{"id":"fixture-model","provider":"fixture-provider"},"thinkingLevel":"high","isStreaming":true}})));
    let StoreCommand::Save(saved) = stored.try_recv().unwrap() else {
        panic!("Expected task save");
    };
    assert_eq!(
        saved.runs[0].session.as_deref(),
        Some(std::path::Path::new("/fixture/task-worker.jsonl"))
    );
    app.task_event(0,rpc::Event::Record(json!({"type":"tool_execution_start","toolCallId":"tool","toolName":"read","args":{"path":"src/main.rs"}})));
    assert!(
        stored.try_recv().is_err(),
        "No redundant task persistence for ordinary tool events"
    );
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
        let expected_tab = if expected.is_some() {
            work_item::Tab::Activity
        } else {
            work_item::Tab::Evidence
        };
        assert_eq!(
            app.work_item_tab(app.tasks.selected.as_ref().unwrap()),
            expected_tab
        );
    }
}
fn routing_catalog() -> Vec<crate::protocol::Model> {
    [
        ("claude-haiku-5-5", 0.1),
        ("claude-opus-5-5", 4.0),
        ("gpt-6.1-sol", 2.0),
        ("gpt-6-astra", 10.0),
    ]
    .into_iter()
    .map(|(id, price)| crate::protocol::Model {
        id: id.into(),
        name: id.into(),
        provider: "fixture-provider".into(),
        context_window: 600000,
        input_cost: price,
        output_cost: price * 5.0,
    })
    .collect()
}
#[test]
fn dynamic_stage_routes_then_launches_only_validated_selection() {
    let mut app = app();
    live(&mut app, Phase::Proposing);
    app.tasks.live_mut().unwrap().dynamic_routing = true;
    app.state.models = routing_catalog();
    app.task_launch(Role::Proposal);
    assert_eq!(app.tasks.processes[0].routing_for, Some(Role::Proposal));
    assert_eq!(
        app.tasks.live_task().unwrap().runs[0]
            .selection
            .as_ref()
            .unwrap()
            .model,
        "claude-haiku-5-5"
    );
    let id = app.tasks.processes[0].pending.as_ref().unwrap().id.clone();
    event(
        &mut app,
        json!({"type":"response","id":id,"command":"get_state","success":true,"data":{"model":{"id":"claude-haiku-5-5","provider":"fixture-provider"},"thinkingLevel":"off"}}),
    );
    let id = app.tasks.processes[0].pending.as_ref().unwrap().id.clone();
    event(&mut app, json!({"type":"agent_start"}));
    event(
        &mut app,
        json!({"type":"response","id":id,"command":"prompt","success":true,"data":{"disposition":"started"}}),
    );
    assistant(
        &mut app,
        r#"{"model":"gpt-6.1-sol","thinking":"high","difficulty":"standard","reason":"Inspecting native layout and executable test contracts warrants a technical standard model.","exceptional_reason":null}"#,
    );
    event(&mut app, json!({"type":"agent_settled","aborted":false}));
    assert_eq!(app.tasks.live_task().unwrap().runs.len(), 2);
    assert!(app.tasks.processes[0].routing_for.is_none());
    assert_eq!(
        app.tasks.live_task().unwrap().runs[1]
            .selection
            .as_ref()
            .unwrap()
            .model,
        "gpt-6.1-sol"
    );
}
#[test]
fn exceptional_request_pauses_before_any_expensive_launch() {
    let mut app = app();
    live(&mut app, Phase::Building);
    app.tasks.live_mut().unwrap().dynamic_routing = true;
    app.state.models = routing_catalog();
    app.task_route_finished(Role::Builder,r#"{"model":"gpt-6-astra","thinking":"high","difficulty":"exceptional","reason":"A remaining concurrency invariant needs specialized deep reasoning after ordinary approaches failed.","exceptional_reason":"The prior Opus and Sol attempts could not satisfy the interleaving invariant; request an explicit exceptional investigation rather than another routine implementation."}"#);
    let t = app.tasks.current().unwrap();
    assert_eq!(t.phase, Phase::Blocked);
    assert!(t.pending_route.is_none());
    assert!(t.note.contains("outside this workflow"));
    assert!(app.tasks.processes.is_empty());
    assert!(app.tasks.live.is_none());
}
#[test]
fn routed_model_mismatch_and_budget_stop_do_not_fallback_or_continue() {
    let mut app = app();
    live(&mut app, Phase::Proposing);
    app.tasks.live_mut().unwrap().dynamic_routing = true;
    app.state.models = routing_catalog();
    app.task_launch(Role::Proposal);
    let id = app.tasks.processes[0].pending.as_ref().unwrap().id.clone();
    event(
        &mut app,
        json!({"type":"response","id":id,"command":"get_state","success":true,"data":{"model":{"id":"gpt-6.1-sol","provider":"fixture-provider"},"thinkingLevel":"high"}}),
    );
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Blocked);
    assert!(app.tasks.processes.is_empty());
    live(&mut app, Phase::Building);
    app.tasks.live_mut().unwrap().spend_limit = Some(0.01);
    process(&mut app, Role::Builder);
    event(
        &mut app,
        json!({"type":"message_update","usage":{"cost":{"total":0.02}}}),
    );
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Blocked);
    assert!(app.tasks.processes.is_empty());
    assert!(app.tasks.live.is_none());
}
#[test]
fn old_tasks_remain_pinned_and_new_tasks_opt_into_dynamic_routing() {
    let t = Task::new("/tmp".into(), "p".into(), "m".into(), "high".into());
    let mut v = serde_json::to_value(&t).unwrap();
    for k in ["dynamic_routing", "spend_limit", "pending_route"] {
        v.as_object_mut().unwrap().remove(k);
    }
    let restored: Task = serde_json::from_value(v).unwrap();
    assert!(!restored.dynamic_routing);
    let mut app = app();
    app.new_task();
    assert!(app.tasks.current().unwrap().dynamic_routing);
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
fn workflow_long_titles_stay_inside_the_inspector_across_redraws() {
    for task_selected in [false, true] {
        for theme in [theme::Theme::Dark, theme::Theme::Light] {
            let mut app = app();
            app.demo = true;
            if task_selected {
                app.demo_task();
                let task = app.tasks.current_mut().unwrap();
                task.objective =
                    "A long task objective with observable behavior and acceptance checks "
                        .repeat(12);
                task.model = "model-with-an-extremely-long-unbroken-identifier-".repeat(12);
            }
            app.demo_agent_tree();
            for node in app.agents.nodes.values_mut() {
                node.name =
                    "this button at the top right corner, to see the files and views ".repeat(12);
                node.status = "Waiting for input".into();
            }
            let ctx = app.ctx.clone();
            theme.apply(&ctx);
            for width in [350.0, 260.0, 350.0] {
                for _ in 0..8 {
                    let mut bounds = None;
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::SidePanel::right("workflow-overflow-test").exact_width(width).show(ctx, |ui| {
                                let mut layout = CardLayout { order: vec![Card::Workflow], ..Default::default() };
                                layout.hidden.clear();
                                inspector::stack(ui, &mut layout, |_| String::new(), Some((Card::Workflow, "Open")), |ui, _| {
                                    // Expand the connected-agent branch in task mode as well.
                                    ui.style_mut().animation_time = 0.0;
                                    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), ui.make_persistent_id(egui::Id::new("Other connected agents")), true).store(ui.ctx());
                                    let available = ui.available_rect_before_wrap();
                                    app.task_workflow(ui);
                                    bounds = Some((available, ui.min_rect()));
                                });
                            });
                        },
                    );
                    let (available, actual) = bounds.unwrap();
                    assert!(
                        actual.right() <= available.right() + 1.0,
                        "Workflow overflows with task={task_selected}, width={width}: {actual:?} outside {available:?}"
                    );
                }
            }
        }
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
