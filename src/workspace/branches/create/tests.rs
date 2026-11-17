use super::*;
#[test]
fn create_from_old_commit_preserves_dirty_checkout_and_refuses_bad_or_stale_start() {
    let root = switch_fixture();
    let git = crate::workspace::tests::git;
    let cancel = AtomicBool::new(false);
    let old = String::from_utf8(git(&root, &["rev-parse", "target"]))
        .unwrap()
        .trim()
        .to_owned();
    std::fs::write(root.join("file.txt"), "dirty retained\n").unwrap();
    let before = crate::workspace::tests::snapshot(&root);
    let review = inspect_start(&root, "from-history", &old, &cancel).unwrap();
    assert_ne!(review.head, review.target);
    apply(review, &cancel).unwrap();
    assert_eq!(
        git(&root, &["rev-parse", "from-history"]),
        format!("{old}\n").as_bytes()
    );
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    assert_eq!(
        std::fs::read(root.join("file.txt")).unwrap(),
        b"dirty retained\n"
    );
    for bad in [
        "HEAD~1".into(),
        "f".repeat(40),
        String::from_utf8(git(&root, &["rev-parse", "HEAD:file.txt"]))
            .unwrap()
            .trim()
            .into(),
    ] {
        assert!(inspect_start(&root, "bad-start", &bad, &cancel).is_err());
    }
    let review = inspect_start(&root, "stale-start", &old, &cancel).unwrap();
    git(&root, &["symbolic-ref", "HEAD", "refs/heads/target"]);
    assert!(apply(review, &cancel).is_err());
}
#[test]
fn exact_create_preserves_current_head_index_files_and_refuses_existing() {
    let root = std::fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
    let cancel = AtomicBool::new(false);
    let before = crate::workspace::tests::snapshot(&root);
    let file = std::fs::read(root.join("README.md")).unwrap();
    let r = inspect(&root, "feature/世界", &cancel).unwrap();
    let head = r.head.clone();
    apply(r, &cancel).unwrap();
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    assert_eq!(std::fs::read(root.join("README.md")).unwrap(), file);
    assert_eq!(
        String::from_utf8(crate::workspace::tests::git(
            &root,
            &["rev-parse", "refs/heads/feature/世界"]
        ))
        .unwrap()
        .trim(),
        head
    );
    assert!(inspect(&root, "feature/世界", &cancel).is_err());
}
fn switch_fixture() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("pui-switch-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let git = crate::workspace::tests::git;
    git(&root, &["init", "--initial-branch=main"]);
    git(&root, &["config", "user.name", "Test"]);
    git(&root, &["config", "user.email", "test@example.invalid"]);
    std::fs::write(root.join("file.txt"), "one\n").unwrap();
    git(&root, &["add", "file.txt"]);
    git(
        &root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-qm",
            "one",
        ],
    );
    git(&root, &["branch", "target"]);
    std::fs::write(root.join("file.txt"), "two\n").unwrap();
    git(&root, &["add", "file.txt"]);
    git(
        &root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-qm",
            "two",
        ],
    );
    root
}
#[test]
fn clean_switch_changes_tracked_content_without_force_and_dirty_or_stale_refuses() {
    let cancel = AtomicBool::new(false);
    let root = switch_fixture();
    let review = inspect_action(&root, "target", true, &cancel).unwrap();
    let target = review.target.clone();
    assert_eq!(
        review.files,
        vec![crate::workspace::history::files::Entry {
            status: 'M',
            path: "file.txt".into()
        }]
    );
    apply(review, &cancel).unwrap();
    assert_eq!(std::fs::read(root.join("file.txt")).unwrap(), b"one\n");
    assert_eq!(
        crate::workspace::commit::branch_head(&root, &cancel, Instant::now() + DEADLINE).unwrap(),
        ("refs/heads/target".into(), target)
    );
    std::fs::write(root.join("file.txt"), "dirty\n").unwrap();
    assert!(inspect_action(&root, "main", true, &cancel).is_err());
    assert_eq!(std::fs::read(root.join("file.txt")).unwrap(), b"dirty\n");
    let root = switch_fixture();
    let review = inspect_action(&root, "target", true, &cancel).unwrap();
    std::fs::write(root.join("untracked"), "preserve").unwrap();
    assert!(apply(review, &cancel).is_err());
    assert_eq!(std::fs::read(root.join("untracked")).unwrap(), b"preserve");
    let root = switch_fixture();
    let review = inspect_action(&root, "target", true, &cancel).unwrap();
    crate::workspace::tests::git(&root, &["update-ref", "refs/heads/target", &review.head]);
    assert!(apply(review, &cancel).is_err());
    assert_eq!(std::fs::read(root.join("file.txt")).unwrap(), b"two\n");
}
#[test]
fn switch_preview_added_deleted_and_same_commit_is_empty() {
    let root = switch_fixture();
    let git = crate::workspace::tests::git;
    git(&root, &["rm", "file.txt"]);
    std::fs::write(root.join("世界.txt"), "new\n").unwrap();
    git(&root, &["add", "世界.txt"]);
    git(
        &root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-qm",
            "replace",
        ],
    );
    let cancel = AtomicBool::new(false);
    let before = crate::workspace::tests::snapshot(&root);
    let review = inspect_action(&root, "target", true, &cancel).unwrap();
    assert_eq!(review.files.len(), 2);
    assert!(
        review
            .files
            .iter()
            .any(|e| e.status == 'A' && e.path == "file.txt")
    );
    assert!(
        review
            .files
            .iter()
            .any(|e| e.status == 'D' && e.path == "世界.txt")
    );
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    assert!(
        inspect_action(&root, "main", true, &cancel)
            .unwrap()
            .files
            .is_empty()
    );
}
#[test]
fn checkout_smudge_filter_refuses_before_execution_or_worktree_change() {
    let root = switch_fixture();
    let before = crate::workspace::tests::snapshot(&root);
    let content = std::fs::read(root.join("file.txt")).unwrap();
    crate::workspace::tests::git(
        &root,
        &["config", "filter.blocked.smudge", "touch filter-marker"],
    );
    let error = inspect_action(&root, "target", true, &AtomicBool::new(false)).unwrap_err();
    assert!(error.contains("smudge"));
    assert!(!root.join("filter-marker").exists());
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    assert_eq!(std::fs::read(root.join("file.txt")).unwrap(), content);
}
#[test]
fn target_submodule_refuses_without_checkout() {
    let root = switch_fixture();
    let git = crate::workspace::tests::git;
    let head = String::from_utf8(git(&root, &["rev-parse", "HEAD"])).unwrap();
    git(
        &root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},sub", head.trim()),
        ],
    );
    git(
        &root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-qm",
            "submodule entry",
        ],
    );
    git(&root, &["branch", "has-submodule"]);
    git(&root, &["reset", "--hard", "HEAD^"]);
    let before = crate::workspace::tests::snapshot(&root);
    let error = inspect_action(&root, "has-submodule", true, &AtomicBool::new(false)).unwrap_err();
    assert!(error.contains("submodule"));
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    assert!(!root.join("sub").exists());
}
#[test]
fn changed_source_cancel_invalid_names_and_foreign_ref_lock_refuse() {
    for name in ["", "-option", "bad..name", "bad.lock", "bad\nname"] {
        assert!(!valid_name(name));
    }
    for case in 0..3 {
        let root = std::fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
        let cancel = AtomicBool::new(false);
        let review = inspect(&root, "created", &cancel).unwrap();
        if case == 0 {
            crate::workspace::tests::git(&root, &["symbolic-ref", "HEAD", "refs/heads/different"]);
        }
        if case == 1 {
            std::fs::write(root.join(".git/refs/heads/created.lock"), "foreign").unwrap();
        }
        assert!(apply(review, &AtomicBool::new(case == 2)).is_err());
        assert!(!root.join(".git/refs/heads/created").exists());
        if case == 1 {
            assert_eq!(
                std::fs::read(root.join(".git/refs/heads/created.lock")).unwrap(),
                b"foreign"
            );
        }
    }
}
