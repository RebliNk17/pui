use super::*;
pub(crate) fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!("pui-identity-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let cancel = AtomicBool::new(false);
    crate::workspace::initialize::apply(
        crate::workspace::initialize::inspect(&root, "main", &cancel).unwrap(),
        &cancel,
    )
    .unwrap();
    root
}
#[test]
fn local_identity_atomic_pair_enables_first_commit_and_refuses_overwrite() {
    let root = fixture();
    let cancel = AtomicBool::new(false);
    let review = inspect(&root, "Fixture 世界", "fixture@example.invalid", &cancel).unwrap();
    apply(review, &cancel).unwrap();
    assert_eq!(
        crate::workspace::tests::git(&root, &["config", "--local", "user.name"]),
        "Fixture 世界\n".as_bytes()
    );
    assert_eq!(
        crate::workspace::tests::git(&root, &["config", "--local", "user.email"]),
        b"fixture@example.invalid\n"
    );
    let before = fs::read(root.join(".git/config")).unwrap();
    assert!(inspect(&root, "Other", "other@example.invalid", &cancel).is_err());
    assert_eq!(fs::read(root.join(".git/config")).unwrap(), before);
    fs::write(root.join("file.txt"), "first\n").unwrap();
    crate::workspace::index_action::apply(
        crate::workspace::index_action::prepare(
            &root,
            "file.txt",
            "??",
            crate::workspace::index_action::Action::Stage,
            &cancel,
        )
        .unwrap(),
        &cancel,
    )
    .unwrap();
    let snapshot = super::super::prepare(&root, &cancel).unwrap();
    assert_eq!(snapshot.author.name, "Fixture 世界");
    super::super::apply(snapshot, "first", &cancel).unwrap();
}
#[test]
fn inherited_include_override_is_explicit_preserves_origin_and_revalidates() {
    let root = fixture();
    let inherited = root.join(".git/inherited-identity");
    fs::write(
        &inherited,
        "[user]\n name = Inherited\n email = inherited@example.invalid\n",
    )
    .unwrap();
    crate::workspace::tests::git(
        &root,
        &["config", "include.path", inherited.to_str().unwrap()],
    );
    let bytes = fs::read(&inherited).unwrap();
    let cancel = AtomicBool::new(false);
    assert!(inspect(&root, "Local", "local@example.invalid", &cancel).is_err());
    let review = inspect_override(&root, "Local", "local@example.invalid", true, &cancel).unwrap();
    assert_eq!(review.inherited.len(), 2);
    apply(review, &cancel).unwrap();
    assert_eq!(fs::read(&inherited).unwrap(), bytes);
    assert_eq!(
        crate::workspace::tests::git(&root, &["config", "--get", "user.name"]),
        b"Local\n"
    );
    assert!(inspect_override(&root, "Other", "other@example.invalid", true, &cancel).is_err());
    let root = fixture();
    let inherited = root.join(".git/inherited-identity");
    fs::write(
        &inherited,
        "[user]\n name = First\n email = first@example.invalid\n",
    )
    .unwrap();
    crate::workspace::tests::git(
        &root,
        &["config", "include.path", inherited.to_str().unwrap()],
    );
    let review = inspect_override(&root, "Local", "local@example.invalid", true, &cancel).unwrap();
    fs::write(
        &inherited,
        "[user]\n name = Changed\n email = changed@example.invalid\n",
    )
    .unwrap();
    assert!(apply(review, &cancel).is_err());
}
#[test]
fn config_change_foreign_lock_and_invalid_inputs_refuse() {
    for case in 0..2 {
        let root = fixture();
        let cancel = AtomicBool::new(false);
        let review = inspect(&root, "Fixture", "fixture@example.invalid", &cancel).unwrap();
        if case == 0 {
            crate::workspace::tests::git(&root, &["config", "test.changed", "true"]);
        } else {
            fs::write(root.join(".git/config.lock"), "foreign").unwrap();
        }
        let before = fs::read(root.join(".git/config")).unwrap();
        assert!(apply(review, &cancel).is_err());
        assert_eq!(fs::read(root.join(".git/config")).unwrap(), before);
        if case == 1 {
            assert_eq!(fs::read(root.join(".git/config.lock")).unwrap(), b"foreign");
        }
    }
    for (name, email) in [
        ("", "a@b"),
        ("bad\0name", "a@b"),
        ("name", "not-email"),
        ("name", "a b@c"),
    ] {
        assert!(!valid(name, email));
    }
}
