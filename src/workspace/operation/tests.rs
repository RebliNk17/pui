use super::*;
use std::fs;

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    crate::workspace::tests::git(root, args)
}

fn base_repo() -> PathBuf {
    let root = std::env::temp_dir().join(format!("pui-op-abort-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    git(&root, &["init", "--template=", "-b", "main"]);
    git(&root, &["config", "user.name", "Fixture"]);
    git(&root, &["config", "user.email", "fixture@example.invalid"]);
    fs::write(
        root.join("conflict.txt"),
        "base
",
    )
    .unwrap();
    fs::write(
        root.join("keep.txt"),
        "keep
",
    )
    .unwrap();
    git(&root, &["add", "--all"]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "base"],
    );
    git(&root, &["branch", "other"]);
    fs::write(
        root.join("conflict.txt"),
        "ours
",
    )
    .unwrap();
    git(&root, &["add", "--all"]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "ours"],
    );
    git(&root, &["switch", "-q", "other"]);
    fs::write(
        root.join("conflict.txt"),
        "theirs
",
    )
    .unwrap();
    git(&root, &["add", "--all"]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "theirs"],
    );
    git(&root, &["switch", "-q", "main"]);
    root
}

fn failing_git(root: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap()
        .status;
    assert!(!status.success(), "expected conflict for {args:?}");
}

#[test]
fn merge_rebase_cherry_pick_and_revert_inspect_and_abort_restore_clean_head() {
    let cancel = AtomicBool::new(false);

    // 1. Merge conflict -> inspect -> abort
    let root = base_repo();
    let head_before = git(&root, &["rev-parse", "HEAD"]);
    failing_git(&root, &["merge", "--no-edit", "other"]);
    assert_eq!(active_summary(&root, &cancel).unwrap(), Some(Kind::Merge));
    let review = inspect(&root, &cancel).unwrap();
    assert_eq!(review.kind, Kind::Merge);
    assert_eq!(review.unmerged_files, vec!["conflict.txt".to_owned()]);
    assert!(review.target.is_some());
    abort(review, &cancel).unwrap();
    assert_eq!(active_summary(&root, &cancel).unwrap(), None);
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(
        fs::read(root.join("conflict.txt")).unwrap(),
        b"ours
"
    );
    assert!(git(&root, &["status", "--porcelain=v1", "-z"]).is_empty());

    // 2. Detached rebase conflict -> inspect -> abort
    let root = base_repo();
    let head_before = git(&root, &["rev-parse", "HEAD"]);
    failing_git(&root, &["rebase", "other"]);
    assert_eq!(active_summary(&root, &cancel).unwrap(), Some(Kind::Rebase));
    let review = inspect(&root, &cancel).unwrap();
    assert_eq!(review.kind, Kind::Rebase);
    assert_eq!(review.branch, "detached HEAD");
    assert_eq!(review.unmerged_files, vec!["conflict.txt".to_owned()]);
    abort(review, &cancel).unwrap();
    assert_eq!(active_summary(&root, &cancel).unwrap(), None);
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(
        git(&root, &["symbolic-ref", "HEAD"]),
        b"refs/heads/main
"
    );
    assert_eq!(
        fs::read(root.join("conflict.txt")).unwrap(),
        b"ours
"
    );

    // 3. Cherry-pick conflict -> inspect -> abort
    let root = base_repo();
    let head_before = git(&root, &["rev-parse", "HEAD"]);
    failing_git(&root, &["cherry-pick", "other"]);
    assert_eq!(
        active_summary(&root, &cancel).unwrap(),
        Some(Kind::CherryPick)
    );
    let review = inspect(&root, &cancel).unwrap();
    assert_eq!(review.kind, Kind::CherryPick);
    abort(review, &cancel).unwrap();
    assert_eq!(active_summary(&root, &cancel).unwrap(), None);
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head_before);

    // 4. Revert conflict -> inspect -> abort
    let root = base_repo();
    fs::write(
        root.join("conflict.txt"),
        "third
",
    )
    .unwrap();
    git(&root, &["add", "--all"]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "third"],
    );
    let head_before = git(&root, &["rev-parse", "HEAD"]);
    failing_git(&root, &["revert", "--no-edit", "HEAD~1"]);
    assert_eq!(active_summary(&root, &cancel).unwrap(), Some(Kind::Revert));
    let review = inspect(&root, &cancel).unwrap();
    assert_eq!(review.kind, Kind::Revert);
    abort(review, &cancel).unwrap();
    assert_eq!(active_summary(&root, &cancel).unwrap(), None);
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head_before);
}

#[test]
fn clean_stale_locked_filtered_and_ambiguous_operations_refuse_without_mutating() {
    let cancel = AtomicBool::new(false);
    let clean = base_repo();
    assert_eq!(active_summary(&clean, &cancel).unwrap(), None);
    assert!(inspect(&clean, &cancel).is_err());

    for case in 0..5 {
        let root = base_repo();
        failing_git(&root, &["merge", "--no-edit", "other"]);
        let review = inspect(&root, &cancel).unwrap();
        match case {
            0 => fs::write(
                root.join("conflict.txt"),
                "edited after review
",
            )
            .unwrap(),
            1 => fs::write(
                root.join(".git/MERGE_MSG"),
                "mutated msg
",
            )
            .unwrap(),
            2 => fs::write(root.join(".git/index.lock"), "foreign").unwrap(),
            3 => {
                git(&root, &["config", "filter.bad.smudge", "touch smudge-ran"]);
            }
            _ => {
                fs::write(root.join(".git/CHERRY_PICK_HEAD"), &review.head).unwrap();
            }
        }
        let merge_head_before = fs::read(root.join(".git/MERGE_HEAD")).unwrap();
        assert!(abort(review, &cancel).is_err(), "case {case} must refuse");
        assert_eq!(
            fs::read(root.join(".git/MERGE_HEAD")).unwrap(),
            merge_head_before
        );
        assert!(!root.join("smudge-ran").exists());
        if case == 2 {
            assert_eq!(fs::read(root.join(".git/index.lock")).unwrap(), b"foreign");
        }
    }
}
