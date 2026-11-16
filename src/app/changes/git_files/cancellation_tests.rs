use super::*;
use crate::app::files::tests as file_tests;
use std::time::{Duration, Instant};

#[cfg(unix)]
fn stalled<T: Send + 'static>(
    ctx: &egui::Context,
    name: &str,
    result: T,
) -> (Request<T>, mpsc::Receiver<(String, bool)>) {
    let (started, ready) = mpsc::channel();
    let (finished, done) = mpsc::channel();
    let request = spawn_read(ctx, named_permit(ctx, name).unwrap(), move |cancel| {
        let outcome = workspace::read_only::tests::stalled(cancel, started);
        finished.send(outcome).unwrap();
        result // delivery sees cancellation and must discard even this late result
    });
    ready.recv_timeout(Duration::from_secs(1)).unwrap();
    (request, done)
}

#[cfg(unix)]
#[test]
fn actual_supersession_and_session_leave_cancel_stalled_workers_reap_and_recover_permits() {
    for transition in [
        "replacement",
        "session leave",
        "window close",
        "new chat",
        "file navigation",
    ] {
        let ctx = egui::Context::default();
        let mut app = file_tests::app(&ctx);
        app.workspace = super::tests::workspace();
        app.draft = "Keep draft".into();
        app.open_diff_review(("first.rs".into(), "+old".into()).into(), false);
        let (request, done) = stalled(
            &ctx,
            "git-review-worker",
            super::tests::loaded(&app.workspace, 1, Ok("+late body".into())),
        );
        app.diff_rx = Some(request);
        let started = Instant::now();
        match transition {
            "replacement" => {
                app.open_diff_review(("replacement.rs".into(), "+new".into()).into(), true)
            }
            "session leave" => app.leave_session_layout(),
            "window close" => eframe::App::on_exit(&mut app, None),
            "new chat" => app.start_chat_in(std::env::temp_dir()),
            "file navigation" => {
                app.draft = "/files".into();
                assert!(app.submit_command());
                app.draft = "Keep draft".into();
            }
            _ => unreachable!(),
        }
        assert!(app.diff_rx.is_none());
        let (error, reaped) = done.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(error.contains("cancelled") && reaped);
        assert!(started.elapsed() < Duration::from_secs(1));
        // A second actual request waits only for the previous worker's cleanup,
        // never accumulates a second worker while a child is still alive.
        let gate = ctx
            .data(|data| data.get_temp::<Arc<AtomicBool>>(egui::Id::new("git-review-worker")))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while gate.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        let permit = permit(&ctx).expect("cancelled worker returns shared permit");
        drop(permit);
        let review = app.changes.review.as_ref().map(|doc| doc.0.clone());
        app.focus_composer = true;
        for _ in 0..2 {
            file_tests::frame(&ctx, &mut app, vec![]);
        }
        assert_eq!(app.changes.review.as_ref().map(|doc| doc.0.clone()), review);
        if transition == "replacement" {
            assert_eq!(review.as_deref(), Some("replacement.rs"));
        }
        if transition != "new chat" {
            assert_eq!(app.draft, "Keep draft");
        }
        assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new("composer"))));
    }
}

#[cfg(unix)]
#[test]
fn actual_session_leave_cancels_inline_and_status_not_saved_layout_workers() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    app.workspace = super::tests::workspace();
    let (inline, inline_done) = stalled(&ctx, "git-review-worker", Ok("+late inline".into()));
    app.changes.diffs.insert(
        "first.rs".into(),
        Diff {
            content: None,
            rx: Some(inline),
        },
    );
    let (status, status_done) = stalled(&ctx, "git-workspace-worker", app.workspace.clone());
    app.workspace_rx = Some(status);
    app.leave_session_layout();
    assert!(app.workspace_rx.is_none() && app.changes.diffs.is_empty());
    for done in [inline_done, status_done] {
        let (error, reaped) = done.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(error.contains("cancelled") && reaped);
    }
    app.enter_session_layout();
    assert!(app.changes.diffs.is_empty());
    app.poll_views();
    assert!(app.changes.review.is_none());
}

#[test]
fn actual_inline_and_embedded_git_reads_reject_filters_without_running_them() {
    let root = workspace::tests::fixture();
    let mut workspace = workspace::inspect(&root);
    assert!(workspace.error.is_none());
    workspace.files.retain(|file| file.path == "README.md");
    let index = std::fs::read(root.join(".git/index")).unwrap();
    let head = std::fs::read(root.join(".git/HEAD")).unwrap();
    std::fs::write(
        root.join(".git/info/attributes"),
        "* filter=pui-cancel-test\n",
    )
    .unwrap();
    let mut config = std::fs::OpenOptions::new()
        .append(true)
        .open(root.join(".git/config"))
        .unwrap();
    use std::io::Write;
    writeln!(
        config,
        "[filter \"pui-cancel-test\"]\n clean = printf ran > .git/pui-filter-ran; cat"
    )
    .unwrap();
    drop(config);
    let ctx = egui::Context::default();
    let file = workspace.files[0].clone();
    let inline = start_inline(&ctx, &root, file.clone(), workspace::DiffMode::Combined)
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap_err();
    let embedded = start(&ctx, &workspace, file)
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .finish(&workspace)
        .unwrap()
        .unwrap_err();
    assert!(inline.contains("clean/process filters"));
    assert_eq!(inline, embedded);
    assert!(!root.join(".git/pui-filter-ran").exists());
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(root.join(".git/HEAD")).unwrap(), head);
}

#[cfg(unix)]
#[test]
fn deadline_returns_permit_and_busy_status_never_starts_another_worker() {
    let ctx = egui::Context::default();
    let (started, ready) = mpsc::channel();
    let (finished, done) = mpsc::channel();
    let active = spawn_read(&ctx, permit(&ctx).unwrap(), move |cancel| {
        let result =
            workspace::read_only::tests::stalled_for(cancel, started, Duration::from_millis(60));
        finished.send(result.clone()).unwrap();
        result
    });
    ready.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(permit(&ctx).is_none());
    let (error, reaped) = done.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(error.contains("timed out") && reaped);
    assert!(active.recv_timeout(Duration::from_secs(1)).unwrap().1);
    assert!(permit(&ctx).is_some());

    let (status, done) = stalled(&ctx, "git-workspace-worker", Workspace::default());
    for _ in 0..1024 {
        let busy = start_workspace(&ctx, "/unused-fixture-root".into())
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        assert!(busy.error.as_deref().unwrap().contains("still finishing"));
    }
    drop(status);
    let (error, reaped) = done.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(error.contains("cancelled") && reaped);
}
