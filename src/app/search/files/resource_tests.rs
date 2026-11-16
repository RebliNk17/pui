use super::tests::{app, search};
use super::*;

#[test]
fn result_budget_and_line_only_edits_do_not_restart_filename_discovery() {
    let mut app = app();
    let root = app.prefs.cwd.clone();
    std::fs::create_dir(root.join("many")).unwrap();
    for n in 0..240 {
        std::fs::write(root.join(format!("many/file-{n:03}.rs")), "not indexed").unwrap();
    }
    let start = std::time::Instant::now();
    app.open_quick_files();
    app.search = "many/".into();
    search(&mut app);
    assert_eq!(app.spotlight.files.paths.len(), 200);
    assert_eq!(app.spotlight.hits.len(), 200);
    assert!(app.spotlight.files.limited);
    let retained: usize = app
        .spotlight
        .hits
        .iter()
        .map(|hit| {
            hit.candidate.title.capacity()
                + hit.detail.capacity()
                + hit.candidate.project.capacity()
                + hit.candidate.cwd.capacity()
                + hit.candidate.path.as_ref().map_or(0, |p| p.capacity())
        })
        .sum::<usize>()
        + app
            .spotlight
            .files
            .paths
            .iter()
            .map(|path| path.capacity())
            .sum::<usize>();
    eprintln!(
        "quick-open fixture: 240 filenames / 200 retained in {:?}; path/string capacities {} bytes (excludes object/channel/font/OS overhead)",
        start.elapsed(),
        retained
    );
    let paths = app.spotlight.files.paths.as_ptr();
    let cancel = app.spotlight.files.cancel.clone();
    for line in [1, 2, 800] {
        app.search = format!("many/:{line}");
        search(&mut app);
        assert!(Arc::ptr_eq(&cancel, &app.spotlight.files.cancel));
        assert_eq!(paths, app.spotlight.files.paths.as_ptr());
    }
    app.close_search();
    assert!(app.spotlight.hits.is_empty());
    assert!(app.spotlight.files.paths.is_empty());
}
#[test]
fn stale_query_line_scope_and_disconnected_worker_cannot_retarget_a_result() {
    let mut app = app();
    let root = app.prefs.cwd.clone();
    std::fs::create_dir_all(root.join("nested/src/nested")).unwrap();
    std::fs::write(root.join("nested/src/nested/example.rs"), "").unwrap();
    app.open_quick_files();
    app.search = "src/nested/example.rs:1".into();
    search(&mut app);
    let hit = app.spotlight.hits[0].clone();
    assert_eq!(hit.candidate.path, Some(root.join("src/nested/example.rs")));
    app.search = "src/nested/example.rs:2".into();
    app.activate_search_hit(hit.clone());
    assert!(app.search_open && !app.files.is_open());
    app.search = "README".into();
    app.activate_search_hit(hit.clone());
    assert!(!app.files.is_open());
    app.spotlight.scope = Scope::Settings;
    app.activate_search_hit(hit);
    assert!(!app.files.is_open());
    app.refresh_search(&app.ctx.clone());
    assert!(app.spotlight.files.paths.is_empty());
    app.open_quick_files();
    app.search = "example.rs".into();
    search(&mut app);
    let (tx, rx) = mpsc::channel();
    drop(tx);
    app.spotlight.files.rx = Some(rx);
    app.refresh_search(&app.ctx.clone());
    assert!(!app.spotlight.files.pending());
    assert!(app.spotlight.files.note().unwrap().contains("stopped"));
}

#[test]
fn workspace_change_and_scope_switch_drop_late_worker_results() {
    let mut app = app();
    app.open_quick_files();
    app.search = "example.rs".into();
    search(&mut app);
    let (tx, rx) = mpsc::channel();
    app.spotlight.files.rx = Some(rx);
    let old = app.spotlight.files.cancel.clone();
    app.prefs.cwd = app.prefs.cwd.join("src");
    search(&mut app);
    assert!(old.load(Ordering::Relaxed));
    assert!(
        tx.send(Ok((vec![PathBuf::from("/outside/stale.rs")], false)))
            .is_err()
    );
    assert!(
        app.spotlight
            .hits
            .iter()
            .all(|hit| hit.candidate.cwd == app.prefs.cwd)
    );
    let (tx, rx) = mpsc::channel();
    app.spotlight.files.rx = Some(rx);
    app.spotlight.scope = Scope::All;
    app.refresh_search(&app.ctx.clone());
    assert!(tx.send(Ok((vec![], false))).is_err());
    assert!(app.spotlight.files.paths.is_empty());
}
