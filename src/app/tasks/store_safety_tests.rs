use super::*;

fn home() -> PathBuf {
    std::env::temp_dir().join(format!("pui-store-safety-{}", uuid::Uuid::new_v4()))
}
fn task(home: &std::path::Path) -> Task {
    let mut task = Task::new(home.into(), "p".into(), "m".into(), "high".into());
    task.parent_session = Some(home.join("original-conversation.jsonl"));
    task.phase = Phase::Blocked;
    task.attempts.push(Attempt::new(0));
    task.attempts[0].fingerprint = Some("captured".into());
    task.attempts[0].note = "Retained evidence".into();
    task.approved = Some(Contract {
        criteria: vec![domain::Criterion {
            id: "C1".into(),
            description: "Retain approved command".into(),
            command: "never execute".into(),
        }],
    });
    task.runs.push(Run {
        id: uuid::Uuid::new_v4().to_string(),
        role: Role::Checks,
        attempt: 0,
        status: "Blocked".into(),
        session: None,
        summary: "Original evidence".into(),
        cost: 1.25,
        permission_cost: 0.125,
        selection: None,
        routing_for: None,
    });
    domain::store::save(home, &task).unwrap();
    task
}

#[test]
fn save_lock_conflict_releases_projection_and_stops_only_rejected_owner_without_global_failure() {
    let home = home();
    let original = task(&home);
    let next = task(&home);
    let _foreign = domain::store::lock(&home, &original.id).unwrap();
    let mut stale = original.clone();
    stale.note = "rejected write".into();
    let (commands, rx) = mpsc::channel();
    let (errors, error_rx) = mpsc::channel();
    commands
        .send(StoreCommand::Save(Box::new(stale.clone())))
        .unwrap();
    commands
        .send(StoreCommand::Save(Box::new(next.clone())))
        .unwrap();
    drop(commands);
    run_store(&home, rx, errors, &egui::Context::default());
    let mut tasks = Tasks::default();
    tasks.records = vec![stale, next.clone()];
    tasks.live = Some(original.id.clone());
    tasks.local_owned.insert(original.id.clone());
    tasks.local_owned.insert(next.id.clone());
    tasks.errors = Some(error_rx);
    let (_result, result) = mpsc::channel();
    tasks.job = Some(Job {
        task: original.id.clone(),
        rx: result,
    });
    tasks.persist_loaded();
    assert!(!tasks.storage_failed);
    assert!(
        tasks
            .error
            .as_deref()
            .unwrap()
            .contains("another Pui window")
    );
    assert!(!tasks.local_owned.contains(&original.id));
    assert!(tasks.local_owned.contains(&next.id));
    assert!(tasks.live.is_none());
    assert!(tasks.job.is_none());
    assert!(tasks.updates.requested);
    let persisted = domain::store::load(&home).unwrap();
    assert_eq!(
        persisted.iter().find(|t| t.id == original.id).unwrap().note,
        original.note
    );
    assert!(persisted.iter().any(|t| t.id == next.id));
}

#[test]
fn nonconflict_save_errors_still_fail_closed_and_unrelated_live_owner_survives_conflicts() {
    let home = home();
    let foreign = task(&home);
    let mut tasks = Tasks::default();
    tasks.live = Some("other-live-task".into());
    tasks.local_owned.insert(foreign.id.clone());
    let (errors, rx) = mpsc::channel();
    tasks.errors = Some(rx);
    errors
        .send(StoreError {
            task: foreign.id.clone(),
            message: domain::store::LOCK_CONFLICT.into(),
            conflict: true,
        })
        .unwrap();
    tasks.persist_loaded();
    assert_eq!(tasks.live.as_deref(), Some("other-live-task"));
    assert!(!tasks.storage_failed);
    errors
        .send(StoreError {
            task: foreign.id,
            message: "disk unavailable".into(),
            conflict: false,
        })
        .unwrap();
    tasks.persist_loaded();
    assert!(tasks.storage_failed);
}

#[test]
fn detach_rejects_entire_stale_batch_before_first_write_and_releases_acquired_locks() {
    let home = home();
    let first = task(&home);
    let stale = task(&home);
    let mut current = stale.clone();
    current.attempts[0].note = "New authoritative evidence, same timestamp".into();
    domain::store::save(&home, &current).unwrap();
    assert_eq!(current.updated, stale.updated);
    let (commands, rx) = mpsc::channel();
    let (errors, error_rx) = mpsc::channel();
    let (reply, outcome) = mpsc::channel();
    let (refresh, refreshed) = mpsc::channel();
    commands
        .send(StoreCommand::Detach {
            tasks: vec![first.clone(), stale],
            reply,
            projection: None,
        })
        .unwrap();
    commands
        .send(StoreCommand::Refresh { reply: refresh })
        .unwrap();
    drop(commands);
    run_store(&home, rx, errors, &egui::Context::default());
    assert!(
        outcome
            .recv()
            .unwrap()
            .unwrap_err()
            .contains("changed in another window")
    );
    assert!(refreshed.recv().unwrap().unwrap().owned.is_empty());
    assert!(error_rx.try_recv().is_err());
    let records = domain::store::load(&home).unwrap();
    for expected in [&first, &current] {
        assert_eq!(
            serde_json::to_value(records.iter().find(|t| t.id == expected.id).unwrap()).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        let _guard = domain::store::lock(&home, &expected.id).unwrap();
    }
}

#[test]
fn detach_lock_conflict_and_active_foreign_records_leave_every_relationship_untouched() {
    for active in [false, true] {
        let home = home();
        let first = task(&home);
        let mut foreign = task(&home);
        let guard = if active {
            foreign.phase = Phase::Checking;
            domain::store::save(&home, &foreign).unwrap();
            None
        } else {
            Some(domain::store::lock(&home, &foreign.id).unwrap())
        };
        let records = domain::store::load(&home).unwrap();
        let before = serde_json::to_value(&records).unwrap();
        let (commands, rx) = mpsc::channel();
        let (errors, error_rx) = mpsc::channel();
        let (reply, outcome) = mpsc::channel();
        let (refresh, refreshed) = mpsc::channel();
        commands
            .send(StoreCommand::Detach {
                tasks: records,
                reply,
                projection: None,
            })
            .unwrap();
        commands
            .send(StoreCommand::Refresh { reply: refresh })
            .unwrap();
        drop(commands);
        run_store(&home, rx, errors, &egui::Context::default());
        assert!(outcome.recv().unwrap().is_err());
        assert!(refreshed.recv().unwrap().unwrap().owned.is_empty());
        assert!(error_rx.try_recv().is_err());
        assert_eq!(
            serde_json::to_value(domain::store::load(&home).unwrap()).unwrap(),
            before
        );
        let _first_lock = domain::store::lock(&home, &first.id).unwrap();
        drop(guard);
    }
}

#[test]
fn deletion_caller_cannot_bypass_detach_validation_with_optimistic_helper_save() {
    for stale in [false, true] {
        let home = home();
        let expected = task(&home);
        let mut authoritative = expected.clone();
        if stale {
            authoritative.attempts[0].note = "New authoritative evidence".into();
            domain::store::save(&home, &authoritative).unwrap();
        }
        let before = serde_json::to_value(&authoritative).unwrap();
        let parent = expected.parent_session.clone().unwrap();
        let mut tasks = Tasks::default();
        tasks.records = vec![expected.clone()];
        tasks
            .parents
            .insert(expected.id.clone(), "original-agent".into());
        let (commands, rx) = mpsc::channel();
        let (errors, error_rx) = mpsc::channel();
        // Snapshot is produced before detachment, but delivered to the UI only
        // after its acknowledgment. No local ownership masks stale projection.
        let (refresh, stale_refresh) = mpsc::channel();
        commands
            .send(StoreCommand::Refresh { reply: refresh })
            .unwrap();
        tasks.updates.pending = Some(stale_refresh);
        tasks.store = Some(commands);
        let outcome = tasks.prepare_detach_deleted_chat(&parent).unwrap().unwrap();
        // Match actual deletion.rs call order: helper executes before worker ack.
        tasks.detach_deleted_chat(&parent);
        assert_eq!(tasks.records[0].parent_session, expected.parent_session);
        assert!(tasks.parents.contains_key(&expected.id));
        tasks.store.take();
        run_store(&home, rx, errors, &egui::Context::default());
        assert_eq!(outcome.recv().unwrap().is_err(), stale);
        tasks.persist_loaded();
        assert!(tasks.updates.pending.is_none(), "discard pre-ack refresh");
        assert!(tasks.updates.requested);
        assert!(error_rx.try_recv().is_err());
        let actual = domain::store::load(&home).unwrap().remove(0);
        if stale {
            assert_eq!(serde_json::to_value(actual).unwrap(), before);
            assert_eq!(tasks.records[0].parent_session, expected.parent_session);
            assert!(tasks.parents.contains_key(&expected.id));
        } else {
            authoritative.parent_session = None;
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(&authoritative).unwrap()
            );
            assert!(tasks.records[0].parent_session.is_none());
            assert!(!tasks.parents.contains_key(&expected.id));
        }
    }
}

#[test]
fn successful_detach_changes_only_parent_and_preserves_evidence_spend_and_contract() {
    let home = home();
    let expected = task(&home);
    let (commands, rx) = mpsc::channel();
    let (errors, error_rx) = mpsc::channel();
    let (reply, outcome) = mpsc::channel();
    commands
        .send(StoreCommand::Detach {
            tasks: vec![expected.clone()],
            reply,
            projection: None,
        })
        .unwrap();
    drop(commands);
    run_store(&home, rx, errors, &egui::Context::default());
    outcome.recv().unwrap().unwrap();
    assert!(error_rx.try_recv().is_err());
    let actual = domain::store::load(&home).unwrap().remove(0);
    assert!(actual.parent_session.is_none());
    let mut unchanged = expected;
    unchanged.parent_session = None;
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(unchanged).unwrap()
    );
}
