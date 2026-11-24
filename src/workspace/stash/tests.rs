use super::*;
use std::fs;

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    crate::workspace::tests::git(root, args)
}

fn repo() -> PathBuf {
    let root = std::env::temp_dir().join(format!("pui-stash-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    git(&root, &["init", "--template=", "-b", "main"]);
    git(&root, &["config", "user.name", "Fixture"]);
    git(&root, &["config", "user.email", "fixture@example.invalid"]);
    fs::write(
        root.join("tracked.txt"),
        "line 1
",
    )
    .unwrap();
    git(&root, &["add", "tracked.txt"]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "initial"],
    );
    root
}

#[test]
fn push_preserves_untracked_files_and_apply_or_drop_enforces_exact_oid_and_clean_tree() {
    let cancel = AtomicBool::new(false);
    let root = repo();
    assert!(list(&root, &cancel).unwrap().is_empty());
    assert!(inspect_push(&root, "clean", &cancel).is_err());

    fs::write(
        root.join("tracked.txt"),
        "modified line 1
",
    )
    .unwrap();
    fs::write(
        root.join("untracked.txt"),
        "keep untracked
",
    )
    .unwrap();

    let push_review = inspect_push(&root, "WIP feature 世界", &cancel).unwrap();
    assert_eq!(push_review.files, vec!["tracked.txt".to_owned()]);
    let oid = apply_push(push_review, &cancel).unwrap();

    // Tracked file reverted to HEAD; untracked file untouched!
    assert_eq!(
        fs::read(root.join("tracked.txt")).unwrap(),
        b"line 1
"
    );
    assert_eq!(
        fs::read(root.join("untracked.txt")).unwrap(),
        b"keep untracked
"
    );

    let entries = list(&root, &cancel).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].index, 0);
    assert_eq!(entries[0].oid, oid);
    assert_eq!(entries[0].selector, "stash@{0}");
    assert!(entries[0].summary.contains("WIP feature 世界"));

    // Apply refuses while untracked.txt is present
    assert!(inspect_entry(&root, 0, &oid, EntryAction::Apply, &cancel).is_err());

    // Remove untracked.txt so worktree is clean -> Apply succeeds and retains stash entry
    fs::remove_file(root.join("untracked.txt")).unwrap();
    let apply_review = inspect_entry(&root, 0, &oid, EntryAction::Apply, &cancel).unwrap();
    assert_eq!(apply_review.files, vec!["tracked.txt".to_owned()]);
    assert!(apply_review.preview.contains("+modified line 1"));
    apply_entry(apply_review, &cancel).unwrap();
    assert_eq!(
        fs::read(root.join("tracked.txt")).unwrap(),
        b"modified line 1
"
    );
    assert_eq!(list(&root, &cancel).unwrap().len(), 1);

    // Drop works even when worktree is dirty, but refuses if another stash shifts top_oid
    let drop_review = inspect_entry(&root, 0, &oid, EntryAction::Drop, &cancel).unwrap();
    let second_push = inspect_push(&root, "second stash", &cancel).unwrap();
    let second_oid = apply_push(second_push, &cancel).unwrap();
    assert_ne!(second_oid, oid);
    assert!(
        apply_entry(drop_review, &cancel).is_err(),
        "stale reflog shift must refuse drop"
    );
    assert_eq!(list(&root, &cancel).unwrap().len(), 2);

    // Fresh drop of stash@{1} removes only that entry
    let fresh_drop = inspect_entry(&root, 1, &oid, EntryAction::Drop, &cancel).unwrap();
    apply_entry(fresh_drop, &cancel).unwrap();
    let remaining = list(&root, &cancel).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].oid, second_oid);
}

#[test]
fn push_refuses_invalid_message_drift_locks_and_filters() {
    let cancel = AtomicBool::new(false);
    let root = repo();
    fs::write(
        root.join("tracked.txt"),
        "change
",
    )
    .unwrap();
    assert!(!valid_message(
        "bad
message"
    ));
    assert!(!valid_message(&"a".repeat(257)));
    assert!(
        inspect_push(
            &root,
            "bad
message",
            &cancel
        )
        .is_err()
    );

    let review = inspect_push(&root, "ok", &cancel).unwrap();
    fs::write(
        root.join("tracked.txt"),
        "drifted
",
    )
    .unwrap();
    assert!(apply_push(review, &cancel).is_err());

    let review = inspect_push(&root, "ok", &cancel).unwrap();
    fs::write(root.join(".git/index.lock"), "locked").unwrap();
    assert!(apply_push(review, &cancel).is_err());
    fs::remove_file(root.join(".git/index.lock")).unwrap();

    git(
        &root,
        &["config", "filter.bad.smudge", "touch smudge-marker"],
    );
    assert!(inspect_push(&root, "ok", &cancel).is_err());
    assert!(!root.join("smudge-marker").exists());
}
