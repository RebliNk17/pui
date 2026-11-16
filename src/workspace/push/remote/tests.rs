use super::*;
#[test]
fn creates_remote_preserves_other_config_and_refuses_existing_or_foreign_lock() {
    let root = std::fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
    let cancel = AtomicBool::new(false);
    let before = crate::workspace::tests::snapshot(&root);
    create(
        &root,
        "native",
        "git@example.invalid:team/repo.git",
        &cancel,
    )
    .unwrap();
    assert_eq!(
        crate::workspace::tests::git(&root, &["config", "remote.native.url"]),
        b"git@example.invalid:team/repo.git\n"
    );
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    let config = fs::read(root.join(".git/config")).unwrap();
    assert!(create(&root, "native", "https://other.invalid/repo", &cancel).is_err());
    assert_eq!(fs::read(root.join(".git/config")).unwrap(), config);
    fs::write(root.join(".git/config.lock"), "foreign").unwrap();
    assert!(create(&root, "second", "https://host.invalid/repo", &cancel).is_err());
    assert_eq!(fs::read(root.join(".git/config.lock")).unwrap(), b"foreign");
    assert_eq!(fs::read(root.join(".git/config")).unwrap(), config);
}
#[test]
fn upstream_from_explicit_review_becomes_default_and_never_overwrites() {
    let root = std::fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
    let cancel = AtomicBool::new(false);
    let branch = String::from_utf8(crate::workspace::tests::git(
        &root,
        &["symbolic-ref", "--short", "HEAD"],
    ))
    .unwrap();
    let _ = std::process::Command::new("git")
        .current_dir(&root)
        .args([
            "config",
            "--remove-section",
            &format!("branch.{}", branch.trim()),
        ])
        .output()
        .unwrap();
    crate::workspace::tests::git(
        &root,
        &[
            "config",
            "remote.origin.url",
            "https://example.invalid/repo",
        ],
    );
    let s = prepare_target(&root, Some(("origin", "refs/heads/chosen")), &cancel).unwrap();
    let before = crate::workspace::tests::snapshot(&root);
    set_upstream(&s, &cancel).unwrap();
    assert_eq!(
        crate::workspace::tests::git(
            &root,
            &["config", &format!("branch.{}.remote", branch.trim())]
        ),
        b"origin\n"
    );
    assert_eq!(
        crate::workspace::tests::git(
            &root,
            &["config", &format!("branch.{}.merge", branch.trim())]
        ),
        b"refs/heads/chosen\n"
    );
    assert_eq!(
        prepare(&root, &cancel).unwrap().destination,
        "refs/heads/chosen"
    );
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    let config = fs::read(root.join(".git/config")).unwrap();
    assert!(set_upstream(&s, &cancel).is_err());
    assert_eq!(fs::read(root.join(".git/config")).unwrap(), config);
}
#[test]
fn remote_input_validation_and_cancel_leave_config_unchanged() {
    let root = std::fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
    let before = fs::read(root.join(".git/config")).unwrap();
    for name in ["", "-option", "bad.name", "bad\nname"] {
        assert!(!valid_name(name));
    }
    for url in ["https://secret@host/repo", "ext::touch marker", "bad\0url"] {
        assert!(!valid_url(url));
    }
    assert!(
        create(
            &root,
            "new",
            "https://host.invalid/repo",
            &AtomicBool::new(true)
        )
        .is_err()
    );
    assert_eq!(fs::read(root.join(".git/config")).unwrap(), before);
}
