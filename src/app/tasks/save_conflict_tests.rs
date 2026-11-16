use super::*;

#[test]
fn released_crash_owner_can_claim_exact_interrupted_record_for_fingerprint_only() {
    let home = std::env::temp_dir().join(format!("pui-crash-revalidate-{}", uuid::Uuid::new_v4()));
    let mut raw = Task::new(home.clone(), "p".into(), "m".into(), "high".into());
    raw.phase = Phase::Checking;
    raw.attempts.push(Attempt::new(0));
    raw.attempts[0].fingerprint = Some("captured-before-crash".into());
    domain::store::save(&home, &raw).unwrap();
    let owner = domain::store::lock(&home, &raw.id).unwrap();
    let recovered = domain::store::load(&home).unwrap().remove(0);
    assert_eq!(recovered.phase, Phase::Interrupted);
    let original = std::fs::read(home.join(format!("{}.json", raw.id))).unwrap();
    let (commands, rx) = mpsc::channel();
    let (errors, error_rx) = mpsc::channel();
    let root = home.clone();
    let worker =
        std::thread::spawn(move || run_store(&root, rx, errors, &egui::Context::default()));
    let claim = |expected: Task| {
        let (reply, result) = mpsc::channel();
        commands
            .send(StoreCommand::ClaimCurrent {
                expected: Box::new(expected),
                reply,
            })
            .unwrap();
        result.recv_timeout(Duration::from_secs(3)).unwrap()
    };
    assert_eq!(
        claim(recovered.clone()),
        Err(domain::store::LOCK_CONFLICT.into())
    );
    drop(owner);
    let mut stale = recovered.clone();
    stale.attempts[0].note = "not authoritative".into();
    assert!(claim(stale).is_err());
    claim(recovered).unwrap();
    // Claiming changes neither disk evidence nor the uncertain crash state.
    assert_eq!(
        std::fs::read(home.join(format!("{}.json", raw.id))).unwrap(),
        original
    );
    assert!(domain::store::lock(&home, &raw.id).is_err());
    assert!(error_rx.try_recv().is_err());
    drop(commands);
    worker.join().unwrap();
}

#[test]
fn queued_stale_saves_stay_rejected_after_foreign_owner_releases_until_fresh_validated_claim() {
    let home = std::env::temp_dir().join(format!("pui-save-rejected-{}", uuid::Uuid::new_v4()));
    let mut expected = Task::new(home.clone(), "p".into(), "m".into(), "high".into());
    expected.phase = Phase::Blocked;
    expected.attempts.push(Attempt::new(0));
    expected.attempts[0].note = "old evidence".into();
    domain::store::save(&home, &expected).unwrap();
    let foreign = domain::store::lock(&home, &expected.id).unwrap();
    let (commands, rx) = mpsc::channel();
    let (errors, error_rx) = mpsc::channel();
    let root = home.clone();
    let worker =
        std::thread::spawn(move || run_store(&root, rx, errors, &egui::Context::default()));
    commands
        .send(StoreCommand::Save(Box::new(expected.clone())))
        .unwrap();
    assert!(
        error_rx
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .conflict
    );
    let mut fresh = expected.clone();
    fresh.attempts[0].note = "new foreign evidence, same timestamp".into();
    domain::store::save(&home, &fresh).unwrap();
    drop(foreign);
    // A previously queued snapshot cannot borrow the newly available lock.
    commands
        .send(StoreCommand::Save(Box::new(expected.clone())))
        .unwrap();
    assert!(
        error_rx
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .conflict
    );
    let unrelated = Task::new(home.clone(), "p".into(), "m".into(), "high".into());
    commands
        .send(StoreCommand::Save(Box::new(unrelated.clone())))
        .unwrap();
    let (reply, stale_claim) = mpsc::channel();
    commands
        .send(StoreCommand::ClaimCurrent {
            expected: Box::new(expected.clone()),
            reply,
        })
        .unwrap();
    assert!(
        stale_claim
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .is_err()
    );
    commands
        .send(StoreCommand::Save(Box::new(expected)))
        .unwrap();
    assert!(
        error_rx
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .conflict
    );
    let current = domain::store::load(&home).unwrap();
    assert_eq!(
        current.iter().find(|t| t.id == fresh.id).unwrap().attempts[0].note,
        fresh.attempts[0].note
    );
    assert!(current.iter().any(|t| t.id == unrelated.id));
    let (reply, fresh_claim) = mpsc::channel();
    commands
        .send(StoreCommand::ClaimCurrent {
            expected: Box::new(fresh.clone()),
            reply,
        })
        .unwrap();
    fresh_claim
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
        .unwrap();
    fresh.note = "acknowledged current owner save".into();
    commands
        .send(StoreCommand::Save(Box::new(fresh.clone())))
        .unwrap();
    drop(commands);
    worker.join().unwrap();
    assert!(error_rx.try_recv().is_err());
    assert_eq!(
        domain::store::load(&home)
            .unwrap()
            .iter()
            .find(|t| t.id == fresh.id)
            .unwrap()
            .note,
        fresh.note
    );
}
