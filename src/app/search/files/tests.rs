use super::*;

#[test]
fn path_line_query_is_bounded_and_never_parses_urls_or_zero_as_positions() {
    for (value, expected) in [
        ("src/main.rs:42", ("src/main.rs", Some(42))),
        (
            "folder with spaces/a.rs:8",
            ("folder with spaces/a.rs", Some(8)),
        ),
        ("name:part.rs", ("name:part.rs", None)),
        (" file.rs ", ("file.rs", None)),
        ("", ("", None)),
    ] {
        assert_eq!(query(value).unwrap(), expected);
    }
    for value in [
        "a:0",
        "a:1000001",
        ":5",
        "a:999999999999999999999999999999",
        "a\n.rs",
    ] {
        assert!(query(value).is_err(), "{value}");
    }
}

pub(super) fn app() -> Pui {
    let ctx = egui::Context::default();
    crate::app::files::tests::app(&ctx)
}
pub(super) fn search(app: &mut Pui) {
    let ctx = app.ctx.clone();
    app.spotlight.changed_at = ctx.input(|input| input.time) - 1.0;
    app.refresh_search(&ctx);
    if let Some(rx) = app.spotlight.files.rx.take() {
        let (paths, limited) = rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        app.spotlight.files.paths = paths;
        app.spotlight.files.limited = limited;
    }
    app.spotlight.files.displayed = None;
    app.refresh_search(&ctx);
}
#[test]
fn quick_files_worker_is_filename_only_bounded_cancelled_and_workspace_scoped() {
    let mut app = app();
    let root = app.prefs.cwd.clone();
    std::fs::write(root.join("secret-marker.txt"), "DO_NOT_INDEX_CONTENT").unwrap();
    std::fs::write(root.join(".env"), "not discoverable").unwrap();
    std::fs::create_dir_all(root.join("target")).unwrap();
    std::fs::write(root.join("target/hidden.rs"), "").unwrap();
    app.open_quick_files();
    app.search = "example.rs:7".into();
    search(&mut app);
    assert_eq!(app.spotlight.hits.len(), 1);
    assert_eq!(
        app.spotlight.hits[0].candidate.title,
        "src/nested/example.rs:7"
    );
    assert!(app.spotlight.hits[0].detail.contains("line 7"));
    app.search = "DO_NOT_INDEX_CONTENT".into();
    search(&mut app);
    assert!(app.spotlight.hits.is_empty());
    app.search.clear();
    search(&mut app);
    assert!(app.spotlight.hits.iter().all(
        |hit| !hit.candidate.title.contains(".env") && !hit.candidate.title.contains("target/")
    ));
    app.search = "old".into();
    app.spotlight.changed_at = -1.0;
    app.refresh_search(&app.ctx.clone());
    let old = app.spotlight.files.cancel.clone();
    app.search = "new".into();
    search(&mut app);
    assert!(old.load(Ordering::Relaxed));
    app.close_search();
    assert!(app.spotlight.files.rx.is_none() && app.spotlight.files.paths.is_empty());
}
#[test]
fn current_file_hit_opens_without_prompt_abort_or_draft_change_and_stale_root_is_refused() {
    let mut app = app();
    app.state.busy = true;
    app.draft = "Kept draft".into();
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let count = app.state.messages.len();
    app.open_quick_files();
    app.search = "example.rs:1".into();
    search(&mut app);
    let hit = app.spotlight.hits[0].clone();
    let root = app.prefs.cwd.clone();
    app.prefs.cwd = root.join("src");
    app.activate_search_hit(hit.clone());
    assert!(app.search_open);
    assert!(app.spotlight.notice.as_deref().unwrap().contains("changed"));
    app.prefs.cwd = root;
    app.activate_search_hit(hit);
    assert!(!app.search_open && app.files.is_open());
    assert_eq!(app.draft, "Kept draft");
    assert!(app.state.busy);
    assert_eq!(app.state.messages.len(), count);
    assert!(commands.try_recv().is_err());
}
