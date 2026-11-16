use super::*;
fn fixture() -> (PathBuf, Snapshot) {
    let root = fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
    let cancel = AtomicBool::new(false);
    let existing = prepare(&root, "README.md", " M", Action::Stage, &cancel).unwrap();
    apply(existing, &cancel).unwrap();
    let content = (0..40)
        .map(|n| format!("base line {n}\n"))
        .collect::<String>();
    fs::write(root.join("README.md"), &content).unwrap();
    apply(
        prepare(&root, "README.md", "MM", Action::Stage, &cancel).unwrap(),
        &cancel,
    )
    .unwrap();
    fs::write(
        root.join("README.md"),
        content
            .replace("base line 2\n", "selected first\n")
            .replace("base line 35\n", "selected second\n"),
    )
    .unwrap();
    let snapshot = prepare(&root, "README.md", "MM", Action::Stage, &cancel).unwrap();
    (root, snapshot)
}
#[test]
fn explicit_hunk_only_updates_selected_index_change_worktree_unchanged() {
    let (root, mut snapshot) = fixture();
    assert_eq!(count(&snapshot), 2);
    let patch = extract(&snapshot, 0).unwrap();
    assert!(patch.contains("selected first"));
    assert!(!patch.contains("selected second"));
    let before = crate::workspace::tests::snapshot(&root);
    let file = fs::read(root.join("README.md")).unwrap();
    snapshot.hunk = selection::Selection(Some(1));
    apply(snapshot, &AtomicBool::new(false)).unwrap();
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["show", ":README.md"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("selected first"));
    assert!(text.contains("base line 35"));
    assert!(!text.contains("selected second"));
    assert_eq!(file, fs::read(root.join("README.md")).unwrap());
    assert_eq!(before.2, crate::workspace::tests::snapshot(&root).2);
}
#[test]
fn unstage_one_hunk_preserves_other_staged_changes_and_newer_working_file() {
    let root = fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
    let cancel = AtomicBool::new(false);
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["show", "HEAD:README.md"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let original = String::from_utf8(output.stdout).unwrap();
    let mut lines = original
        .split_inclusive('\n')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert!(lines.len() > 60);
    lines[10] = "first staged marker\n".into();
    lines[50] = "second staged marker\n".into();
    fs::write(root.join("README.md"), lines.concat()).unwrap();
    apply(
        prepare(&root, "README.md", " M", Action::Stage, &cancel).unwrap(),
        &cancel,
    )
    .unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(root.join("README.md"))
        .unwrap()
        .write_all(b"unstaged extra tail\n")
        .unwrap();
    let file = fs::read(root.join("README.md")).unwrap();
    let before = crate::workspace::tests::snapshot(&root);
    let mut selected = prepare(&root, "README.md", "MM", Action::Unstage, &cancel).unwrap();
    assert_eq!(count(&selected), 2);
    selected.hunk = selection::Selection(Some(1));
    apply(selected, &cancel).unwrap();
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["show", ":README.md"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let index = String::from_utf8(out.stdout).unwrap();
    assert!(!index.contains("first staged marker"));
    assert!(index.contains("second staged marker"));
    assert!(!index.contains("unstaged extra tail"));
    assert_eq!(file, fs::read(root.join("README.md")).unwrap());
    assert_eq!(before.2, crate::workspace::tests::snapshot(&root).2);
    // Restage the working file, then unstage both original hunks in one transaction.
    apply(
        prepare(&root, "README.md", "MM", Action::Stage, &cancel).unwrap(),
        &cancel,
    )
    .unwrap();
    let mut selected = prepare(&root, "README.md", "M ", Action::Unstage, &cancel).unwrap();
    assert_eq!(count(&selected), 3);
    selected.hunk = selection::Selection(Some(3));
    apply(selected, &cancel).unwrap();
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["show", ":README.md"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("first staged marker"));
    assert!(!text.contains("second staged marker"));
    assert!(text.contains("unstaged extra tail"));
    assert_eq!(file, fs::read(root.join("README.md")).unwrap());
}
#[test]
fn multihunk_stage_applies_together_and_empty_masks_fail_closed() {
    let (root, mut snapshot) = fixture();
    snapshot.hunk = selection::Selection(Some(3));
    let patch = extract_selection(&snapshot).unwrap();
    assert!(patch.contains("selected first"));
    assert!(patch.contains("selected second"));
    let before = fs::read(root.join("README.md")).unwrap();
    apply(snapshot, &AtomicBool::new(false)).unwrap();
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["show", ":README.md"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, before);
    assert!(selection::Selection(Some(3)).valid(2));
    assert!(!selection::Selection(Some(0)).valid(2));
    assert!(!selection::Selection(Some(4)).valid(2));
    assert!(selection::Selection(Some(u64::MAX)).valid(64));
}
#[test]
fn invalid_hunk_and_changed_content_refuse_without_index_change() {
    for changed in [false, true] {
        let (root, mut snapshot) = fixture();
        let before = fs::read(root.join(".git/index")).unwrap();
        snapshot.hunk = selection::Selection(Some(if changed { 1 } else { 1 << 9 }));
        if changed {
            fs::write(root.join("README.md"), "changed after frozen preview").unwrap();
        }
        assert!(apply(snapshot, &AtomicBool::new(false)).is_err());
        assert_eq!(before, fs::read(root.join(".git/index")).unwrap());
    }
}
