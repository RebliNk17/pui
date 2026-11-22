use super::*;
impl Review {
    pub(crate) fn real_fixture_for_ui() -> Self {
        inspect_file(&fixture())
    }
    pub(crate) fn fixture_for_ui(text: &str) -> Self {
        Self {
            root: "/fixture".into(),
            path: "file 世界.txt".into(),
            text: text.into(),
            mode: "100644".into(),
            index_path: "/fixture/.git/index".into(),
            index: vec![],
            branch: "refs/heads/main".into(),
            head: "a".repeat(40),
            operation: vec![],
            permissions: 0o100644,
        }
    }
}
fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    crate::workspace::tests::git(root, args)
}
fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!("pui-resolve-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    git(&root, &["init", "--template=", "-b", "main"]);
    git(&root, &["config", "user.name", "Fixture"]);
    git(&root, &["config", "user.email", "fixture@example.invalid"]);
    for value in ["base\n", "theirs\n", "ours\n"] {
        if value == "theirs\n" {
            git(&root, &["switch", "-c", "other"]);
        }
        if value == "ours\n" {
            git(&root, &["switch", "main"]);
        }
        for name in ["file.txt", "second.txt"] {
            fs::write(root.join(name), value).unwrap();
        }
        git(&root, &["add", "--all"]);
        git(
            &root,
            &["-c", "commit.gpgsign=false", "commit", "-qm", value],
        );
    }
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "merge",
            "--no-edit",
            "other",
        ])
        .output()
        .unwrap()
        .status;
    assert!(!status.success());
    root
}
fn inspect_file(root: &Path) -> Review {
    inspect(root, "file.txt", &AtomicBool::new(false)).unwrap()
}
#[test]
fn save_with_backup_then_fresh_stage_preserves_other_conflicts_head_operation_and_workfile() {
    let root = fixture();
    let review = inspect_file(&root);
    let before_index = fs::read(root.join(".git/index")).unwrap();
    let head = git(&root, &["rev-parse", "HEAD"]);
    let merge = fs::read(root.join(".git/MERGE_HEAD")).unwrap();
    let other = git(&root, &["ls-files", "--stage", "--", "second.txt"]);
    let original = review.text.clone();
    assert!(!valid_resolution(&original));
    assert!(stage(review.clone(), &AtomicBool::new(false)).is_err());
    let draft = "reviewed 世界 resolution\n";
    let backup = save(review.clone(), draft.into(), &AtomicBool::new(false)).unwrap();
    assert_eq!(fs::read(&backup).unwrap(), original.as_bytes());
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), before_index);
    assert_eq!(fs::read(root.join("file.txt")).unwrap(), draft.as_bytes());
    assert!(stage(review, &AtomicBool::new(false)).is_err()); // Saving isn't a publishable index review.
    let fresh = inspect_file(&root);
    stage(fresh, &AtomicBool::new(false)).unwrap();
    assert_eq!(git(&root, &["show", ":file.txt"]), draft.as_bytes());
    assert!(git(&root, &["ls-files", "-u", "--", "file.txt"]).is_empty());
    assert_eq!(
        git(&root, &["ls-files", "--stage", "--", "second.txt"]),
        other
    );
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(root.join(".git/MERGE_HEAD")).unwrap(), merge);
    assert_eq!(fs::read(root.join("file.txt")).unwrap(), draft.as_bytes());
    assert!(inspect(&root, "file.txt", &AtomicBool::new(false)).is_err());
    assert!(!root.join(".git/index.lock").exists());
}
#[test]
fn stale_file_index_head_operation_foreign_lock_and_cancel_never_publish() {
    for case in 0..6 {
        let root = fixture();
        fs::write(root.join("file.txt"), "resolved\n").unwrap();
        let review = inspect_file(&root);
        match case {
            0 => {
                fs::write(root.join("file.txt"), "changed\n").unwrap();
            }
            1 => {
                fs::write(root.join("extra"), "other staged\n").unwrap();
                git(&root, &["add", "extra"]);
            }
            2 => {
                let target = String::from_utf8(git(&root, &["rev-parse", "other"])).unwrap();
                git(&root, &["update-ref", "refs/heads/main", target.trim()]);
            }
            3 => {
                fs::write(root.join(".git/MERGE_MSG"), "different operation\n").unwrap();
            }
            4 => {
                fs::write(root.join(".git/index.lock"), "foreign lock").unwrap();
            }
            _ => {}
        }
        let index = fs::read(root.join(".git/index")).unwrap();
        let current = fs::read(root.join("file.txt")).unwrap();
        let cancel = AtomicBool::new(case == 5);
        assert!(stage(review.clone(), &cancel).is_err(), "case {case}");
        assert!(
            save(review, "new draft\n".into(), &cancel).is_err(),
            "case {case}"
        );
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
        assert_eq!(fs::read(root.join("file.txt")).unwrap(), current);
        if case == 4 {
            assert_eq!(
                fs::read(root.join(".git/index.lock")).unwrap(),
                b"foreign lock"
            );
        } else {
            assert!(!root.join(".git/index.lock").exists());
        }
    }
}
#[test]
fn attributes_filters_markers_binary_bounds_and_symlinks_refuse() {
    let root = fixture();
    let cancel = AtomicBool::new(false);
    let review = inspect_file(&root);
    for draft in [
        "<<<<<<< ours\na\n=======\nb\n>>>>>>> theirs\n",
        "ok\n  ||||||| base\n",
        "not\0text",
    ] {
        assert!(!valid_resolution(draft));
        assert!(save(review.clone(), draft.into(), &cancel).is_err());
    }
    assert!(!valid_resolution(&"é".repeat(32769)));
    assert!(!valid_resolution(&"\n".repeat(2049)));
    assert!(valid_resolution(""));
    fs::write(root.join(".gitattributes"), "file.txt text\n").unwrap();
    assert!(
        inspect(&root, "file.txt", &cancel)
            .unwrap_err()
            .contains("attributes")
    );
    fs::rename(root.join(".gitattributes"), root.join("attrs.saved")).unwrap();
    git(
        &root,
        &["config", "filter.guard.clean", "touch FILTER_EXECUTED"],
    );
    assert!(inspect(&root, "file.txt", &cancel).is_err());
    assert!(!root.join("FILTER_EXECUTED").exists());
    git(&root, &["config", "--unset", "filter.guard.clean"]);
    fs::write(root.join("file.txt"), b"bin\0ary").unwrap();
    assert!(inspect(&root, "file.txt", &cancel).is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("second.txt", root.join("link.txt")).unwrap();
        assert!(inspect(&root, "link.txt", &cancel).is_err());
    }
}
#[test]
fn exact_empty_no_newline_and_unicode_paths_stage_without_interpreting_pathspecs() {
    let root = fixture();
    let cancel = AtomicBool::new(false);
    let initial = inspect_file(&root);
    let stages = git(&root, &["ls-files", "-u", "-z", "--", "file.txt"]);
    let paths = ["-literal 世界[1].txt", " :(glob)*.txt"];
    for (path, content) in paths.into_iter().zip(["", "no final newline"]) {
        let temp = root.join("index-input");
        let mut input = Vec::new();
        for record in stages.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let tab = record.iter().position(|b| *b == b'\t').unwrap();
            input.extend_from_slice(&record[..tab + 1]);
            input.extend_from_slice(path.as_bytes());
            input.push(0);
        }
        fs::write(&temp, &input).unwrap();
        let stdin = fs::File::open(&temp).unwrap();
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["update-index", "-z", "--index-info"])
                .stdin(stdin)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join(path), content).unwrap();
        let review = inspect(&root, path, &cancel).unwrap();
        stage(review, &cancel).unwrap();
        assert_eq!(
            git(&root, &["show", &format!(":{path}")]),
            content.as_bytes()
        );
    }
    assert!(stage(initial, &cancel).is_err());
}
#[cfg(unix)]
#[test]
fn permission_change_and_symlink_parent_refuse_without_touching_target() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = fixture();
    fs::write(root.join("file.txt"), "resolved\n").unwrap();
    let review = inspect_file(&root);
    let index = fs::read(root.join(".git/index")).unwrap();
    let old = fs::metadata(root.join("file.txt"))
        .unwrap()
        .permissions()
        .mode();
    fs::set_permissions(
        root.join("file.txt"),
        fs::Permissions::from_mode(old ^ 0o020),
    )
    .unwrap();
    assert!(stage(review.clone(), &AtomicBool::new(false)).is_err());
    assert!(save(review, "edited\n".into(), &AtomicBool::new(false)).is_err());
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    symlink(&root, root.join("alias")).unwrap();
    assert!(inspect(&root, "alias/file.txt", &AtomicBool::new(false)).is_err());
}
#[test]
fn detached_rebase_conflict_resolves_file_without_advancing_or_continuing() {
    let root = fixture();
    git(&root, &["merge", "--abort"]);
    git(&root, &["switch", "other"]);
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "rebase",
            "--no-autostash",
            "main",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    fs::write(root.join("file.txt"), "rebased resolution\n").unwrap();
    let review = inspect_file(&root);
    assert!(review.branch.is_empty());
    let head = git(&root, &["rev-parse", "HEAD"]);
    let rebase = fs::read(root.join(".git/REBASE_HEAD")).unwrap();
    stage(review, &AtomicBool::new(false)).unwrap();
    assert_eq!(git(&root, &["show", ":file.txt"]), b"rebased resolution\n");
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(root.join(".git/REBASE_HEAD")).unwrap(), rebase);
    assert!(root.join(".git/rebase-merge").is_dir());
    assert!(!git(&root, &["ls-files", "-u", "--", "second.txt"]).is_empty());
}
#[test]
fn linked_worktree_resolution_uses_its_own_index() {
    let source = fixture();
    let root = source.with_file_name(format!("pui-resolve-linked-{}", uuid::Uuid::new_v4()));
    git(
        &source,
        &[
            "worktree",
            "add",
            "-b",
            "linked",
            root.to_str().unwrap(),
            "HEAD",
        ],
    );
    let root = fs::canonicalize(root).unwrap();
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "merge",
            "--no-edit",
            "other",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let source_index = fs::read(source.join(".git/index")).unwrap();
    let review = inspect_file(&root);
    assert!(!review.index_path.starts_with(root.join(".git")));
    let backup = save(review, "linked resolved\n".into(), &AtomicBool::new(false)).unwrap();
    let review = inspect_file(&root);
    assert_eq!(backup.parent(), review.index_path.parent());
    stage(review, &AtomicBool::new(false)).unwrap();
    assert_eq!(git(&root, &["show", ":file.txt"]), b"linked resolved\n");
    assert_eq!(fs::read(source.join(".git/index")).unwrap(), source_index);
}
