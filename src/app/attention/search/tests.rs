use super::*;
fn candidate(path: Option<&str>, id: &str) -> crate::app::search::Candidate {
    crate::app::search::Candidate {
        title: "Same title".into(),
        project: "/repo".into(),
        cwd: "/repo".into(),
        path: path.map(PathBuf::from),
        target: sidebar_tree::Target::Agent(id.into()),
    }
}
#[test]
fn snapshot_uses_only_exact_own_unread_recent_events_never_live_or_descendant_status() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.attention.observe(
        Owner::Saved("/repo/saved.jsonl".into()),
        Path::new("/repo"),
        Kind::Completed,
    );
    app.attention.observe(
        Owner::Runtime("child".into()),
        Path::new("/repo"),
        Kind::NeedsInput,
    );
    app.attention.observe(
        Owner::Runtime("read".into()),
        Path::new("/repo"),
        Kind::Failed,
    );
    app.attention.mark_read(&Owner::Runtime("read".into()));
    app.attention.observe(
        Owner::Runtime("old".into()),
        Path::new("/repo"),
        Kind::Completed,
    );
    app.attention.entries.back_mut().unwrap().metadata.at = now() - MAX_AGE_SECONDS - 1;
    app.attention.observe(
        Owner::Runtime("future".into()),
        Path::new("/repo"),
        Kind::Completed,
    );
    app.attention.entries.back_mut().unwrap().metadata.at = now() + 60;
    let snapshot = app.search_attention_snapshot();
    assert!(
        app.search_attention(&candidate(Some("/repo/saved.jsonl"), "other"), &snapshot)
            .is_some()
    );
    assert!(
        app.search_attention(&candidate(Some("/repo/other.jsonl"), "child"), &snapshot)
            .is_none(),
        "real saved identity wins over runtime guess"
    );
    assert_eq!(
        app.search_attention(&candidate(None, "child"), &snapshot)
            .unwrap()
            .label(),
        "Observed: needs input"
    );
    for id in ["parent", "read", "old", "future"] {
        assert!(
            app.search_attention(&candidate(None, id), &snapshot)
                .is_none()
        );
    }
    let task = crate::app::search::Candidate {
        target: sidebar_tree::Target::Task("child".into(), None),
        ..candidate(None, "child")
    };
    assert!(app.search_attention(&task, &snapshot).is_none());
}
#[test]
fn snapshot_keeps_original_event_when_read_replaced_forgotten_and_is_bounded_at_256() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    for n in 0..300 {
        app.attention.observe(
            Owner::Runtime(format!("runtime-{n}")),
            Path::new("/repo"),
            Kind::Completed,
        );
    }
    let snapshot = app.search_attention_snapshot();
    assert_eq!(snapshot.runtime.len(), MAX_RECORDS);
    assert!(snapshot.saved.is_empty());
    let item = candidate(None, "runtime-299");
    let old = app.search_attention(&item, &snapshot).unwrap();
    app.attention.mark_all_read();
    app.attention.observe(
        Owner::Runtime("runtime-299".into()),
        Path::new("/repo"),
        Kind::Failed,
    );
    assert_eq!(app.search_attention(&item, &snapshot), Some(old));
    assert_eq!(
        app.search_attention(&item, &app.search_attention_snapshot())
            .unwrap()
            .label(),
        "Observed: failed"
    );
    let capacity = snapshot.runtime.keys().map(String::capacity).sum::<usize>();
    assert!(capacity < 256 * 32);
    let start = std::time::Instant::now();
    for _ in 0..10000 {
        assert!(app.search_attention(&item, &snapshot).is_some());
    }
    eprintln!(
        "10000frozenattentionlookups {:?}; snapshot{}boundedruntimekeys; {capacity}B key capacity + map overhead",
        start.elapsed(),
        snapshot.runtime.len()
    );
}
