use super::*;
fn snapshot(root: &Path) -> Snapshot {
    prepare(
        root,
        "README.md",
        " M",
        Action::Stage,
        &AtomicBool::new(false),
    )
    .unwrap()
}
fn initial_fixture() -> PathBuf {
    static SEED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let seed = SEED.get_or_init(|| {
        let root =
            std::env::temp_dir().join(format!("pui-first-stage-seed-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        super::super::tests::git(&root, &["init", "--template=", "--initial-branch=main"]);
        super::super::tests::git(&root, &["config", "user.name", "Pui Test"]);
        super::super::tests::git(&root, &["config", "user.email", "pui@example.invalid"]);
        fs::write(root.join("first.txt"), "first\n").unwrap();
        root
    });
    let root = std::env::temp_dir().join(format!("pui-first-stage-{}", uuid::Uuid::new_v4()));
    super::super::tests::copy_tree(seed, &root);
    fs::canonicalize(root).unwrap()
}
#[test]
fn no_index_stage_unstage_restage_and_initial_commit_preserve_working_file() {
    let root = initial_fixture();
    let cancel = AtomicBool::new(false);
    assert!(!root.join(".git/index").exists());
    let visible = super::super::read(&root, &cancel);
    assert!(visible.error.is_none(), "{:?}", visible.error);
    assert!(
        visible
            .files
            .iter()
            .any(|f| f.path == "first.txt" && f.status == "??")
    );
    let stage = prepare(&root, "first.txt", "??", Action::Stage, &cancel).unwrap();
    assert!(stage.index.is_empty());
    apply(stage, &cancel).unwrap();
    let visible = super::super::read(&root, &cancel);
    assert!(visible.error.is_none(), "{:?}", visible.error);
    assert!(
        visible
            .files
            .iter()
            .any(|f| f.path == "first.txt" && f.status == "A ")
    );
    let unstage = prepare(&root, "first.txt", "A ", Action::Unstage, &cancel).unwrap();
    assert!(unstage.preview.contains("+first"));
    apply(unstage, &cancel).unwrap();
    let stage = prepare(&root, "first.txt", "??", Action::Stage, &cancel).unwrap();
    apply(stage, &cancel).unwrap();
    let commit = super::super::commit::prepare(&root, &cancel).unwrap();
    super::super::commit::apply(commit, "first commit entirely native", &cancel).unwrap();
    assert_eq!(fs::read(root.join("first.txt")).unwrap(), b"first\n");
    assert_eq!(
        super::super::tests::git(&root, &["show", "HEAD:first.txt"]),
        b"first\n"
    );
}
#[test]
fn absent_index_snapshot_refuses_new_index_foreign_lock_and_branch_creation() {
    for case in 0..3 {
        let root = initial_fixture();
        let cancel = AtomicBool::new(false);
        let stage = prepare(&root, "first.txt", "??", Action::Stage, &cancel).unwrap();
        if case == 0 {
            super::super::tests::git(&root, &["add", "--", "first.txt"]);
        }
        if case == 1 {
            fs::write(root.join(".git/index.lock"), "foreign").unwrap();
        }
        if case == 2 {
            super::super::tests::git(&root, &["symbolic-ref", "HEAD", "refs/heads/other"]);
        }
        assert!(apply(stage, &cancel).is_err());
        assert_eq!(fs::read(root.join("first.txt")).unwrap(), b"first\n");
        if case == 1 {
            assert_eq!(fs::read(root.join(".git/index.lock")).unwrap(), b"foreign");
            assert!(!root.join(".git/index").exists());
        }
    }
}
#[test]
fn stage_unstage_changes_only_disposable_index_and_preserves_head_worktree() {
    let root = fs::canonicalize(super::super::tests::fixture()).unwrap();
    let before = super::super::tests::snapshot(&root);
    let file = fs::read(root.join("README.md")).unwrap();
    let action = snapshot(&root);
    apply(action, &AtomicBool::new(false)).unwrap();
    let staged = super::super::tests::snapshot(&root);
    assert_ne!(before.1, staged.1);
    assert_eq!(before.2, staged.2);
    assert_eq!(file, fs::read(root.join("README.md")).unwrap());
    let action = prepare(
        &root,
        "README.md",
        "M ",
        Action::Unstage,
        &AtomicBool::new(false),
    )
    .unwrap();
    apply(action, &AtomicBool::new(false)).unwrap();
    assert_eq!(before.0, super::super::tests::snapshot(&root).0);
    assert_eq!(file, fs::read(root.join("README.md")).unwrap());
}
#[test]
fn changed_file_cancel_and_foreign_index_lock_refuse_without_overwrite() {
    for case in 0..3 {
        let root = fs::canonicalize(super::super::tests::fixture()).unwrap();
        let action = snapshot(&root);
        let before = fs::read(&action.index_path).unwrap();
        let lock = action.index_path.with_file_name("index.lock");
        if case == 0 {
            fs::write(root.join("README.md"), "changed after inspection").unwrap();
        }
        if case == 1 {
            fs::write(&lock, "foreign lock").unwrap();
        }
        assert!(apply(action, &AtomicBool::new(case == 2)).is_err());
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), before);
        if case == 1 {
            assert_eq!(fs::read(&lock).unwrap(), b"foreign lock");
        }
    }
}
#[test]
fn partially_staged_unstage_preserves_worktree_and_unrelated_entries() {
    let root = fs::canonicalize(super::super::tests::fixture()).unwrap();
    let cancel = AtomicBool::new(false);
    apply(snapshot(&root), &cancel).unwrap();
    fs::write(root.join("README.md"), "working second version\n").unwrap();
    let file = fs::read(root.join("README.md")).unwrap();
    let action = prepare(&root, "README.md", "MM", Action::Unstage, &cancel).unwrap();
    apply(action, &cancel).unwrap();
    assert_eq!(fs::read(root.join("README.md")).unwrap(), file);
    let fresh = prepare(&root, "README.md", " M", Action::Stage, &cancel).unwrap();
    assert_eq!(fresh.status, " M");
}
#[test]
fn index_or_head_change_since_inspection_refuses_original_snapshot() {
    let root = fs::canonicalize(super::super::tests::fixture()).unwrap();
    let action = snapshot(&root);
    let mut index = fs::read(&action.index_path).unwrap();
    index.push(0);
    fs::write(&action.index_path, &index).unwrap();
    assert!(apply(action, &AtomicBool::new(false)).is_err());
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
}
#[test]
fn effective_filter_drivers_are_refused_before_any_index_write() {
    let root = fs::canonicalize(super::super::tests::fixture()).unwrap();
    let config = root.join(".git/config");
    let mut content = fs::read_to_string(&config).unwrap();
    content.push_str("\n[filter \"blocked\"]\n clean = touch /unused-pui-filter-marker\n");
    fs::write(config, content).unwrap();
    let before = fs::read(root.join(".git/index")).unwrap();
    assert!(
        prepare(
            &root,
            "README.md",
            " M",
            Action::Stage,
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), before);
}
#[test]
fn staged_and_unstaged_patches_separate_index_and_worktree_versions_without_mutation() {
    let root = fs::canonicalize(super::super::tests::fixture()).unwrap();
    let cancel = AtomicBool::new(false);
    apply(snapshot(&root), &cancel).unwrap();
    fs::write(root.join("README.md"), "only unstaged second version\n").unwrap();
    let file = FileChange {
        path: "README.md".into(),
        status: "MM".into(),
        ..Default::default()
    };
    let before = super::super::tests::snapshot(&root);
    let staged = super::super::diff_mode(&root, &file, &cancel, DiffMode::Staged).unwrap();
    let unstaged = super::super::diff_mode(&root, &file, &cancel, DiffMode::Unstaged).unwrap();
    assert!(staged.contains("+fixture modified README"));
    assert!(!staged.contains("only unstaged"));
    assert!(unstaged.contains("-fixture modified README"));
    assert!(unstaged.contains("+only unstaged second version"));
    assert_eq!(before, super::super::tests::snapshot(&root));
}
#[test]
fn confirmation_preview_distinguishes_mm_stage_and_unstage_and_is_frozen() {
    let root = fs::canonicalize(super::super::tests::fixture()).unwrap();
    let cancel = AtomicBool::new(false);
    let first = snapshot(&root);
    assert!(first.preview.contains("+fixture modified README"));
    apply(first, &cancel).unwrap();
    fs::write(root.join("README.md"), "second working content\n").unwrap();
    let stage = prepare(&root, "README.md", "MM", Action::Stage, &cancel).unwrap();
    let unstage = prepare(&root, "README.md", "MM", Action::Unstage, &cancel).unwrap();
    assert!(stage.preview.contains("+second working content"));
    assert!(stage.preview.contains("-fixture modified README"));
    assert!(!unstage.preview.contains("second working content"));
    assert!(unstage.preview.contains("+fixture modified README"));
    let before = fs::read(root.join(".git/index")).unwrap();
    let mut forged = unstage;
    forged.preview.push_str("not confirmed");
    assert!(apply(forged, &cancel).is_err());
    assert_eq!(before, fs::read(root.join(".git/index")).unwrap());
}
#[test]
fn new_file_preview_exact_and_oversize_binary_controls_refuse_whole() {
    let root = fs::canonicalize(super::super::tests::fixture()).unwrap();
    let cancel = AtomicBool::new(false);
    let before = fs::read(root.join(".git/index")).unwrap();
    for content in [
        b"literal \"quotes\" =1+2\n".to_vec(),
        vec![b'x'; 131073],
        b"bad\0binary".to_vec(),
        "x\n".repeat(1025).into_bytes(),
    ] {
        fs::write(root.join("preview-new.txt"), &content).unwrap();
        let result = prepare(&root, "preview-new.txt", "??", Action::Stage, &cancel);
        if content.starts_with(b"literal") {
            assert_eq!(result.unwrap().preview, String::from_utf8(content).unwrap());
        } else {
            assert!(result.is_err());
        }
    }
    assert_eq!(before, fs::read(root.join(".git/index")).unwrap());
}
#[test]
fn statuses_fail_closed_for_rename_conflict_type_submodule_like_and_invalid_paths() {
    for status in ["R ", "UU", " T", "!!", " D!"] {
        assert!(!available(status, Action::Stage));
        assert!(!available(status, Action::Unstage));
    }
    assert!(available("MM", Action::Stage));
    assert!(available("MM", Action::Unstage));
    assert!(
        prepare(
            Path::new("/unused"),
            "../x",
            " M",
            Action::Stage,
            &AtomicBool::new(false)
        )
        .is_err()
    );
}
