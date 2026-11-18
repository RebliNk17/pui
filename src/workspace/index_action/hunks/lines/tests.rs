use super::*;
fn fixture() -> (PathBuf, Snapshot) {
    let root = fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
    let cancel = AtomicBool::new(false);
    apply(
        prepare(&root, "README.md", " M", Action::Stage, &cancel).unwrap(),
        &cancel,
    )
    .unwrap();
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
fn line_stage_adds_only_selected_additions_and_worktree_unchanged() {
    let (root, mut snapshot) = fixture();
    let parsed = lines::parse(&snapshot).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].len(), 1);
    assert_eq!(parsed[1].len(), 1);
    let file = fs::read(root.join("README.md")).unwrap();
    let before = crate::workspace::tests::snapshot(&root);
    snapshot.lines = lines::LineSelection {
        active: true,
        lines: vec![vec![0], vec![]],
    };
    assert!(lines::synthesize(&snapshot).is_some());
    apply(snapshot, &AtomicBool::new(false)).unwrap();
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["show", ":README.md"])
        .output()
        .unwrap();
    let index = String::from_utf8(out.stdout).unwrap();
    assert!(index.contains("selected first"));
    assert!(index.contains("base line 35"));
    assert!(!index.contains("selected second"));
    assert_eq!(file, fs::read(root.join("README.md")).unwrap());
    assert_eq!(before.2, crate::workspace::tests::snapshot(&root).2);
}
#[test]
fn line_selection_refuses_invalid_ordinal_and_inactive() {
    let (root, snapshot) = fixture();
    let before = fs::read(root.join(".git/index")).unwrap();
    {
        let mut snap = snapshot.clone();
        snap.lines = lines::LineSelection {
            active: true,
            lines: vec![vec![99], vec![]],
        };
        assert!(lines::synthesize(&snap).is_none());
        assert!(apply(snap, &AtomicBool::new(false)).is_err());
    }
    assert_eq!(before, fs::read(root.join(".git/index")).unwrap());
    let mut snap = snapshot;
    snap.lines = lines::LineSelection {
        active: false,
        lines: vec![vec![0]],
    };
    assert!(!snap.lines.any());
    assert!(lines::synthesize(&snap).is_none());
}
