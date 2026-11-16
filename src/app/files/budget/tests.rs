use super::*;
use crate::app::files::tests as file_tests;

#[test]
fn shared_across_viewers_and_cloned_leases_release_exactly_once() {
    let ctx = egui::Context::default();
    let content = Content::Text(String::with_capacity(MAX_BYTES));
    let a = Lease::new(&ctx);
    let b = Lease::new(&ctx);
    a.admit(&content).unwrap();
    assert!(b.admit(&Content::Text("x".into())).is_err());
    assert_eq!(a.pool.used.load(Ordering::Relaxed), MAX_BYTES);
    let clone = a.clone();
    drop(a);
    assert_eq!(b.pool.used.load(Ordering::Relaxed), MAX_BYTES);
    drop(clone);
    assert_eq!(b.pool.used.load(Ordering::Relaxed), 0);
    b.admit(&Content::Text("available".into())).unwrap();
    assert_eq!(b.bytes.load(Ordering::Relaxed), 9);
    let oversized = Lease::new(&ctx);
    assert!(
        oversized
            .admit(&Content::Text(String::with_capacity(MAX_BYTES + 1)))
            .unwrap_err()
            .contains("smaller image")
    );
    assert_eq!(b.pool.used.load(Ordering::Relaxed), 9);
    // Contexts represent independent apps/tests, not a process-global hidden singleton.
    assert_eq!(
        Lease::new(&egui::Context::default())
            .pool
            .used
            .load(Ordering::Relaxed),
        0
    );
}

#[test]
fn lease_reconciliation_is_idempotent_shrinks_and_preserves_original_on_failed_growth() {
    let ctx = egui::Context::default();
    let lease = Lease::new(&ctx);
    lease.reserve(16).unwrap();
    lease.reserve(16).unwrap();
    assert_eq!(lease.pool.used.load(Ordering::Relaxed), 16);
    let other = Lease::new(&ctx);
    other.reserve(MAX_BYTES - 16).unwrap();
    assert!(lease.reserve(17).is_err());
    assert_eq!(lease.bytes.load(Ordering::Relaxed), 16);
    lease.reserve(8).unwrap();
    assert_eq!(lease.pool.used.load(Ordering::Relaxed), MAX_BYTES - 8);
    lease.release();
    lease.release();
    assert_eq!(lease.pool.used.load(Ordering::Relaxed), MAX_BYTES - 16);
    drop(lease);
    other.reserve(MAX_BYTES).unwrap();
}

#[test]
fn table_capacity_counts_source_lines_index_and_escaped_decodes() {
    let preview = delimited::Preview::new("Name,Note\nAlpha,\"a\"\"b\"\n".into(), b',');
    let bytes = preview.retained_bytes();
    assert!(bytes > preview.source.capacity());
    let content = Content::Table(preview);
    assert_eq!(content.retained_bytes(), bytes);
    let invalid = delimited::Preview::new("\"unterminated".into(), b',');
    assert!(invalid.retained_bytes() > invalid.source.capacity());
}

#[test]
fn worker_and_queued_result_keep_charge_until_last_owner_drops() {
    let ctx = egui::Context::default();
    let tab = Lease::new(&ctx);
    let worker = tab.clone();
    let observer = Lease::new(&ctx);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = Content::Text("worker result".into());
        worker.admit(&result).unwrap();
        tx.send((result, worker)).unwrap();
    })
    .join()
    .unwrap();
    assert_eq!(observer.pool.used.load(Ordering::Relaxed), 13);
    drop(tab);
    assert_eq!(observer.pool.used.load(Ordering::Relaxed), 13);
    drop(rx); // includes completed channel payload and worker lease
    assert_eq!(observer.pool.used.load(Ordering::Relaxed), 0);
}

#[test]
fn simultaneous_admission_never_oversubscribes_or_leaks_failed_reservations() {
    let ctx = egui::Context::default();
    let observer = Lease::new(&ctx);
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let lease = Lease::new(&ctx);
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let content = Content::Text(String::with_capacity(16 * 1024 * 1024));
                barrier.wait();
                let admitted = lease.admit(&content).is_ok();
                (lease, content, admitted)
            })
        })
        .collect();
    let retained: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(retained.iter().filter(|entry| entry.2).count(), 4);
    assert_eq!(observer.pool.used.load(Ordering::Relaxed), MAX_BYTES);
    drop(retained);
    assert_eq!(observer.pool.used.load(Ordering::Relaxed), 0);
}

#[test]
fn image_upload_retains_conservative_cpu_and_gpu_charge_until_tab_closes() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    let path = app.prefs.cwd.join("budget.png");
    image::RgbaImage::from_pixel(128, 128, image::Rgba([50, 100, 150, 255]))
        .save(&path)
        .unwrap();
    app.files
        .open_target(&ctx, &app.prefs.cwd.clone(), path.to_str().unwrap())
        .unwrap();
    file_tests::settle(&mut app.files);
    let observer = Lease::new(&ctx);
    let before = observer.pool.used.load(Ordering::Relaxed);
    assert_eq!(before, 128 * 128 * 4 * 2);
    for _ in 0..5 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    assert!(matches!(
        app.files.tabs[0].content,
        Some(Ok(Content::UploadedImage))
    ));
    assert!(app.files.tabs[0].texture.is_some());
    assert_eq!(observer.pool.used.load(Ordering::Relaxed), before);
    app.files.close_tab(0);
    assert_eq!(observer.pool.used.load(Ordering::Relaxed), 0);
}

#[test]
fn full_budget_is_visible_and_retry_is_explicit_preserving_draft_run_permissions_and_files() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let mut app = file_tests::app(&ctx);
        app.prefs.theme = mode;
        let occupied = Lease::new(&ctx);
        let held = Content::Text(String::with_capacity(MAX_BYTES));
        occupied.admit(&held).unwrap();
        app.state.busy = true;
        app.draft = "Keep draft".into();
        app.state.dialogs.push_back(json!({"id":"budget-confirm","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."}));
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let path = app.prefs.cwd.join("README.md");
        let bytes = fs::read(&path).unwrap();
        app.files
            .open_workspace_source(&ctx, &app.prefs.cwd.clone(), &path, Some(2))
            .unwrap();
        file_tests::settle(&mut app.files);
        assert!(
            matches!(&app.files.tabs[0].content, Some(Err(error)) if error.contains("64 MiB across chats"))
        );
        for _ in 0..5 {
            file_tests::frame(&ctx, &mut app, vec![]);
        }
        let out = file_tests::frame(&ctx, &mut app, vec![]);
        file_tests::text(&out, "Close file tabs");
        assert_eq!(app.draft, "Keep draft");
        assert!(app.state.busy && app.state.dialogs.len() == 1 && app.pending_dialog.is_none());
        assert!(commands.try_recv().is_err());
        drop(occupied);
        drop(held);
        // Repainting does not silently read/retry or swap the existing error.
        file_tests::frame(&ctx, &mut app, vec![]);
        assert!(matches!(app.files.tabs[0].content, Some(Err(_))));
        let refresh = ctx
            .data(|data| {
                data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("file-preview-refresh"))
            })
            .unwrap();
        file_tests::click(&ctx, &mut app, refresh.1.center());
        file_tests::settle(&mut app.files);
        assert!(
            matches!(&app.files.tabs[0].content, Some(Ok(Content::Text(text))) if text.as_bytes() == bytes)
        );
        assert!(!app.files.tabs[0].external && app.files.tabs[0].source);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        app.files.close_tab(0);
        // Receipt precedes the worker dropping its final lease. Wait for the
        // actual worker completion, not a scheduler-dependent immediate drop.
        loader::wait_idle(&ctx);
        assert_eq!(Lease::new(&ctx).pool.used.load(Ordering::Relaxed), 0);
    }
}

#[test]
fn sixteen_max_cell_tables_share_budget_across_parked_chats_and_release_on_delete() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    app.state.session_file = Some("/fixture/budget-a".into());
    let root = app.prefs.cwd.clone();
    let source = "a,".repeat(99) + "a\n";
    let source = source.repeat(1_000);
    let start = std::time::Instant::now();
    for n in 0..MAX_TABS {
        let path = root.join(format!("table{n}.csv"));
        fs::write(&path, &source).unwrap();
        app.files
            .open_target(&ctx, &root, path.to_str().unwrap())
            .unwrap();
    }
    file_tests::settle(&mut app.files);
    let observer = Lease::new(&ctx);
    let bytes = observer.pool.used.load(Ordering::Relaxed);
    let admitted = app
        .files
        .tabs
        .iter()
        .filter(|tab| matches!(tab.content, Some(Ok(_))))
        .count();
    assert!(bytes <= MAX_BYTES && admitted > 0 && admitted < MAX_TABS);
    assert_eq!(app.files.tabs.len(), MAX_TABS);
    app.leave_session_layout();
    assert!(app.files.tabs.is_empty());
    assert_eq!(observer.pool.used.load(Ordering::Relaxed), bytes);
    app.state.session_file = Some("/fixture/budget-b".into());
    app.files
        .open_target(&ctx, &root, root.join("table0.csv").to_str().unwrap())
        .unwrap();
    file_tests::settle(&mut app.files);
    assert!(
        matches!(&app.files.tabs[0].content, Some(Err(error)) if error.contains("64 MiB across chats")),
        "A second chat must share the parked chat's payload limit"
    );
    assert_eq!(observer.pool.used.load(Ordering::Relaxed), bytes);
    app.session_layouts
        .forget_path(Path::new("/fixture/budget-a"));
    assert_eq!(observer.pool.used.load(Ordering::Relaxed), 0);
    eprintln!(
        "16 × 100,000-cell worker previews: {:?}; admitted {admitted}, charged {bytes} bytes; zero after parked-layout deletion",
        start.elapsed()
    );
}
