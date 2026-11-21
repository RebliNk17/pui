use super::*;

fn task() -> Task {
    Task::new(
        "/fixture/project".into(),
        "p".into(),
        "m".into(),
        "off".into(),
    )
}
#[test]
fn external_refresh_reparents_removes_and_adds_retained_history_without_touching_owner() {
    let mut tasks = Tasks::default();
    let mut retained = task();
    retained.parent_session = Some("/fixture/old-chat".into());
    retained.attempts.push(Attempt::new(0));
    let evidence = serde_json::to_value(&retained.attempts).unwrap();
    let mut owner = task();
    owner.phase = Phase::Building;
    owner.note = "new in-memory owner state".into();
    let deleted = task();
    tasks
        .parents
        .insert(retained.id.clone(), "old-agent".into());
    tasks.records = vec![retained.clone(), owner.clone(), deleted.clone()];
    tasks.selected = Some(retained.id.clone());
    tasks.live = Some(owner.id.clone());
    let mut foreign_owner = owner.clone();
    foreign_owner.note = "older disk state".into();
    let mut moved = retained.clone();
    moved.parent_session = Some("/fixture/new-chat".into());
    let added = task();
    tasks.merge_history(Snapshot {
        warning: None,
        records: vec![moved.clone(), foreign_owner, added.clone()],
        owned: BTreeSet::from([owner.id.clone()]),
    });
    assert_eq!(
        tasks.current().unwrap().parent_session,
        moved.parent_session
    );
    assert!(!tasks.parents.contains_key(&retained.id));
    assert_eq!(
        serde_json::to_value(&tasks.current().unwrap().attempts).unwrap(),
        evidence
    );
    assert_eq!(tasks.live_task().unwrap().note, owner.note);
    assert!(!tasks.records.iter().any(|t| t.id == deleted.id));
    assert!(tasks.records.iter().any(|t| t.id == added.id));
}
#[test]
fn queued_local_save_and_owner_claim_are_not_overwritten_by_older_refresh() {
    let mut tasks = Tasks::default();
    let local = task();
    let claim = task();
    tasks.records = vec![local.clone(), claim.clone()];
    tasks.local_owned.insert(local.id.clone());
    let (_tx, rx) = mpsc::channel();
    tasks.revalidate_claim = Some((claim.id.clone(), rx));
    tasks.merge_history(Snapshot {
        warning: None,
        records: vec![],
        owned: BTreeSet::new(),
    });
    assert_eq!(tasks.records.len(), 2);
}
#[test]
fn real_history_watch_delivers_external_atomic_save_and_refresh() {
    let home = std::env::temp_dir().join(format!("pui-history-watch-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    let watch = watch(home.clone(), &egui::Context::default());
    watch.watcher.wait_ready().unwrap();
    domain::store::save(&home, &task()).unwrap();
    watch.events.recv_timeout(Duration::from_secs(5)).unwrap();
}
