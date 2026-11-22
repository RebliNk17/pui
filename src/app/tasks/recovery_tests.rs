use super::*;
use std::{fs, path::Path, process::Command};

fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!("pui-recovery-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join(".gitignore"), "node_modules/\n").unwrap();
    fs::write(root.join("candidate.txt"), "retained unchanged candidate\n").unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "-qm",
            "fixture",
        ],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
    }
    root
}
fn app(root: &Path) -> (Pui, Receiver<StoreCommand>) {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(egui::Context::default()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.state.provider = "fixture-provider".into();
    app.state.model = "claude-haiku-5-5".into();
    app.state.thinking = "high".into();
    app.state.models = vec![crate::protocol::Model {
        id: app.state.model.clone(),
        name: app.state.model.clone(),
        provider: app.state.provider.clone(),
        context_window: 600000,
        input_cost: 0.1,
        output_cost: 0.5,
    }];
    let owner = app.ensure_agent();
    let mut task = Task::new(
        root.into(),
        app.state.provider.clone(),
        app.state.model.clone(),
        "high".into(),
    );
    task.initial_approved = true;
    task.objective = "Recover only the retained candidate".into();
    task.workspace = Some(root.into());
    task.phase = Phase::Blocked;
    task.spend_limit = Some(5.0);
    task.approved = Some(Contract {
        criteria: vec![domain::Criterion {
            id: "C1".into(),
            description: "Fixture check".into(),
            command: "printf recovered".into(),
        }],
    });
    let mut attempt = Attempt::new(2);
    attempt.fingerprint = Some(domain::workspace::fingerprint(root).unwrap());
    attempt.note = "Original failure must remain".into();
    attempt.checks.push(Check {
        criterion: "C1".into(),
        command: "printf recovered".into(),
        exit_code: Some(1),
        output: "original failure".into(),
        truncated: false,
        cancelled: false,
        duration_ms: 1,
    });
    task.attempts.push(attempt);
    app.tasks.parents.insert(task.id.clone(), owner);
    app.tasks.selected = Some(task.id.clone());
    app.tasks.records = vec![task];
    let (store, commands) = mpsc::channel();
    app.tasks.store = Some(store);
    (app, commands)
}
fn claim_and_ack(app: &mut Pui, commands: &Receiver<StoreCommand>) {
    let StoreCommand::ClaimCurrent { reply, .. } =
        commands.recv_timeout(Duration::from_secs(3)).unwrap()
    else {
        panic!("expected authoritative claim")
    };
    reply.send(Ok(())).unwrap();
    app.task_poll_recovery();
    assert!(app.tasks.job.is_none(), "no work before durable consent");
    assert!(app.tasks.processes.is_empty());
    let StoreCommand::SaveAcknowledged {
        expected,
        task,
        reply,
    } = commands.recv_timeout(Duration::from_secs(3)).unwrap()
    else {
        panic!("expected acknowledged save")
    };
    assert!(expected.recoveries.is_empty());
    assert_eq!(task.recoveries.len(), 1);
    reply.send(Ok(())).unwrap();
    app.task_poll_recovery();
}
fn job(app: &mut Pui) {
    let job = app.tasks.job.take().expect("bounded read-only job");
    app.task_job_result(job.rx.recv_timeout(Duration::from_secs(10)).unwrap());
}
fn init(app: &mut Pui, commands: &Receiver<StoreCommand>) -> Receiver<Value> {
    let (client, _, sent) = rpc::Client::stub_with_records();
    app.tasks.processes.last_mut().unwrap().client = client;
    let index = app.tasks.processes.len() - 1;
    let id = app.tasks.processes[index]
        .pending
        .as_ref()
        .unwrap()
        .id
        .clone();
    app.task_event(index, rpc::Event::Record(json!({"type":"response","command":"get_state","id":id,"success":true,"data":{"model":{"id":"claude-haiku-5-5","provider":"fixture-provider"},"thinkingLevel":"high"}})));
    assert!(
        sent.try_recv().is_err(),
        "no effect before durable dispatch acknowledgment"
    );
    loop {
        if let StoreCommand::SaveAcknowledged { task, reply, .. } =
            commands.recv_timeout(Duration::from_secs(3)).unwrap()
        {
            assert_eq!(
                task.operations.last().unwrap().state,
                domain::engine::State::Dispatching
            );
            reply.send(Ok(())).unwrap();
            break;
        }
    }
    app.task_poll_processes();
    sent
}
fn check(app: &mut Pui, data: Value) {
    let index = app.tasks.processes.len() - 1;
    let id = app.tasks.processes[index]
        .pending
        .as_ref()
        .unwrap()
        .id
        .clone();
    app.task_event(
        index,
        rpc::Event::Record(
            json!({"type":"response","command":"bash","id":id,"success":true,"data":data}),
        ),
    );
}
#[test]
fn request_is_pure_scoped_and_fails_closed_for_unknown_legacy_stale_and_modes() {
    let root = fixture();
    let (mut app, commands) = app(&root);
    let id = app.tasks.selected.clone().unwrap();
    let before = record(app.tasks.current().unwrap()).unwrap();
    let request = app.task_recovery_request(&id).unwrap();
    assert!(request.detail.contains("ONE NEW PAID"));
    assert!(request.detail.contains("printf recovered"));
    assert!(request.detail.contains("No router"));
    assert!(request.detail.contains("Some(5.0)"));
    assert_eq!(record(app.tasks.current().unwrap()).unwrap(), before);
    assert!(commands.try_recv().is_err());
    assert!(app.task_recovery_request("unknown").is_err());
    for phase in [
        Phase::Building,
        Phase::Stale,
        Phase::Draft,
        Phase::AwaitingApproval,
    ] {
        app.tasks.current_mut().unwrap().phase = phase;
        assert!(app.task_recovery_request(&id).is_err());
    }
    app.tasks.current_mut().unwrap().phase = Phase::Blocked;
    app.tasks.current_mut().unwrap().initial_approved = false;
    assert!(app.task_recovery_request(&id).is_err());
    app.tasks.current_mut().unwrap().initial_approved = true;
    for mode in [
        settings::PermissionMode::Plan,
        settings::PermissionMode::DontAsk,
    ] {
        app.prefs.settings.permission = mode;
        assert!(app.task_recovery_request(&id).is_err());
    }
}
#[test]
fn cancelled_confirmation_and_pending_claim_execute_nothing() {
    let root = fixture();
    let (mut app, commands) = app(&root);
    let id = app.tasks.selected.clone().unwrap();
    drop(app.task_recovery_request(&id).unwrap()); // cancelling the UI is not approval
    assert!(commands.try_recv().is_err());
    app.task_confirm_check_rerun(app.task_recovery_request(&id).unwrap())
        .unwrap();
    app.tasks.stop();
    let StoreCommand::ClaimCurrent { reply, .. } =
        commands.recv_timeout(Duration::from_secs(3)).unwrap()
    else {
        panic!()
    };
    assert!(reply.send(Ok(())).is_err());
    app.task_poll_recovery();
    assert!(app.tasks.current().unwrap().recoveries.is_empty());
    assert!(app.tasks.processes.is_empty());
    assert!(app.tasks.job.is_none());
}
#[test]
fn full_record_drift_before_and_during_claim_and_failed_durable_save_execute_nothing() {
    for point in 0..3 {
        let root = fixture();
        let (mut app, commands) = app(&root);
        let id = app.tasks.selected.clone().unwrap();
        let request = app.task_recovery_request(&id).unwrap();
        if point == 0 {
            app.tasks
                .current_mut()
                .unwrap()
                .approved
                .as_mut()
                .unwrap()
                .criteria[0]
                .command = "changed command".into();
            assert!(app.task_confirm_check_rerun(request).is_err());
            assert!(commands.try_recv().is_err());
        } else {
            app.task_confirm_check_rerun(request).unwrap();
            let StoreCommand::ClaimCurrent { reply, .. } =
                commands.recv_timeout(Duration::from_secs(3)).unwrap()
            else {
                panic!()
            };
            if point == 1 {
                app.tasks.current_mut().unwrap().spend_limit = Some(500.0);
            }
            reply.send(Ok(())).unwrap();
            app.task_poll_recovery();
            if point == 2 {
                let StoreCommand::SaveAcknowledged { reply, .. } =
                    commands.recv_timeout(Duration::from_secs(3)).unwrap()
                else {
                    panic!()
                };
                reply.send(Err("record changed".into())).unwrap();
                app.task_poll_recovery();
            }
        }
        assert!(app.tasks.processes.is_empty());
        assert!(app.tasks.job.is_none());
        assert!(
            commands
                .try_iter()
                .all(|c| !matches!(c, StoreCommand::Save(_)))
        );
    }
}
#[test]
fn unchanged_ignored_setup_allows_explicit_rechecks_and_fresh_verifier_at_repair_limit() {
    let root = fixture();
    let (mut app, commands) = app(&root);
    let id = app.tasks.selected.clone().unwrap();
    let old_attempts = serde_json::to_value(&app.tasks.current().unwrap().attempts).unwrap();
    let request = app.task_recovery_request(&id).unwrap();
    fs::create_dir(root.join("node_modules")).unwrap();
    fs::write(
        root.join("node_modules/dependency"),
        "authorized ignored setup",
    )
    .unwrap();
    app.task_confirm_check_rerun(request).unwrap();
    claim_and_ack(&mut app, &commands);
    job(&mut app); // fingerprint before checks
    job(&mut app); // environment inspection
    assert_eq!(app.tasks.processes.len(), 1);
    assert_eq!(app.tasks.processes[0].role, Role::Checks);
    let sent = init(&mut app, &commands);
    assert_eq!(
        sent.recv_timeout(Duration::from_secs(2)).unwrap()["command"],
        "printf recovered"
    );
    check(
        &mut app,
        json!({"exitCode":0,"output":"recovered","cancelled":false}),
    );
    job(&mut app); // fingerprint after checks, then fresh read-only verifier
    assert_eq!(app.tasks.processes.last().unwrap().role, Role::Verifier);
    assert!(
        app.tasks
            .processes
            .last()
            .unwrap()
            .state
            .messages
            .is_empty()
    );
    let sent = init(&mut app, &commands);
    let prompt = sent.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(prompt["message"].as_str().unwrap().contains("recovered"));
    assert!(
        !prompt["message"]
            .as_str()
            .unwrap()
            .contains("original failure")
    );
    let execution = app.task_recovery_execution_mut().unwrap();
    execution.review = Some(Review {
        summary: "Fresh independent evidence".into(),
        criteria: vec![domain::Finding {
            id: "C1".into(),
            status: Verdict::Pass,
            evidence: "candidate.txt and separately executed check".into(),
        }],
    });
    let fingerprint = execution.fingerprint.clone();
    app.task_recovery_fingerprinted(FingerprintStep::RecoveryAfterReview, &fingerprint);
    let task = app.tasks.current().unwrap();
    assert_eq!(task.phase, Phase::Ready);
    assert_eq!(task.recoveries.len(), 1);
    assert!(task.recoveries[0].passed(task.approved.as_ref().unwrap()));
    assert_eq!(task.latest().unwrap().number, 2);
    assert_eq!(serde_json::to_value(&task.attempts).unwrap(), old_attempts);
    assert_eq!(task.runs.len(), 2);
    assert!(
        task.runs
            .iter()
            .all(|r| matches!(r.role, Role::Checks | Role::Verifier))
    );
}
#[test]
fn uncertain_check_and_candidate_drift_never_replay_launch_verifier_or_repair() {
    for point in 0..4 {
        let root = fixture();
        let (mut app, commands) = app(&root);
        let id = app.tasks.selected.clone().unwrap();
        app.task_confirm_check_rerun(app.task_recovery_request(&id).unwrap())
            .unwrap();
        claim_and_ack(&mut app, &commands);
        if point == 0 {
            fs::write(root.join("candidate.txt"), "drift before").unwrap();
        }
        job(&mut app);
        if point == 0 {
            assert_eq!(app.tasks.current().unwrap().phase, Phase::Stale);
        } else {
            job(&mut app);
            let _sent = init(&mut app, &commands);
            if point == 1 {
                check(&mut app, json!({"output":"uncertain","cancelled":false}));
                assert_eq!(app.tasks.current().unwrap().phase, Phase::Blocked);
                assert_eq!(
                    app.tasks.current().unwrap().recoveries[0].state,
                    RecoveryState::Blocked
                );
            } else {
                if point == 2 {
                    fs::write(root.join("candidate.txt"), "drift during checks").unwrap();
                }
                check(
                    &mut app,
                    json!({"exitCode":0,"output":"done","cancelled":false}),
                );
                job(&mut app);
                if point == 2 {
                    assert_eq!(app.tasks.current().unwrap().phase, Phase::Stale);
                } else {
                    app.tasks.stop();
                    assert_eq!(
                        app.tasks.current().unwrap().recoveries[0].state,
                        RecoveryState::Cancelled
                    );
                }
            }
        }
        let task = app.tasks.current().unwrap();
        assert_eq!(task.attempts.len(), 1);
        assert_eq!(task.latest().unwrap().number, 2);
        assert_eq!(task.latest().unwrap().checks[0].output, "original failure");
        assert!(task.runs.iter().all(|r| r.role != Role::Builder));
        assert!(app.tasks.live.is_none());
        assert!(app.tasks.job.is_none());
    }
}
#[test]
fn real_authoritative_record_drift_is_rejected_before_mutation() {
    let root = fixture();
    let home = root.join(".private-store");
    let (mut app, _commands) = app(&root);
    let id = app.tasks.selected.clone().unwrap();
    domain::store::save(&home, app.tasks.current().unwrap()).unwrap();
    let request = app.task_recovery_request(&id).unwrap();
    let mut changed = app.tasks.current().unwrap().clone();
    changed.constraints = "same timestamp but authoritative change".into();
    domain::store::save(&home, &changed).unwrap();
    let before = fs::read(home.join(format!("{id}.json"))).unwrap();
    let (store, rx) = mpsc::channel();
    let (errors, _) = mpsc::channel();
    app.tasks.store = Some(store);
    app.task_confirm_check_rerun(request).unwrap();
    // Process exactly the pending claim with the real production store implementation.
    let command = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let (tx, worker_rx) = mpsc::channel();
    tx.send(command).unwrap();
    drop(tx);
    run_store(&home, worker_rx, errors, &app.ctx);
    app.task_poll_recovery();
    assert!(app.tasks.current().unwrap().recoveries.is_empty());
    assert!(app.tasks.job.is_none());
    assert!(app.tasks.processes.is_empty());
    assert_eq!(fs::read(home.join(format!("{id}.json"))).unwrap(), before);
}

fn initial_fixture() -> (Pui, Receiver<StoreCommand>, PathBuf) {
    let source = fixture();
    let (mut app, commands) = app(&source);
    let home = std::env::temp_dir().join(format!("pui-initial-proof-{}", uuid::Uuid::new_v4()));
    let task = app.tasks.current().unwrap();
    let head = domain::workspace::inspect(&source).unwrap().head;
    let (candidate, isolated) =
        domain::workspace::prepare(&source, &task.id, &home.join("worktrees"), &head).unwrap();
    assert!(isolated);
    fs::create_dir_all(home.join("store")).unwrap();
    let receipt =
        domain::apply::capture_receipt(&source, &candidate, &head, &task.id, &home.join("store"))
            .unwrap();
    let task = app.tasks.current_mut().unwrap();
    task.workspace = Some(candidate.clone());
    task.isolated = true;
    task.preparation_receipt = Some(receipt);
    task.prepared_fingerprint = Some(domain::workspace::fingerprint(&candidate).unwrap());
    task.implementation_pending = true;
    task.attempts.clear();
    (app, commands, candidate)
}
fn initial_claim_ack(app: &mut Pui, commands: &Receiver<StoreCommand>) {
    let StoreCommand::ClaimCurrent { reply, .. } =
        commands.recv_timeout(Duration::from_secs(3)).unwrap()
    else {
        panic!()
    };
    reply.send(Ok(())).unwrap();
    app.task_poll_recovery();
    let StoreCommand::SaveAcknowledged {
        expected,
        task,
        reply,
    } = commands.recv_timeout(Duration::from_secs(3)).unwrap()
    else {
        panic!()
    };
    assert!(expected.attempts.is_empty());
    assert!(task.attempts.is_empty());
    assert!(task.recoveries.is_empty());
    assert!(task.note.contains("NEW approval"));
    assert!(app.tasks.processes.is_empty());
    assert!(app.tasks.job.is_none());
    reply.send(Ok(())).unwrap();
    app.task_poll_recovery();
}
#[test]
fn explicit_initial_resume_only_when_builder_effects_provably_never_started() {
    let (mut app, commands, candidate) = initial_fixture();
    let id = app.tasks.selected.clone().unwrap();
    let request = app.task_initial_resume_request(&id).unwrap();
    assert!(request.detail.contains("NEW approval"));
    assert!(request.detail.contains("two repair rounds"));
    fs::create_dir(candidate.join("node_modules")).unwrap();
    fs::write(
        candidate.join("node_modules/ready"),
        "ignored explicit setup",
    )
    .unwrap();
    app.task_confirm_initial_resume(request).unwrap();
    initial_claim_ack(&mut app, &commands);
    job(&mut app);
    job(&mut app);
    assert!(
        app.tasks.processes.is_empty(),
        "builder cannot precede durable no-resume marker"
    );
    let StoreCommand::SaveAcknowledged { task, reply, .. } =
        commands.recv_timeout(Duration::from_secs(3)).unwrap()
    else {
        panic!()
    };
    assert!(!task.implementation_pending);
    assert_eq!(task.attempts.len(), 1);
    reply.send(Ok(())).unwrap();
    app.task_poll_recovery();
    let task = app.tasks.current().unwrap();
    assert_eq!(task.phase, Phase::Building);
    assert!(!task.implementation_pending);
    assert_eq!(task.attempts.len(), 1);
    assert_eq!(task.latest().unwrap().number, 0);
    assert!(task.recoveries.is_empty());
    assert_eq!(task.runs.len(), 1);
    assert_eq!(task.runs[0].role, Role::Builder);
    assert!(!app.task_recovery_active());
    app.tasks.stop();
    assert!(
        app.task_initial_resume_request(&id).is_err(),
        "never resume after builder may begin"
    );
}
#[test]
fn initial_resume_rejects_preparation_drift_and_missing_proof() {
    for point in 0..3 {
        let (mut app, commands, candidate) = initial_fixture();
        let id = app.tasks.selected.clone().unwrap();
        if point < 2 {
            let task = app.tasks.current_mut().unwrap();
            if point == 0 {
                task.prepared_fingerprint = None;
            } else {
                task.attempts.push(Attempt::new(0));
            }
            assert!(app.task_initial_resume_request(&id).is_err());
        } else {
            let request = app.task_initial_resume_request(&id).unwrap();
            fs::write(candidate.join("candidate.txt"), "changed after setup").unwrap();
            app.task_confirm_initial_resume(request).unwrap();
            initial_claim_ack(&mut app, &commands);
            job(&mut app);
            assert!(app.tasks.current().unwrap().attempts.is_empty());
            assert!(app.tasks.live.is_none());
        }
        assert!(app.tasks.processes.is_empty());
    }
}
#[test]
fn latest_failed_recovery_supersedes_older_pass_and_postreview_drift_is_stale() {
    for drift in [false, true] {
        let root = fixture();
        let (mut app, commands) = app(&root);
        let id = app.tasks.selected.clone().unwrap();
        app.task_confirm_check_rerun(app.task_recovery_request(&id).unwrap())
            .unwrap();
        claim_and_ack(&mut app, &commands);
        job(&mut app);
        job(&mut app);
        let _ = init(&mut app, &commands);
        check(
            &mut app,
            json!({"exitCode":0,"output":"done","cancelled":false}),
        );
        job(&mut app);
        let execution = app.task_recovery_execution_mut().unwrap();
        execution.review = Some(Review {
            summary: "Independent failure".into(),
            criteria: vec![domain::Finding {
                id: "C1".into(),
                status: Verdict::Fail,
                evidence: "missing requested behavior".into(),
            }],
        });
        let fingerprint = execution.fingerprint.clone();
        app.task_recovery_fingerprinted(
            FingerprintStep::RecoveryAfterReview,
            if drift {
                "drift-after-review"
            } else {
                &fingerprint
            },
        );
        let task = app.tasks.current().unwrap();
        assert_eq!(task.phase, if drift { Phase::Stale } else { Phase::Failed });
        if !drift {
            assert_eq!(
                task.reason,
                Some(domain::decision::Reason::VerificationFailed)
            );
        }
        assert!(task.current_evidence().unwrap().invalidated);
        assert!(
            !task
                .current_evidence()
                .unwrap()
                .passed(task.approved.as_ref().unwrap())
        );
        assert_eq!(task.attempts.len(), 1);
        assert_eq!(task.latest().unwrap().number, 2);
        assert!(app.tasks.live.is_none());
        assert!(app.tasks.job.is_none());
    }
}

#[test]
fn initial_builder_start_is_never_sent_without_ack_after_cancel_or_record_drift() {
    for point in 0..4 {
        let (mut app, commands, _) = initial_fixture();
        let id = app.tasks.selected.clone().unwrap();
        app.task_confirm_initial_resume(app.task_initial_resume_request(&id).unwrap())
            .unwrap();
        initial_claim_ack(&mut app, &commands);
        job(&mut app);
        job(&mut app);
        let StoreCommand::SaveAcknowledged { task, reply, .. } =
            commands.recv_timeout(Duration::from_secs(3)).unwrap()
        else {
            panic!()
        };
        assert!(app.tasks.processes.is_empty());
        let mut crashed = (*task).clone();
        crashed.recover();
        assert!(!crashed.implementation_pending);
        assert_eq!(
            crashed.attempts.len(),
            1,
            "persisted start conservatively forbids resume after crash"
        );
        match point {
            0 => {
                drop(reply);
            }
            1 => {
                app.tasks.stop();
                assert!(reply.send(Ok(())).is_err());
            }
            2 => {
                app.tasks.current_mut().unwrap().objective = "drift before paid launch".into();
                reply.send(Ok(())).unwrap();
            }
            _ => {
                app.prefs.settings.permission = settings::PermissionMode::Plan;
                reply.send(Ok(())).unwrap();
            }
        }
        app.task_poll_recovery();
        assert!(app.tasks.processes.is_empty());
        assert!(app.tasks.live.is_none());
        assert!(app.task_initial_resume_request(&id).is_err());
    }
}

#[test]
fn recovery_pending_check_approval_cannot_send_answer_or_settings_after_authority_drift() {
    for owner_drift in [false, true] {
        for settings_update in [false, true] {
            let root = fixture();
            let (mut app, commands) = app(&root);
            let id = app.tasks.selected.clone().unwrap();
            app.task_confirm_check_rerun(app.task_recovery_request(&id).unwrap())
                .unwrap();
            claim_and_ack(&mut app, &commands);
            job(&mut app);
            job(&mut app);
            let sent = init(&mut app, &commands);
            assert_eq!(
                sent.recv_timeout(Duration::from_secs(2)).unwrap()["command"],
                "printf recovered"
            );
            app.tasks.open = true;
            let run = app.tasks.processes.last().unwrap().run.clone();
            app.tasks.processes.last_mut().unwrap().state.dialogs.push_back(json!({
                "type":"extension_ui_request", "method":"select", "id":"recovery-exact-check-permission",
                "title":"Allow this exact command?", "options":["Allow once", "Deny"]
            }));
            assert!(
                sent.try_recv().is_err(),
                "permission question is never auto-answered"
            );
            if owner_drift {
                app.agents.active = "different-owning-chat".into();
            } else {
                app.prefs.settings.permission = settings::PermissionMode::Plan;
            }
            if settings_update {
                app.task_apply_settings();
            } else {
                app.task_answer(
                    &run,
                    "recovery-exact-check-permission",
                    json!({"value":"Allow once"}),
                );
            }
            let task = app.tasks.records.iter().find(|t| t.id == id).unwrap();
            assert_eq!(task.phase, Phase::Blocked);
            assert_eq!(
                task.recoveries.last().unwrap().state,
                RecoveryState::Blocked
            );
            assert!(task.recoveries.last().unwrap().checks.is_empty());
            assert!(app.tasks.live.is_none());
            assert!(app.tasks.processes.is_empty());
            assert!(app.tasks.job.is_none());
            for record in sent.try_iter() {
                assert_ne!(
                    record["type"], "extension_ui_response",
                    "stale scope cannot answer permission"
                );
                assert_ne!(
                    record["type"], "prompt",
                    "stale scope cannot apply settings through a prompt"
                );
                assert_ne!(
                    record["type"], "bash",
                    "stale scope cannot submit another command"
                );
            }
            assert!(task.runs.iter().all(|run| run.role == Role::Checks));
        }
    }
}

#[test]
fn failed_consent_ack_reconciles_authoritative_record_and_allows_fresh_confirmation() {
    let root = fixture();
    let home =
        std::env::temp_dir().join(format!("pui-recovery-reconcile-{}", uuid::Uuid::new_v4()));
    let (mut app, commands) = app(&root);
    let original = app.tasks.current().unwrap().clone();
    let id = original.id.clone();
    domain::store::save(&home, &original).unwrap();
    let (work, worker_rx) = mpsc::channel();
    let (errors, _) = mpsc::channel();
    let worker_home = home.clone();
    let worker = std::thread::spawn(move || {
        run_store(&worker_home, worker_rx, errors, &egui::Context::default())
    });
    let barrier = || {
        let (reply, rx) = mpsc::channel();
        work.send(StoreCommand::Refresh { reply }).unwrap();
        rx.recv_timeout(Duration::from_secs(3)).unwrap().unwrap()
    };
    app.task_confirm_check_rerun(app.task_recovery_request(&id).unwrap())
        .unwrap();
    work.send(commands.recv_timeout(Duration::from_secs(3)).unwrap())
        .unwrap();
    barrier();
    app.task_poll_recovery();
    let command = commands.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(matches!(command, StoreCommand::SaveAcknowledged { .. }));
    let mut authoritative = original.clone();
    authoritative.constraints = "authoritative same-timestamp drift".into();
    domain::store::save(&home, &authoritative).unwrap();
    let disk_before = fs::read(home.join(format!("{id}.json"))).unwrap();
    work.send(command).unwrap();
    barrier();
    app.task_poll_recovery();
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Interrupted);
    assert!(
        app.tasks.current().unwrap().recoveries.is_empty(),
        "definite rejection restores exact old evidence before quarantine"
    );
    assert!(
        app.task_recovery_request(&id).is_err(),
        "reconciliation must finish before another approval"
    );
    assert!(app.tasks.processes.is_empty());
    assert!(app.tasks.job.is_none());
    assert!(app.tasks.live.is_none());
    assert!(
        domain::store::lock(&home, &id).is_err(),
        "claim remains held until explicit release"
    );
    let release = commands.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(matches!(release, StoreCommand::ReleaseCurrent { .. }));
    work.send(release).unwrap();
    barrier();
    app.task_poll_recovery();
    assert_eq!(
        record(app.tasks.current().unwrap()).unwrap(),
        record(&authoritative).unwrap()
    );
    assert!(!app.task_recovery_active());
    assert!(
        app.task_recovery_request(&id).is_ok(),
        "fresh scoped consent is available without restart"
    );
    assert!(
        domain::store::lock(&home, &id).is_ok(),
        "failed transition must not retain writer ownership"
    );
    assert_eq!(
        fs::read(home.join(format!("{id}.json"))).unwrap(),
        disk_before,
        "no blind Save overwrites authoritative drift"
    );
    assert!(commands.try_recv().is_err());
    drop(work);
    worker.join().unwrap();
}
#[test]
fn disconnected_consent_ack_stays_nonactive_and_quarantined_until_readonly_reconciliation() {
    let root = fixture();
    let (mut app, commands) = app(&root);
    let original = app.tasks.current().unwrap().clone();
    let id = original.id.clone();
    app.task_confirm_check_rerun(app.task_recovery_request(&id).unwrap())
        .unwrap();
    let StoreCommand::ClaimCurrent { reply, .. } =
        commands.recv_timeout(Duration::from_secs(3)).unwrap()
    else {
        panic!()
    };
    reply.send(Ok(())).unwrap();
    app.task_poll_recovery();
    let StoreCommand::SaveAcknowledged { reply, .. } =
        commands.recv_timeout(Duration::from_secs(3)).unwrap()
    else {
        panic!()
    };
    drop(reply);
    app.task_poll_recovery();
    let task = app.tasks.current().unwrap();
    assert_eq!(task.phase, Phase::Interrupted);
    assert_eq!(
        task.recoveries.last().unwrap().state,
        RecoveryState::Blocked,
        "unknown acknowledgment cannot erase possibly durable consent"
    );
    assert!(app.tasks.live.is_none());
    assert!(app.tasks.job.is_none());
    assert!(app.tasks.processes.is_empty());
    let StoreCommand::ReleaseCurrent { reply, .. } =
        commands.recv_timeout(Duration::from_secs(3)).unwrap()
    else {
        panic!()
    };
    drop(reply);
    app.task_poll_recovery();
    assert!(app.task_recovery_active());
    assert!(app.task_recovery_request(&id).is_err());
    assert!(app.task_retry_recovery_reconciliation(&id));
    let StoreCommand::ReleaseCurrent { reply, .. } =
        commands.recv_timeout(Duration::from_secs(3)).unwrap()
    else {
        panic!()
    };
    reply
        .send(Ok(refresh::Snapshot {
            warning: None,
            records: vec![original.clone()],
            owned: Default::default(),
        }))
        .unwrap();
    app.task_poll_recovery();
    assert_eq!(
        record(app.tasks.current().unwrap()).unwrap(),
        record(&original).unwrap()
    );
    assert!(app.task_recovery_request(&id).is_ok());
    assert!(commands.try_iter().all(|command| !matches!(
        command,
        StoreCommand::Save(_) | StoreCommand::SaveAcknowledged { .. }
    )));
}
#[test]
fn preparation_and_builder_ack_failure_are_nonactive_and_do_not_restart_paid_work() {
    for builder in [false, true] {
        let (mut app, commands, _) = initial_fixture();
        let before = app.tasks.current().unwrap().clone();
        let id = before.id.clone();
        app.tasks.live = Some(id.clone());
        let task = app.tasks.current_mut().unwrap();
        task.phase = if builder {
            Phase::Building
        } else {
            Phase::Preparing
        };
        if builder {
            task.implementation_pending = false;
            task.attempts.push(Attempt::new(0));
        }
        if builder {
            app.task_persist_initial_builder(before.clone());
        } else {
            app.task_persist_preparation(before.clone());
        }
        let StoreCommand::SaveAcknowledged { reply, .. } =
            commands.recv_timeout(Duration::from_secs(3)).unwrap()
        else {
            panic!()
        };
        reply
            .send(Err("definitive durable transition rejection".into()))
            .unwrap();
        app.task_poll_recovery();
        assert_eq!(app.tasks.current().unwrap().phase, Phase::Interrupted);
        assert!(app.tasks.current().unwrap().attempts.is_empty());
        assert!(app.tasks.live.is_none());
        assert!(app.tasks.job.is_none());
        assert!(app.tasks.processes.is_empty());
        assert!(
            app.task_initial_resume_request(&id).is_err(),
            "wait for authoritative reconciliation"
        );
        let StoreCommand::ReleaseCurrent { reply, .. } =
            commands.recv_timeout(Duration::from_secs(3)).unwrap()
        else {
            panic!()
        };
        reply
            .send(Ok(refresh::Snapshot {
                warning: None,
                records: vec![before.clone()],
                owned: Default::default(),
            }))
            .unwrap();
        app.task_poll_recovery();
        assert_eq!(
            record(app.tasks.current().unwrap()).unwrap(),
            record(&before).unwrap()
        );
        assert!(app.task_initial_resume_request(&id).is_ok());
        assert!(
            commands.try_recv().is_err(),
            "failure path cannot save, restart, or retry implementation"
        );
    }
}
