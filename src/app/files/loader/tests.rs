use super::*;

pub(super) fn idle(pool: &Pool) {
    let (state, timeout) = pool
        .1
        .wait_timeout_while(pool.0.lock().unwrap(), Duration::from_secs(8), |state| {
            state.active > 0
        })
        .unwrap();
    assert!(!timeout.timed_out(), "File workers did not finish");
    assert!(state.pending.is_empty());
}
pub(super) struct Blocked {
    pub(super) pool: Arc<Pool>,
    release: Vec<mpsc::Sender<()>>,
    done: Vec<Receiver<Result<Content, String>>>,
    _tickets: Vec<Ticket>,
}
impl Drop for Blocked {
    fn drop(&mut self) {
        for release in self.release.drain(..) {
            let _ = release.send(());
        }
        idle(&self.pool);
    }
}
fn job(
    ctx: &egui::Context,
    root: &Path,
    path: &str,
) -> (Job, Receiver<Result<Content, String>>, Ticket) {
    let pool = pool(ctx);
    let cancelled = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    (
        Job {
            root: root.into(),
            path: root.join(path),
            external: false,
            ctx: ctx.clone(),
            tx,
            budget: budget::Lease::new(ctx),
            cancelled: cancelled.clone(),
            before_read: None,
            svg_helper: None,
        },
        rx,
        Ticket {
            pool: Arc::downgrade(&pool),
            cancelled,
        },
    )
}
pub(super) fn blocked(ctx: &egui::Context, root: &Path) -> Blocked {
    let pool = pool(ctx);
    let mut blocked = Blocked {
        pool: pool.clone(),
        release: vec![],
        done: vec![],
        _tickets: vec![],
    };
    for _ in 0..MAX_ACTIVE {
        let (mut job, rx, ticket) = job(ctx, root, "README.md");
        let (tx, release) = mpsc::channel();
        let (started, ready) = mpsc::channel();
        job.before_read = Some(Box::new(move || {
            started.send(()).unwrap();
            release.recv_timeout(Duration::from_secs(8)).unwrap();
        }));
        pool.submit(job).unwrap();
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        blocked.release.push(tx);
        blocked.done.push(rx);
        blocked._tickets.push(ticket);
    }
    blocked
}

#[test]
fn two_workers_and_bounded_fifo_queue_share_across_chats_without_waiting_threads() {
    let ctx = egui::Context::default();
    let app = crate::app::files::tests::app(&ctx);
    let mut blocked = blocked(&ctx, &app.prefs.cwd);
    let pool = blocked.pool.clone();
    let mut tickets = vec![];
    let mut results = vec![];
    let (started, order) = mpsc::channel();
    let start = std::time::Instant::now();
    for n in 0..MAX_PENDING {
        let (mut job, rx, ticket) = job(&ctx, &app.prefs.cwd, "README.md");
        let tx = started.clone();
        job.before_read = Some(Box::new(move || {
            tx.send(n).unwrap();
        }));
        pool.submit(job).unwrap();
        tickets.push(ticket);
        results.push(rx);
    }
    assert_eq!(pool.0.lock().unwrap().active, 2);
    assert_eq!(pool.0.lock().unwrap().pending.len(), 32);
    let (rx, rejected) = start_request(&ctx, &app.prefs.cwd);
    assert!(rejected.is_none());
    assert!(
        rx.recv_timeout(Duration::from_secs(1))
            .unwrap()
            .err()
            .unwrap()
            .contains("queue is full")
    );
    // Releasing one worker drains the FIFO; the other remains held.
    blocked.release.remove(0).send(()).unwrap();
    for n in 0..MAX_PENDING {
        assert_eq!(order.recv_timeout(Duration::from_secs(3)).unwrap(), n);
    }
    for rx in results {
        assert!(rx.recv_timeout(Duration::from_secs(3)).unwrap().is_ok());
    }
    eprintln!(
        "2 active + 32 queued + explicit overflow: {:?}; shallow queued Job storage {} bytes (paths/Arc/channel/allocator overhead excluded), no waiting threads",
        start.elapsed(),
        MAX_PENDING * std::mem::size_of::<Job>()
    );
    drop(tickets);
    drop(blocked);
    assert_eq!(pool.0.lock().unwrap().active, 0);
}
fn start_request(
    ctx: &egui::Context,
    root: &Path,
) -> (Receiver<Result<Content, String>>, Option<Ticket>) {
    start(
        ctx,
        root.into(),
        root.join("README.md"),
        false,
        budget::Lease::new(ctx),
    )
}

#[test]
fn dropping_queued_ticket_removes_request_without_read_or_followup_repaint() {
    let ctx = egui::Context::default();
    let app = crate::app::files::tests::app(&ctx);
    let blocked = blocked(&ctx, &app.prefs.cwd);
    let (mut job, rx, ticket) = job(&ctx, &app.prefs.cwd, "README.md");
    let (read, observed) = mpsc::channel();
    job.before_read = Some(Box::new(move || {
        read.send(()).unwrap();
    }));
    blocked.pool.submit(job).unwrap();
    assert_eq!(blocked.pool.0.lock().unwrap().pending.len(), 1);
    drop(ticket);
    assert!(blocked.pool.0.lock().unwrap().pending.is_empty());
    assert!(matches!(
        rx.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    drop(blocked);
    assert!(observed.try_recv().is_err());
}

#[test]
fn active_cancellation_and_panicked_decode_do_not_strand_permits_or_retarget_result() {
    let ctx = egui::Context::default();
    let app = crate::app::files::tests::app(&ctx);
    let mut blocked = blocked(&ctx, &app.prefs.cwd);
    let pool = blocked.pool.clone();
    blocked._tickets.remove(0); // cancels a request already executing the test hook
    let (mut bad, failed, _ticket) = job(&ctx, &app.prefs.cwd, "README.md");
    bad.before_read = Some(Box::new(|| panic!("test parser failure")));
    pool.submit(bad).unwrap();
    let (ok, _ticket2) = start_request(&ctx, &app.prefs.cwd);
    blocked.release.remove(0).send(()).unwrap();
    assert!(
        failed
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .err()
            .unwrap()
            .contains("worker failed")
    );
    assert!(ok.recv_timeout(Duration::from_secs(3)).unwrap().is_ok());
    assert!(matches!(
        blocked.done[0].recv_timeout(Duration::from_secs(3)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
    drop(blocked);
    assert_eq!(pool.0.lock().unwrap().active, 0);
}

#[test]
fn queued_workspace_read_rechecks_replaced_symlink_and_does_not_gain_external_privilege() {
    let ctx = egui::Context::default();
    let app = crate::app::files::tests::app(&ctx);
    let root = &app.prefs.cwd;
    let blocked = blocked(&ctx, root);
    let path = root.join("README.md");
    let (rx, _ticket) = start_request(&ctx, root);
    fs::rename(&path, root.join("old.md")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("old.md"), &path).unwrap();
    drop(blocked);
    #[cfg(unix)]
    assert!(
        rx.recv_timeout(Duration::from_secs(2))
            .unwrap()
            .err()
            .unwrap()
            .contains("Symlinked")
    );
}

#[test]
fn parked_tabs_finish_but_close_refresh_and_workspace_drop_cancel_only_their_requests() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.state.session_file = Some("/fixture/loader-a".into());
    let root = app.prefs.cwd.clone();
    let blocked = blocked(&ctx, &root);
    app.files
        .open_target(&ctx, &root, root.join("README.md").to_str().unwrap())
        .unwrap();
    app.files.refresh(&ctx);
    assert_eq!(blocked.pool.0.lock().unwrap().pending.len(), 1);
    app.leave_session_layout();
    assert_eq!(
        blocked.pool.0.lock().unwrap().pending.len(),
        1,
        "Parking keeps explicit pending read"
    );
    app.session_layouts
        .forget_path(Path::new("/fixture/loader-a"));
    assert!(blocked.pool.0.lock().unwrap().pending.is_empty());
    app.files
        .open_target(&ctx, &root, root.join("README.md").to_str().unwrap())
        .unwrap();
    app.files.close_tab(0);
    assert!(blocked.pool.0.lock().unwrap().pending.is_empty());
    app.files
        .open_target(&ctx, &root, root.join("README.md").to_str().unwrap())
        .unwrap();
    app.files.workspace(&ctx, &root.join("src"));
    assert!(blocked.pool.0.lock().unwrap().pending.is_empty());
    drop(blocked);
}

#[test]
fn parked_explicit_read_completes_without_render_and_restores_only_to_its_chat() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.state.session_file = Some("/fixture/loader-kept".into());
    let root = app.prefs.cwd.clone();
    let blocked = blocked(&ctx, &root);
    app.files
        .open_workspace_source(&ctx, &root, &root.join("README.md"), Some(2))
        .unwrap();
    app.leave_session_layout();
    assert!(app.files.tabs.is_empty());
    drop(blocked);
    assert!(
        app.files.tabs.is_empty(),
        "Completed background read cannot select another viewer"
    );
    app.enter_session_layout();
    crate::app::files::tests::settle(&mut app.files);
    assert!(
        matches!(&app.files.tabs[0].content, Some(Ok(Content::Text(text))) if text.contains("Workspace"))
    );
    assert_eq!(app.files.tabs[0].line_target, Some(2));
}

#[test]
fn refresh_of_full_queue_cancels_old_same_path_before_admitting_replacement() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    let root = app.prefs.cwd.clone();
    let blocked = blocked(&ctx, &root);
    app.files
        .open_workspace_source(&ctx, &root, &root.join("README.md"), Some(2))
        .unwrap();
    let mut tickets = vec![];
    for _ in 1..MAX_PENDING {
        tickets.push(start_request(&ctx, &root).1);
    }
    assert_eq!(blocked.pool.0.lock().unwrap().pending.len(), MAX_PENDING);
    app.files.refresh(&ctx);
    assert!(
        app.files.tabs[0]._load.is_some(),
        "Old request must free a queue slot before Refresh replacement is admitted"
    );
    assert_eq!(blocked.pool.0.lock().unwrap().pending.len(), MAX_PENDING);
    drop(tickets);
    drop(blocked);
    crate::app::files::tests::settle(&mut app.files);
    assert!(
        matches!(&app.files.tabs[0].content, Some(Ok(Content::Text(text))) if text.contains("Workspace"))
    );
    assert!(!app.files.tabs[0].external && app.files.tabs[0].source);
}

#[test]
fn independent_contexts_have_independent_loader_pools() {
    let a = egui::Context::default();
    let b = egui::Context::default();
    assert!(Arc::ptr_eq(&pool(&a), &pool(&a)));
    assert!(!Arc::ptr_eq(&pool(&a), &pool(&b)));
}
