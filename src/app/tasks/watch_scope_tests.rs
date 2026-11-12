//! Real repository/worktree + OS notifications + supervisor revalidation.
use super::*;
use std::path::Path;

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn fixture() -> (Pui, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("pui-watch-scope-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("packages/app")).unwrap();
    std::fs::create_dir(root.join("shared")).unwrap();
    std::fs::write(root.join("packages/app/code"), "app").unwrap();
    std::fs::write(root.join("shared/config"), "shared original").unwrap();
    git(&root, &["init", "-b", "main"]);
    git(&root, &["add", "."]);
    // A fixture commit exists only in this disposable test repository.
    git(
        &root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "fixture",
        ],
    );
    let head = String::from_utf8(git(&root, &["rev-parse", "HEAD"])).unwrap();
    let workspace = domain::workspace::prepare(
        &root.join("packages/app"),
        "watch",
        &root.with_extension("worktrees"),
        head.trim(),
    )
    .unwrap()
    .0;
    let scope = domain::workspace::fingerprint_root(&workspace).unwrap();
    let mut app = app();
    live(&mut app, Phase::Ready);
    passed_attempt(app.tasks.live_mut().unwrap());
    let fingerprint = domain::workspace::fingerprint(&workspace).unwrap();
    let task = app.tasks.live_mut().unwrap();
    task.workspace = Some(workspace.clone());
    task.isolated = true;
    task.attempts[0].fingerprint = Some(fingerprint);
    app.tasks.live = None;
    app.task_watch_source();
    app.tasks.watcher.as_ref().unwrap().wait_ready().unwrap();
    // Revalidate once after registration, closing the setup gap.
    settle(&mut app);
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Ready);
    (app, scope, root)
}
fn settle(app: &mut Pui) {
    app.tasks
        .changes
        .as_ref()
        .unwrap()
        .try_iter()
        .for_each(drop);
    app.tasks.revalidate_at = Some(std::time::Instant::now());
    app.poll_tasks();
    if let Some(job) = app.tasks.job.take() {
        let result = job.rx.recv_timeout(Duration::from_secs(10)).unwrap();
        app.task_job_result(result);
    }
}

#[test]
fn sibling_change_in_subfolder_workflow_notifies_and_marks_evidence_stale() {
    let (mut app, scope, source) = fixture();
    let workspace = app.tasks.current().unwrap().workspace.clone().unwrap();
    assert!(!scope.join("shared/config").starts_with(&workspace));
    assert_ne!(scope, std::fs::canonicalize(&source).unwrap());
    // Clear setup/read notifications before the mutation under test.
    app.tasks
        .changes
        .as_ref()
        .unwrap()
        .try_iter()
        .for_each(drop);
    std::fs::write(scope.join("shared/config"), "shared changed").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    // Drive the real event -> debounce -> off-thread fingerprint path. Do not
    // force revalidation: without the broader watch scope this must time out.
    while app.tasks.current().unwrap().phase == Phase::Ready && std::time::Instant::now() < deadline
    {
        app.poll_tasks();
        std::thread::sleep(Duration::from_millis(20));
    }
    let task = app.tasks.current().unwrap();
    assert_eq!(task.phase, Phase::Stale);
    assert!(task.latest().unwrap().invalidated);
    assert_eq!(
        std::fs::read_to_string(source.join("shared/config")).unwrap(),
        "shared original"
    );
    assert!(app.tasks.processes.is_empty());
    assert!(app.tasks.live.is_none());
}

#[test]
fn subfolder_workflow_ignores_original_checkout_content_for_evidence() {
    let (mut app, scope, source) = fixture();
    let before = app
        .tasks
        .current()
        .unwrap()
        .latest()
        .unwrap()
        .fingerprint
        .clone();
    std::fs::write(source.join("shared/config"), "original checkout changed").unwrap();
    settle(&mut app);
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Ready);
    assert_eq!(
        app.tasks.current().unwrap().latest().unwrap().fingerprint,
        before
    );
    assert_eq!(
        std::fs::read_to_string(scope.join("shared/config")).unwrap(),
        "shared original"
    );
}

#[test]
fn delayed_watch_setup_failure_revalidates_already_completed_evidence() {
    let (mut app, scope, _) = fixture();
    let (release, blocked) = mpsc::channel();
    app.tasks.watcher = Some(crate::file_watch::Watcher::new_resolved(
        &app.ctx,
        move || {
            blocked.recv().unwrap();
            Err("delayed registration failure".into())
        },
        notify::RecursiveMode::Recursive,
        |_: notify::Result<notify::Event>| {},
    ));
    app.tasks.changes = None;
    app.tasks.revalidate_at = None;
    std::fs::write(
        scope.join("shared/config"),
        "changed while setup was pending",
    )
    .unwrap();
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while app.tasks.current().unwrap().phase == Phase::Ready && std::time::Instant::now() < deadline
    {
        app.poll_tasks();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        app.tasks
            .error
            .as_ref()
            .unwrap()
            .contains("Source watching unavailable")
    );
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Stale);
    assert!(app.tasks.current().unwrap().latest().unwrap().invalidated);
}

#[test]
fn same_task_workspace_change_replaces_the_watch_target() {
    let mut app = app();
    live(&mut app, Phase::Ready);
    app.tasks.live = None;
    app.demo = true; // Assert projection without starting native services.
    app.tasks.current_mut().unwrap().workspace = Some("/fixture/one".into());
    app.task_watch_source();
    let first = app.tasks.watched.clone();
    app.tasks.current_mut().unwrap().workspace = Some("/fixture/two".into());
    app.task_watch_source();
    assert_ne!(app.tasks.watched, first);
    assert_eq!(
        app.tasks.watched.as_ref().unwrap().1,
        PathBuf::from("/fixture/two")
    );
}
