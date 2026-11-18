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
    assert_eq!(parsed[0], vec![0, 1]);
    assert_eq!(parsed[1], vec![2, 3]);
    let file = fs::read(root.join("README.md")).unwrap();
    let before = crate::workspace::tests::snapshot(&root);
    snapshot.lines = lines::LineSelection {
        active: true,
        lines: vec![vec![0, 1], vec![]],
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
fn selected_addition_or_deletion_never_includes_opposing_replacement() {
    for selected in [0, 1] {
        let (root, mut snapshot) = fixture();
        let original = crate::workspace::tests::git(&root, &["show", ":README.md"]);
        let work = fs::read(root.join("README.md")).unwrap();
        snapshot.lines = lines::LineSelection {
            active: true,
            lines: vec![vec![selected], vec![]],
        };
        apply(snapshot, &AtomicBool::new(false)).unwrap();
        let index = String::from_utf8(crate::workspace::tests::git(&root, &["show", ":README.md"]))
            .unwrap();
        let base = String::from_utf8(original).unwrap();
        let expected = if selected == 0 {
            base.replace("base line 2\n", "")
        } else {
            base.replace("base line 3\n", "selected first\nbase line 3\n")
        };
        assert_eq!(index, expected);
        assert_eq!(work, fs::read(root.join("README.md")).unwrap());
    }
}
#[test]
fn active_empty_mixed_duplicate_or_cross_hunk_selection_refuses_without_whole_file_fallback() {
    let (root, snapshot) = fixture();
    let original = fs::read(root.join(".git/index")).unwrap();
    for selection in [
        vec![vec![], vec![]],
        vec![vec![0, 0], vec![]],
        vec![vec![3], vec![]],
        vec![vec![0]],
        vec![vec![0], vec![], vec![]],
    ] {
        let mut snap = snapshot.clone();
        snap.lines = lines::LineSelection {
            active: true,
            lines: selection,
        };
        assert!(apply(snap, &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), original);
    }
    let mut snap = snapshot;
    snap.lines = lines::LineSelection {
        active: true,
        lines: vec![vec![0], vec![]],
    };
    snap.hunk = selection::Selection(Some(1));
    assert!(apply(snap, &AtomicBool::new(false)).is_err());
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), original);
}
#[test]
fn unstage_selected_addition_only_preserves_other_staged_changes() {
    let (root, snap) = fixture();
    let cancel = AtomicBool::new(false);
    let head = String::from_utf8(crate::workspace::tests::git(
        &root,
        &["show", "HEAD:README.md"],
    ))
    .unwrap();
    apply(snap, &cancel).unwrap();
    let mut staged = prepare(&root, "README.md", "M ", Action::Unstage, &cancel).unwrap();
    let target = staged
        .preview
        .split_inclusive('\n')
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .position(|l| l.contains("selected first"))
        .unwrap();
    let sets = lines::parse(&staged).unwrap();
    let plus_ids = staged
        .preview
        .split_inclusive('\n')
        .filter(|l| {
            matches!(l.as_bytes()[0], b'+' | b'-') && !l.starts_with("+++") && !l.starts_with("---")
        })
        .enumerate()
        .filter(|(_, l)| l.starts_with('+'))
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    let id = plus_ids[target];
    staged.lines = lines::LineSelection {
        active: true,
        lines: sets
            .iter()
            .map(|set| if set.contains(&id) { vec![id] } else { vec![] })
            .collect(),
    };
    let before = fs::read(root.join("README.md")).unwrap();
    apply(staged, &cancel).unwrap();
    let index =
        String::from_utf8(crate::workspace::tests::git(&root, &["show", ":README.md"])).unwrap();
    assert!(!index.contains("selected first"));
    assert!(index.contains("selected second"));
    assert_ne!(index, head);
    assert_eq!(fs::read(root.join("README.md")).unwrap(), before);
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
