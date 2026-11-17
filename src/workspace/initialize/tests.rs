use super::*;
fn folder() -> PathBuf {
    let root = std::env::temp_dir().join(format!("pui-init-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    fs::canonicalize(root).unwrap()
}
#[test]
fn initialize_preserves_files_no_templates_and_first_stage_commit_works() {
    let root = folder();
    fs::write(root.join("first.txt"), "first\n").unwrap();
    let cancel = AtomicBool::new(false);
    let review = inspect(&root, "main", &cancel).unwrap();
    assert!(!root.join(".git").exists());
    apply(review, &cancel).unwrap();
    assert_eq!(fs::read(root.join("first.txt")).unwrap(), b"first\n");
    assert!(!root.join(".git/index").exists());
    assert!(!root.join(".git/hooks/pre-commit.sample").exists());
    assert_eq!(
        super::super::tests::git(&root, &["symbolic-ref", "HEAD"]),
        b"refs/heads/main\n"
    );
    super::super::tests::git(&root, &["config", "user.name", "Fixture"]);
    super::super::tests::git(&root, &["config", "user.email", "fixture@example.invalid"]);
    let stage = super::super::index_action::prepare(
        &root,
        "first.txt",
        "??",
        super::super::index_action::Action::Stage,
        &cancel,
    )
    .unwrap();
    super::super::index_action::apply(stage, &cancel).unwrap();
    let snapshot = super::super::commit::prepare(&root, &cancel).unwrap();
    super::super::commit::apply(snapshot, "initial native flow", &cancel).unwrap();
    assert_eq!(
        super::super::tests::git(&root, &["show", "HEAD:first.txt"]),
        b"first\n"
    );
}
#[test]
fn nested_existing_concurrent_git_and_precancel_refuse() {
    let root = folder();
    let cancel = AtomicBool::new(false);
    let review = inspect(&root, "main", &cancel).unwrap();
    fs::create_dir(root.join(".git")).unwrap();
    fs::write(root.join(".git/marker"), "foreign").unwrap();
    assert!(apply(review, &cancel).is_err());
    assert_eq!(fs::read(root.join(".git/marker")).unwrap(), b"foreign");
    let root = folder();
    assert!(
        apply(
            inspect(&root, "main", &cancel).unwrap(),
            &AtomicBool::new(true)
        )
        .is_err()
    );
    assert!(!root.join(".git").exists());
    apply(inspect(&root, "main", &cancel).unwrap(), &cancel).unwrap();
    let child = root.join("child");
    fs::create_dir(&child).unwrap();
    assert!(inspect(&child, "main", &cancel).is_err());
    assert!(!child.join(".git").exists());
}
