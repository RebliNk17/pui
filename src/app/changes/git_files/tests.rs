use super::*;
use crate::app::files::tests as file_tests;
use std::fs;
fn file(path: &str) -> workspace::FileChange {
    workspace::FileChange {
        path: path.into(),
        status: " M".into(),
        ..Default::default()
    }
}
pub(super) fn workspace() -> Workspace {
    Workspace {
        root: "/fixture".into(),
        branch: "main".into(),
        is_git: true,
        files: vec![file("first.rs"), file("second.rs"), file("third.rs")],
        ..Default::default()
    }
}
pub(super) fn changes(ctx: &egui::Context, workspace: &Workspace) -> Changes {
    let mut changes = Changes::default();
    changes.open_review(
        ctx,
        ("first.rs".into(), "@@ -1 +1 @@\n-old\n+kept\n".into()).into(),
        false,
        &workspace.root,
        "chat",
    );
    changes.bind_git_files(workspace);
    changes
}
pub(super) fn inject(
    changes: &mut Changes,
    workspace: &Workspace,
    target: usize,
) -> mpsc::Sender<Loaded> {
    let (tx, rx) = mpsc::channel();
    changes.git_files.pending = Some(Pending {
        rx: rx.into(),
        target,
        owner: changes.review.as_ref().unwrap().2.focus_id(),
    });
    let _ = workspace;
    tx
}
pub(super) fn loaded(
    workspace: &Workspace,
    index: usize,
    result: Result<String, String>,
) -> Loaded {
    Loaded {
        root: workspace.root.clone(),
        file: workspace.files[index].clone(),
        list: List::new(workspace),
        result,
    }
}
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        pressed: true,
        repeat: false,
        physical_key: None,
    }
}

#[test]
fn bounded_metadata_exact_pathspec_membership_and_changed_root_branch_status_are_rejected() {
    let workspace = workspace();
    let list = List::new(&workspace).unwrap();
    assert!(list.matches(&workspace));
    for path in ["", "../outside", "/absolute", "file\nname", "a/../b"] {
        assert!(!valid_path(path));
    }
    assert!(valid_path("literal[abc]*.rs"));
    let mut large = workspace.clone();
    large.files.resize(MAX_FILES + 1, file("x"));
    assert!(List::new(&large).is_none());
    let mut large = workspace.clone();
    large.files = (0..20).map(|_| file(&"x".repeat(4096))).collect();
    assert!(List::new(&large).is_none());
    for kind in 0..4 {
        let mut changed = workspace.clone();
        match kind {
            0 => changed.root = "/different".into(),
            1 => changed.branch = "other".into(),
            2 => changed.files[0].status = "??".into(),
            _ => changed.files.swap(0, 1),
        };
        assert!(!list.matches(&changed));
        assert!(
            loaded(&workspace, 1, Ok("+new".into()))
                .finish(&changed)
                .is_none()
        );
    }
}

#[test]
fn delayed_result_requires_same_document_owner_and_retains_old_body_on_failure() {
    let ctx = egui::Context::default();
    let workspace = workspace();
    let mut changes = changes(&ctx, &workspace);
    let tx = inject(&mut changes, &workspace, 1);
    tx.send(loaded(&workspace, 1, Err("fixture unavailable".into())))
        .unwrap();
    changes.poll_git_files(&ctx, &workspace);
    assert_eq!(changes.review.as_ref().unwrap().0, "first.rs");
    assert!(
        changes
            .git_files
            .error
            .as_ref()
            .unwrap()
            .contains("fixture")
    );
    let tx = inject(&mut changes, &workspace, 1);
    let mut replacement: diff_viewer::Document = ("first.rs".into(), "+replacement".into()).into();
    replacement.3 = false;
    changes.review = Some(replacement);
    tx.send(loaded(&workspace, 1, Ok("+late".into()))).unwrap();
    changes.poll_git_files(&ctx, &workspace);
    assert_eq!(changes.review.as_ref().unwrap().1, "+replacement");
    let _tx = inject(&mut changes, &workspace, 1);
    changes.invalidate();
    assert!(changes.git_files.stale && changes.git_files.pending.is_none());
}

#[test]
fn delayed_success_never_steals_composer_focus_and_back_or_session_change_cancels_request() {
    let ctx = egui::Context::default();
    let workspace = workspace();
    let mut changes = changes(&ctx, &workspace);
    let tx = inject(&mut changes, &workspace, 1);
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
    tx.send(loaded(&workspace, 1, Ok("+actual second".into())))
        .unwrap();
    changes.poll_git_files(&ctx, &workspace);
    assert_eq!(changes.review.as_ref().unwrap().0, "second.rs");
    assert!(!changes.review.as_ref().unwrap().3);
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new("composer"))));
    let _tx = inject(&mut changes, &workspace, 2);
    changes.bind(&workspace.root, "other chat");
    assert!(changes.review.is_none() && changes.git_files.pending.is_none());
}

#[test]
fn one_context_gate_caps_workers_and_untracked_never_reads_file_or_stages() {
    let ctx = egui::Context::default();
    let mut workspace = workspace();
    workspace.files[0].status = "??".into();
    let gate = ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<AtomicBool>>(egui::Id::new("git-review-worker"))
            .clone()
    });
    gate.store(true, Ordering::Relaxed);
    for _ in 0..1024 {
        assert!(start_inline(&ctx, &workspace.root, workspace.files[0].clone()).is_none());
    }
    let rx = start(&ctx, &workspace, workspace.files[0].clone());
    let result = rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .finish(&workspace)
        .unwrap()
        .err()
        .unwrap();
    assert!(result.contains("busy"));
    gate.store(false, Ordering::Release);
    let rx = start(&ctx, &workspace, workspace.files[0].clone());
    let result = rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .finish(&workspace)
        .unwrap()
        .unwrap();
    assert!(result.1.contains("Untracked"));
    let inline = start_inline(&ctx, &workspace.root, workspace.files[0].clone()).unwrap();
    assert!(
        inline
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap()
            .contains("Untracked")
    );
    assert!(!gate.load(Ordering::Acquire));
}

#[test]
fn inline_rows_wait_without_spawning_or_caching_busy_errors_while_shared_worker_reads() {
    let ctx = egui::Context::default();
    let workspace = workspace();
    let (tx, release) = mpsc::channel();
    let active = spawn_read(&ctx, permit(&ctx).unwrap(), move |_| {
        release.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    let mut changes = Changes::default();
    for index in 0..1024 {
        changes.load_diff(&ctx, &workspace.root, file(&format!("file-{index}.rs")));
    }
    assert!(changes.diffs.is_empty());
    let busy = start(&ctx, &workspace, workspace.files[0].clone())
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .finish(&workspace)
        .unwrap()
        .unwrap_err();
    assert!(busy.contains("busy"));
    tx.send(()).unwrap();
    active.recv_timeout(Duration::from_secs(2)).unwrap();
    changes.load_diff(&ctx, &workspace.root, workspace.files[0].clone());
    let result = changes
        .diffs
        .get_mut("first.rs")
        .unwrap()
        .rx
        .take()
        .unwrap()
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert!(result.is_err()); // Missing fixture root is reported, never a partial patch.
}

#[test]
fn read_only_real_git_diff_has_literal_pathspec_and_bounded_stdout_without_commits() {
    let root = workspace::tests::fixture();
    let ctx = egui::Context::default();
    let mut workspace = workspace::inspect(&root);
    workspace.files = vec![file("README.md")];
    for (bytes, expected) in [
        (
            b"fixture modified README\n".to_vec(),
            Ok("+fixture modified README"),
        ),
        (
            vec![b'x'; workspace::MAX_DIFF_BYTES + 1],
            Err("200,000-byte"),
        ),
        (b"invalid utf8: \xff\n".to_vec(), Err("not UTF-8")),
    ] {
        fs::write(root.join("README.md"), bytes).unwrap();
        let before = workspace::tests::snapshot(&root);
        let result = start(&ctx, &workspace, file("README.md"))
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .finish(&workspace)
            .unwrap();
        let inline = start_inline(&ctx, &root, file("README.md"))
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert_eq!(inline, result.clone().map(|(_, body)| body));
        match expected {
            Ok(expected) => assert!(result.unwrap().1.contains(expected)),
            Err(expected) => assert!(result.unwrap_err().contains(expected)),
        }
        assert_eq!(workspace::tests::snapshot(&root), before);
    }
}

#[test]
fn keyboard_navigation_uses_real_pending_read_and_preserves_approval_draft_and_hidden_owner() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    app.prefs.hide_sidebar = true;
    app.workspace = workspace();
    for file in &mut app.workspace.files {
        file.status = "??".into();
    }
    app.open_diff_review(("first.rs".into(), "+original".into()).into(), false);
    app.state.busy = true;
    app.draft = "Keep".into();
    app.state.dialogs.push_back(json!({"id":"git-review-permission","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."}));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    for _ in 0..4 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    file_tests::frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::ArrowRight, egui::Modifiers::ALT),
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
        ],
    );
    if let Some(pending) = app.changes.git_files.pending.as_mut() {
        let loaded = pending.rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let (tx, rx) = mpsc::channel();
        tx.send(loaded).unwrap();
        pending.rx = rx.into();
    }
    file_tests::frame(&ctx, &mut app, vec![]);
    assert_eq!(app.changes.review.as_ref().unwrap().0, "second.rs");
    assert!(app.changes.review.as_ref().unwrap().1.contains("Untracked"));
    assert_eq!(app.draft, "Keep");
    assert!(app.state.busy && app.state.dialogs.len() == 1 && app.pending_dialog.is_none());
    assert!(commands.try_recv().is_err());
    app.settings_open = true;
    file_tests::frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::ArrowRight, egui::Modifiers::ALT)],
    );
    assert_eq!(app.changes.review.as_ref().unwrap().0, "second.rs");
}

#[test]
fn bounded_git_list_metadata_validation_has_no_patch_bodies_or_idle_git_poll() {
    let mut workspace = workspace();
    workspace.files = (0..MAX_FILES)
        .map(|n| file(&format!("src/{n}.rs")))
        .collect();
    let start = std::time::Instant::now();
    let list = List::new(&workspace).unwrap();
    for _ in 0..10_000 {
        assert!(list.matches(&workspace));
    }
    let capacity = list.files.capacity() * std::mem::size_of::<workspace::FileChange>()
        + list
            .files
            .iter()
            .map(|file| file.path.capacity() + file.status.capacity())
            .sum::<usize>();
    eprintln!(
        "256-file Git metadata snapshot {capacity}B; 10k unchanged in-memory comparisons {:?}; no bodies or Git calls",
        start.elapsed()
    );
}
