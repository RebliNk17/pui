use super::*;
use std::fs;

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    crate::workspace::tests::git(root, args)
}

fn repo() -> PathBuf {
    let root = std::env::temp_dir().join(format!("pui-recmgr-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    git(&root, &["init", "--template=", "-b", "main"]);
    git(&root, &["config", "user.name", "Fixture"]);
    git(&root, &["config", "user.email", "fixture@example.invalid"]);
    fs::write(
        root.join("notes.txt"),
        "committed baseline
",
    )
    .unwrap();
    fs::write(
        root.join("conflict.txt"),
        "base conflict
",
    )
    .unwrap();
    git(&root, &["add", "--all"]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "base"],
    );
    root
}

#[test]
fn discovers_discard_and_conflict_backups_matches_paths_and_restores_or_deletes_with_hash_guards() {
    let cancel = AtomicBool::new(false);
    let root = repo();
    let git_dir = root.join(".git");

    let discard_bytes = "discarded working draft 世界\n".as_bytes();
    let discard_name = discard_recovery_name("notes.txt", discard_bytes);
    let discard_path = git_dir.join(&discard_name);
    fs::write(&discard_path, discard_bytes).unwrap();

    let conflict_bytes = b"<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\n";
    let conflict_name = conflict_recovery_name("conflict.txt", conflict_bytes);
    let conflict_path = git_dir.join(&conflict_name);
    fs::write(&conflict_path, conflict_bytes).unwrap();

    // Unrelated file in .git must be ignored
    fs::write(git_dir.join("pui-discard-recovery-nothex"), b"ignored").unwrap();

    let summary = list(&root, &cancel).unwrap();
    assert!(!summary.truncated);
    assert_eq!(summary.entries.len(), 2);

    let conflict_entry = summary
        .entries
        .iter()
        .find(|e| e.kind == Kind::Conflict)
        .unwrap();
    assert_eq!(conflict_entry.matched_path.as_deref(), Some("conflict.txt"));

    let discard_entry = summary
        .entries
        .iter()
        .find(|e| e.kind == Kind::Discard)
        .unwrap();
    assert_eq!(discard_entry.matched_path.as_deref(), Some("notes.txt"));
    assert!(
        discard_entry
            .preview
            .contains("discarded working draft 世界")
    );

    // Wrong target path must refuse cryptographic hash check
    assert!(inspect_restore(&root, &discard_path, "conflict.txt", &cancel).is_err());

    // Restore over modified working file backs up current working content and restores saved bytes
    fs::write(
        root.join("notes.txt"),
        "current working edit
",
    )
    .unwrap();
    let restore_review = inspect_restore(&root, &discard_path, "notes.txt", &cancel).unwrap();
    apply_restore(restore_review, &cancel).unwrap();
    assert_eq!(fs::read(root.join("notes.txt")).unwrap(), discard_bytes);
    assert!(
        discard_path.exists(),
        "restore never auto-deletes recovery copy"
    );

    // Current working edit was backed up as a third recovery file!
    let summary_after = list(&root, &cancel).unwrap();
    assert_eq!(summary_after.entries.len(), 3);

    // Delete refuses if file content changes after review
    let del_review = inspect_delete(&root, &conflict_path, &cancel).unwrap();
    fs::write(&conflict_path, b"tampered").unwrap();
    assert!(apply_delete(del_review, &cancel).is_err());
    assert!(conflict_path.exists());

    // Fresh delete removes only the reviewed recovery file
    let fresh_del = inspect_delete(&root, &conflict_path, &cancel).unwrap();
    apply_delete(fresh_del, &cancel).unwrap();
    assert!(!conflict_path.exists());
    assert_eq!(list(&root, &cancel).unwrap().entries.len(), 2);
}
