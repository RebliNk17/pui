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
#[test]
fn restore_recovery_puts_back_original_working_content_and_keeps_copy() {
    let root = fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
    let cancel = AtomicBool::new(false);
    apply_stage(&root, &cancel);
    fs::write(root.join("README.md"), "unstaged working content\n").unwrap();
    let snapshot = prepare(&root, "README.md", "MM", Action::Discard, &cancel).unwrap();
    let recovery = snapshot.recovery_path();
    super::super::apply(snapshot.clone(), &cancel).unwrap();
    assert_eq!(
        fs::read_to_string(root.join("README.md")).unwrap(),
        "fixture modified README\n"
    );
    assert!(super::super::recovery_available(&snapshot));
    // The recovery copy is content-bound to this path; restore overwrites the
    // working file even though it changed after the discard.
    fs::write(root.join("README.md"), "changed after discard\n").unwrap();
    super::super::restore(&root, "README.md", &recovery, &cancel).unwrap();
    assert_eq!(
        fs::read_to_string(root.join("README.md")).unwrap(),
        "unstaged working content\n"
    );
    assert_eq!(fs::read(&recovery).unwrap(), b"unstaged working content\n");
}
#[test]
fn restore_refuses_missing_copy_and_foreign_status() {
    let root = fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
    let cancel = AtomicBool::new(false);
    let snapshot = prepare(&root, "README.md", " M", Action::Discard, &cancel).unwrap();
    assert!(!super::super::recovery_available(&snapshot));
    let before = fs::read(root.join("README.md")).unwrap();
    assert!(super::super::restore(&root, "README.md", &snapshot.recovery_path(), &cancel).is_err());
    assert_eq!(before, fs::read(root.join("README.md")).unwrap());
}
