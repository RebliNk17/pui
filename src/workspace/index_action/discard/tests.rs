use super::*;
#[test]
fn discard_restores_index_not_head_retains_recovery_and_preserves_index() {
    let root = fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
    let cancel = AtomicBool::new(false);
    apply_stage(&root, &cancel);
    fs::write(root.join("README.md"), "unstaged working content\n").unwrap();
    let index_before = fs::read(root.join(".git/index")).unwrap();
    let snapshot = prepare(&root, "README.md", "MM", Action::Discard, &cancel).unwrap();
    let recovery = snapshot.recovery_path();
    super::super::apply(snapshot, &cancel).unwrap();
    assert_eq!(
        fs::read_to_string(root.join("README.md")).unwrap(),
        "fixture modified README\n"
    );
    assert_eq!(fs::read(&recovery).unwrap(), b"unstaged working content\n");
    assert_eq!(index_before, fs::read(root.join(".git/index")).unwrap());
}
fn apply_stage(root: &Path, cancel: &AtomicBool) {
    super::super::apply(
        prepare(root, "README.md", " M", Action::Stage, cancel).unwrap(),
        cancel,
    )
    .unwrap();
}
#[test]
fn changed_file_or_existing_recovery_refuses_without_replacing() {
    for changed in [true, false] {
        let root = fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
        let cancel = AtomicBool::new(false);
        let snapshot = prepare(&root, "README.md", " M", Action::Discard, &cancel).unwrap();
        if changed {
            fs::write(root.join("README.md"), "newer content").unwrap();
        } else {
            fs::write(snapshot.recovery_path(), "foreign").unwrap();
        }
        let before = fs::read(root.join("README.md")).unwrap();
        assert!(super::super::apply(snapshot, &cancel).is_err());
        assert_eq!(before, fs::read(root.join("README.md")).unwrap());
    }
}
