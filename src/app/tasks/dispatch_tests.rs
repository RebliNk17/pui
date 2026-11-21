use super::*;

fn staged() -> (Pui, Receiver<StoreCommand>, Receiver<Value>) {
    let mut app = app();
    live(&mut app, Phase::Checking);
    process(&mut app, Role::Checks);
    let (client, _, sent) = rpc::Client::stub_with_records();
    app.tasks.processes[0].client = client;
    let (store, commands) = mpsc::channel();
    app.tasks.store = Some(store);
    app.task_stage_dispatch(
        0,
        json!({"type":"bash","command":"exact test","excludeFromContext":true}),
    );
    (app, commands, sent)
}
#[test]
fn dispatch_requires_matching_durable_receipt_and_cancel_fences_late_ack() {
    for cancel in [false, true] {
        let (mut app, commands, sent) = staged();
        assert!(sent.try_recv().is_err());
        let StoreCommand::SaveAcknowledged { task, reply, .. } =
            commands.recv_timeout(Duration::from_secs(2)).unwrap()
        else {
            panic!("receipt must precede dispatch");
        };
        assert_eq!(
            task.operations.last().unwrap().state,
            domain::engine::State::Dispatching
        );
        if cancel {
            app.tasks.stop();
        }
        reply.send(Ok(())).ok();
        app.task_poll_processes();
        let dispatched = sent
            .try_iter()
            .filter(|v| v["type"] == "bash")
            .collect::<Vec<_>>();
        assert_eq!(dispatched.len(), usize::from(!cancel));
        if cancel {
            assert_eq!(
                app.tasks
                    .current()
                    .unwrap()
                    .operations
                    .last()
                    .unwrap()
                    .state,
                domain::engine::State::UnknownEffect
            );
        }
    }
}
#[test]
fn native_resource_guard_tracks_live_work_without_extension_handle() {
    let mut app = app();
    live(&mut app, Phase::Building);
    let (client, _, replies) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let guard = json!({"action":"workflow_tool_guard"});
    assert!(app.workflow_handle_control(&json!({"id":"guard"}), &guard));
    let reply = replies.recv_timeout(Duration::from_secs(2)).unwrap();
    let value: Value = serde_json::from_str(reply["value"].as_str().unwrap()).unwrap();
    assert_eq!(value["blocked"], true);
    app.tasks.stop();
    app.workflow_handle_control(&json!({"id":"guard"}), &guard);
    let reply = replies.recv_timeout(Duration::from_secs(2)).unwrap();
    let value: Value = serde_json::from_str(reply["value"].as_str().unwrap()).unwrap();
    assert_eq!(value["blocked"], false);
}

#[test]
fn continuation_waits_for_ack_and_revalidates_candidate_before_new_stage() {
    let mut app = app();
    live(&mut app, Phase::Interrupted);
    let root = std::env::temp_dir().join(format!("pui-continuation-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("source"), "candidate").unwrap();
    let fingerprint = domain::workspace::fingerprint(&root).unwrap();
    let t = app.tasks.live_mut().unwrap();
    t.initial_approved = true;
    t.workspace = Some(root);
    t.attempts[0].fingerprint = Some(fingerprint);
    app.tasks.live = None;
    let (store, commands) = mpsc::channel();
    app.tasks.store = Some(store);
    let expected = app.tasks.current().unwrap().clone();
    app.task_confirm_continuation(expected, domain::continuation::Boundary::Checks)
        .unwrap();
    assert!(app.tasks.job.is_none() && app.tasks.processes.is_empty());
    let StoreCommand::SaveAcknowledged { reply, .. } =
        commands.recv_timeout(Duration::from_secs(2)).unwrap()
    else {
        panic!("continuation intent");
    };
    reply.send(Ok(())).unwrap();
    app.poll_task_continuation();
    let job = app.tasks.job.take().unwrap();
    app.task_job_result(job.rx.recv_timeout(Duration::from_secs(10)).unwrap());
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Checking);
    assert_eq!(app.tasks.current().unwrap().continuations.len(), 1);
    assert!(
        app.tasks.processes.is_empty(),
        "preflight still precedes commands"
    );
    app.tasks.stop();
}

#[test]
fn failed_continuation_never_launches_repairs() {
    let mut app = app();
    live(&mut app, Phase::Blocked);
    let t = app.tasks.live_mut().unwrap();
    t.initial_approved = true;
    t.attempts[0].fingerprint = Some("candidate".into());
    app.tasks.live = None;
    let (store, commands) = mpsc::channel();
    app.tasks.store = Some(store);
    app.task_confirm_continuation(
        app.tasks.current().unwrap().clone(),
        domain::continuation::Boundary::Checks,
    )
    .unwrap();
    let StoreCommand::SaveAcknowledged { reply, .. } =
        commands.recv_timeout(Duration::from_secs(2)).unwrap()
    else {
        panic!("receipt")
    };
    reply.send(Ok(())).unwrap();
    app.poll_task_continuation();
    app.tasks.job = None;
    let t = app.tasks.live_mut().unwrap();
    t.attempts[0].checks.push(Check {
        criterion: "C1".into(),
        command: contract().criteria[0].command.clone(),
        exit_code: Some(1),
        output: "failed".into(),
        truncated: false,
        cancelled: false,
        duration_ms: 1,
    });
    t.attempts[0].review = Some(Review {
        summary: "Failed".into(),
        criteria: vec![domain::Finding {
            id: "C1".into(),
            status: domain::Verdict::Fail,
            evidence: "failed test".into(),
        }],
    });
    app.task_finish_attempt();
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Failed);
    assert_eq!(
        app.tasks.current().unwrap().reason,
        Some(domain::decision::Reason::VerificationFailed)
    );
    assert_eq!(app.tasks.current().unwrap().attempts.len(), 1);
    assert!(app.tasks.job.is_none() && app.tasks.processes.is_empty());
}

#[test]
fn continuation_authority_change_after_ack_blocks_next_stage() {
    let mut app = app();
    live(&mut app, Phase::Interrupted);
    let t = app.tasks.live_mut().unwrap();
    t.initial_approved = true;
    t.attempts[0].fingerprint = Some("candidate".into());
    app.tasks.live = None;
    let (store, commands) = mpsc::channel();
    app.tasks.store = Some(store);
    app.task_confirm_continuation(
        app.tasks.current().unwrap().clone(),
        domain::continuation::Boundary::Checks,
    )
    .unwrap();
    let StoreCommand::SaveAcknowledged { reply, .. } =
        commands.recv_timeout(Duration::from_secs(2)).unwrap()
    else {
        panic!("receipt")
    };
    reply.send(Ok(())).unwrap();
    app.poll_task_continuation();
    app.tasks.job = None;
    app.prefs.settings.permission = settings::PermissionMode::Plan;
    app.task_continue_fingerprinted("candidate");
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Blocked);
    assert!(app.tasks.processes.is_empty());
}

#[test]
fn cancelling_pending_continuation_waits_for_ack_then_reconciles_without_execution() {
    let mut app = app();
    live(&mut app, Phase::Interrupted);
    let t = app.tasks.live_mut().unwrap();
    t.initial_approved = true;
    t.attempts[0].fingerprint = Some("candidate".into());
    app.tasks.live = None;
    let (store, commands) = mpsc::channel();
    app.tasks.store = Some(store);
    app.task_confirm_continuation(
        app.tasks.current().unwrap().clone(),
        domain::continuation::Boundary::Checks,
    )
    .unwrap();
    let StoreCommand::SaveAcknowledged { reply, .. } =
        commands.recv_timeout(Duration::from_secs(2)).unwrap()
    else {
        panic!("receipt")
    };
    app.tasks.stop();
    assert!(app.tasks.continuation.pending.is_some());
    reply.send(Ok(())).unwrap();
    app.poll_task_continuation();
    assert!(app.tasks.live.is_none() && app.tasks.job.is_none() && app.tasks.processes.is_empty());
    assert!(app.task_recovery_active());
    assert!(
        commands
            .try_iter()
            .all(|c| !matches!(c, StoreCommand::Save(_)))
    );
}

#[test]
fn failed_or_lost_continuation_ack_reconciles_without_blind_save() {
    for lost in [false, true] {
        let mut app = app();
        live(&mut app, Phase::Interrupted);
        let t = app.tasks.live_mut().unwrap();
        t.initial_approved = true;
        t.attempts[0].fingerprint = Some("candidate".into());
        app.tasks.live = None;
        let (store, commands) = mpsc::channel();
        app.tasks.store = Some(store);
        app.task_confirm_continuation(
            app.tasks.current().unwrap().clone(),
            domain::continuation::Boundary::Checks,
        )
        .unwrap();
        let StoreCommand::SaveAcknowledged { reply, .. } =
            commands.recv_timeout(Duration::from_secs(2)).unwrap()
        else {
            panic!("receipt")
        };
        if lost {
            drop(reply);
        } else {
            reply.send(Err("drift".into())).unwrap();
        }
        app.poll_task_continuation();
        assert!(app.tasks.job.is_none() && app.tasks.processes.is_empty());
        assert!(
            commands
                .try_iter()
                .all(|c| !matches!(c, StoreCommand::Save(_)))
        );
        assert!(app.task_recovery_active());
    }
}

#[test]
fn focused_check_uses_frozen_command_and_keeps_final_acceptance_empty() {
    let mut app = app();
    live(&mut app, Phase::Building);
    process(&mut app, Role::Builder);
    let root = std::env::temp_dir().join(format!("pui-feedback-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("source.txt"), "candidate").unwrap();
    app.tasks.live_mut().unwrap().workspace = Some(root);
    let (client, _, replies) = rpc::Client::stub_with_records();
    app.tasks.processes[0].client = client;
    let request = json!({"type":"extension_ui_request","method":"input","title":"Pui workflow check","id":"feedback","placeholder":json!({"criterion":"C1"}).to_string()});
    assert!(app.task_feedback_request(0, &request));
    let job = app.tasks.job.take().unwrap();
    app.task_job_result(job.rx.recv_timeout(Duration::from_secs(10)).unwrap());
    assert_eq!(
        app.tasks.processes.len(),
        2,
        "builder remains alive while check runs"
    );
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.tasks.processes[1].client = client;
    let id = app.tasks.processes[1].pending.as_ref().unwrap().id.clone();
    app.task_event(1,rpc::Event::Record(json!({"type":"response","command":"get_state","id":id,"success":true,"data":{"model":{"id":"fixture-model","provider":"fixture-provider"},"thinkingLevel":"high"}})));
    assert_eq!(
        commands.recv_timeout(Duration::from_secs(2)).unwrap()["command"],
        contract().criteria[0].command
    );
    let id = app.tasks.processes[1].pending.as_ref().unwrap().id.clone();
    app.task_event(1,rpc::Event::Record(json!({"type":"response","command":"bash","id":id,"success":true,"data":{"exitCode":1,"output":"compile defect","cancelled":false}})));
    let job = app.tasks.job.take().unwrap();
    app.task_job_result(job.rx.recv_timeout(Duration::from_secs(10)).unwrap());
    let result = replies.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(result["id"], "feedback");
    assert!(result["value"].as_str().unwrap().contains("compile defect"));
    assert_eq!(app.tasks.live_task().unwrap().feedback.len(), 1);
    assert!(
        app.tasks
            .live_task()
            .unwrap()
            .latest()
            .unwrap()
            .checks
            .is_empty()
    );
    assert_eq!(app.tasks.live_task().unwrap().phase, Phase::Building);
    assert_eq!(app.tasks.processes.len(), 1);
    app.tasks.stop();
}

#[test]
fn failed_or_lost_ack_reconciles_without_dispatch_or_blind_stale_save() {
    for lost in [false, true] {
        let (mut app, commands, sent) = staged();
        let StoreCommand::SaveAcknowledged { reply, .. } =
            commands.recv_timeout(Duration::from_secs(2)).unwrap()
        else {
            panic!("receipt");
        };
        if lost {
            drop(reply);
        } else {
            reply.send(Err("record drift".into())).unwrap();
        }
        app.task_poll_processes();
        assert!(sent.try_iter().all(|v| v["type"] != "bash"));
        assert!(app.tasks.live.is_none());
        assert_eq!(app.tasks.current().unwrap().phase, Phase::Interrupted);
        assert!(
            commands
                .try_iter()
                .all(|c| !matches!(c, StoreCommand::Save(_)))
        );
    }
}
