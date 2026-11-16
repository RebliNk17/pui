use super::*;

fn app() -> Pui {
    Pui::new(
        &eframe::CreationContext::_new_kittest(egui::Context::default()),
        Options {
            demo: true,
            ..Default::default()
        },
    )
}
fn task() -> Task {
    let source =
        std::env::temp_dir().join(format!("pui-operation-source-{}", uuid::Uuid::new_v4()));
    let candidate = source.with_extension("candidate");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&candidate).unwrap();
    let mut task = Task::new(source, "fixture".into(), "model".into(), "high".into());
    task.workspace = Some(candidate);
    task.isolated = true;
    task.phase = Phase::Blocked;
    task.objective = "Bounded setup fixture".into();
    task
}
fn writable(app: &mut Pui, task: Task) -> Receiver<StoreCommand> {
    app.demo = false;
    app.agents.active = "operations-owner".into();
    let (sender, receiver) = mpsc::channel();
    app.tasks.store = Some(sender);
    app.tasks.loaded = None;
    app.tasks.selected = Some(task.id.clone());
    app.tasks
        .parents
        .insert(task.id.clone(), app.agents.active.clone());
    app.tasks.records.push(task);
    receiver
}
fn plan(task: &Task) -> setup::Plan {
    setup::plan(task)
        .unwrap()
        .with_commands(vec!["user-reviewed-fixture-setup".into()])
        .unwrap()
}

#[test]
fn pending_operation_confirmation_keeps_attention_unread_before_modal_render() {
    let mut app = app();
    let ctx = app.ctx.clone();
    let task = Task::new(
        std::env::temp_dir(),
        "fixture".into(),
        "model".into(),
        "high".into(),
    );
    let store = writable(&mut app, task.clone());
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.attention_event(&serde_json::json!({"type":"agent_start"}));
    app.attention_event(&serde_json::json!({"type":"agent_settled"}));
    assert_eq!(app.attention.unread(), 1);
    app.tasks.operations.decision = Some(Decision::RollBack {
        expected: task,
        transaction: "attention-gate".into(),
    });
    for _ in 0..3 {
        let _ = ctx.run(
            egui::RawInput {
                focused: true,
                ..Default::default()
            },
            |ctx| {
                app.attention_inputs(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.attention_button(ui);
                    app.attention_rendered(ui);
                });
            },
        );
    }
    assert_eq!(app.attention.unread(), 1);
    assert!(!app.attention.open);
    assert!(store.try_recv().is_err() && commands.try_recv().is_err());
    app.tasks.operations.decision = None;
    let _ = ctx.run(
        egui::RawInput {
            focused: true,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.attention_rendered(ui));
        },
    );
    assert_eq!(app.attention.unread(), 0);
    assert!(store.try_recv().is_err() && commands.try_recv().is_err());
}

#[test]
fn asynchronous_setup_plan_is_rejected_after_owner_or_permission_drift() {
    let mut app = app();
    let task = task();
    let commands = writable(&mut app, task.clone());
    let captured = app.operation_scope();
    let (sender, receiver) = mpsc::channel();
    app.tasks.operations.review_scope = Some(captured);
    app.tasks.operations.reading = Some((task.id.clone(), receiver));
    sender
        .send(Ok(ReadResult::SetupReady(task.clone(), plan(&task))))
        .unwrap();
    app.agents.active = "another-owner".into();
    app.poll_task_operations();
    assert!(app.tasks.setup.is_none());
    assert!(app.tasks.operations.decision.is_none());
    assert!(commands.try_recv().is_err());
    assert!(
        app.state
            .notice
            .as_ref()
            .unwrap()
            .contains("context changed")
    );
}

#[test]
fn cancelled_setup_after_scope_drift_gets_one_terminal_ack_and_clears() {
    let mut app = app();
    let task = task();
    let commands = writable(&mut app, task.clone());
    let process = setup::Process::start(&task, plan(&task), app.ctx.clone(), json!({})).unwrap();
    app.tasks.operations.setup_owner = Some(app.operation_scope());
    app.tasks.operations.setup_baseline = Some(task.clone());
    app.tasks.setup = Some(process);
    app.agents.active = "another-owner".into();
    app.poll_task_operations();
    let StoreCommand::SaveAcknowledged {
        expected,
        task: saved,
        reply,
    } = commands.recv_timeout(Duration::from_secs(2)).unwrap()
    else {
        panic!("durable terminal evidence required");
    };
    assert!(expected.setup.is_empty());
    assert_eq!(saved.setup[0].status, domain::setup::SetupStatus::Cancelled);
    assert_eq!(saved.phase, Phase::Stale);
    assert!(!saved.implementation_pending);
    reply.send(Ok(())).unwrap();
    app.poll_task_operations();
    assert!(
        app.tasks.setup.is_none(),
        "terminal acknowledgement must not require old owning scope"
    );
    assert!(app.tasks.operations.saving.is_none());
    app.poll_task_operations();
    assert!(
        commands.try_recv().is_err(),
        "terminal evidence must not save every frame"
    );
    assert!(app.tasks.processes.is_empty());
}

#[test]
fn setup_cannot_submit_without_durable_started_evidence_and_record_drift_rejects() {
    let mut app = app();
    let task = task();
    let commands = writable(&mut app, task.clone());
    assert_eq!(app.task_operation_guard(&task), Ok(()));
    let mut changed = task.clone();
    changed.note = "changed".into();
    assert!(app.task_operation_guard(&changed).is_err());
    let process = setup::Process::start(
        &task,
        plan(&task),
        app.ctx.clone(),
        json!({"permission":"bypass"}),
    )
    .unwrap();
    app.tasks.operations.setup_owner = Some(app.operation_scope());
    app.tasks.operations.setup_baseline = Some(task.clone());
    app.tasks.setup = Some(process);
    app.poll_task_operations();
    let StoreCommand::SaveAcknowledged {
        task: saved, reply, ..
    } = commands.recv_timeout(Duration::from_secs(2)).unwrap()
    else {
        panic!();
    };
    assert_eq!(saved.setup[0].results[0].exit_code, None);
    assert!(app.tasks.setup.as_ref().unwrap().needs_persist());
    assert!(
        app.tasks.current().unwrap().setup.is_empty(),
        "projection waits for durable receipt"
    );
    reply.send(Err("ownership conflict".into())).unwrap();
    app.poll_task_operations();
    assert!(app.tasks.setup.is_none());
    assert!(!app.tasks.storage_failed);
    assert!(app.tasks.operations.quarantined.contains(&task.id));
    let StoreCommand::ReleaseCurrent { id, reply } =
        commands.recv_timeout(Duration::from_secs(2)).unwrap()
    else {
        panic!("read-only reconciliation must release ownership");
    };
    assert_eq!(id, task.id);
    reply
        .send(Ok(refresh::Snapshot {
            records: vec![task.clone()],
            owned: Default::default(),
        }))
        .unwrap();
    app.poll_task_operations();
    assert!(!app.tasks.operations.quarantined.contains(&task.id));
    assert_eq!(
        serde_json::to_value(app.tasks.current()).unwrap(),
        serde_json::to_value(&task).unwrap()
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn restart_marks_setup_uncertainty_stale_and_preserves_evidence_without_replay() {
    let mut task = task();
    let mut process =
        setup::Process::start(&task, plan(&task), egui::Context::default(), json!({})).unwrap();
    task.implementation_pending = true;
    task.setup.push(process.record.clone());
    task.recover();
    assert_eq!(task.phase, Phase::Stale);
    assert_eq!(task.setup[0].status, domain::setup::SetupStatus::Unknown);
    assert!(!task.implementation_pending);
    assert_eq!(task.setup[0].results.len(), 1);
    process.cancel();
    process.retire();
}

#[test]
fn setup_confirmation_cancel_and_disabled_states_are_bounded_in_both_themes() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        let mut app = app();
        let task = task();
        let commands = writable(&mut app, task.clone());
        theme.apply(&app.ctx);
        app.tasks.operations.review_scope = Some(app.operation_scope());
        let plan = plan(&task);
        app.tasks.operations.decision = Some(Decision::Setup {
            expected: task.clone(),
            plan,
            commands: "one exact command".into(),
        });
        // Context drift disables affirmative action; Escape always cancels.
        app.agents.active = "foreign".into();
        let ctx = app.ctx.clone();
        for _ in 0..3 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(420.0, 700.0),
                    )),
                    ..Default::default()
                },
                |ctx| app.task_operation_confirmation(ctx),
            );
        }
        assert!(app.tasks.setup.is_none());
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| app.task_operation_confirmation(ctx),
        );
        assert!(app.tasks.operations.decision.is_none());
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn authoritative_detach_after_setup_failure_clears_cached_parent_without_saving_it_back() {
    let mut app = app();
    let mut original = task();
    original.parent_session = Some(original.source.join("owner.jsonl"));
    let commands = writable(&mut app, original.clone());
    let mut detached = original.clone();
    detached.parent_session = None;
    app.tasks.operations.quarantined.insert(original.id.clone());
    let (reply, receiver) = mpsc::channel();
    app.tasks.operations.reconciling = Some((original.id.clone(), receiver));
    reply
        .send(Ok(refresh::Snapshot {
            records: vec![detached],
            owned: Default::default(),
        }))
        .unwrap();
    app.poll_task_operations();
    assert!(app.tasks.current().unwrap().parent_session.is_none());
    assert!(!app.tasks.parents.contains_key(&original.id));
    assert!(!app.tasks.operations.quarantined.contains(&original.id));
    assert!(commands.try_recv().is_err());
}

#[test]
fn reviewing_apply_never_writes_and_context_drift_cannot_confirm() {
    let mut app = app();
    let task = task();
    let commands = writable(&mut app, task.clone());
    let plan: domain::apply::Plan = serde_json::from_value(json!({
        "task_id": task.id, "identity": { "original": task.source, "candidate": task.workspace,
            "original_scope": task.source, "candidate_scope": task.workspace,
            "original_git_dir": "/fixture/original-git", "candidate_git_dir": "/fixture/candidate-git", "common_dir": "/fixture/original-git", "head": "baseline" },
        "baseline_fingerprint": "baseline", "original_fingerprint": "original", "candidate_fingerprint": "candidate", "checked_candidate_fingerprint": "candidate",
        "inventory": "inventory", "task_hash": "task", "paths": [], "patch_hash": "patch", "changes": [], "blockers": []
    })).unwrap();
    app.request_apply(&task, &plan);
    assert!(app.tasks.operations.decision.is_some());
    assert!(
        commands.try_recv().is_err(),
        "review is not application permission"
    );
    app.agents.active = "foreign-owner".into();
    assert!(app.confirm_apply(task.clone(), Some(plan), None).is_err());
    assert!(commands.try_recv().is_err());
    let ctx = app.ctx.clone();
    for _ in 0..3 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            app.task_operation_confirmation(ctx)
        });
    }
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| app.task_operation_confirmation(ctx),
    );
    assert!(app.tasks.operations.decision.is_none());
    assert!(commands.try_recv().is_err());
}
