use super::*;
fn fixture() -> (PathBuf, String) {
    let root = std::env::temp_dir().join(format!("pui-ff-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let git = crate::workspace::tests::git;
    git(&root, &["init", "-b", "main"]);
    git(&root, &["config", "user.name", "Fixture"]);
    git(&root, &["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(root.join("file"), "old\n").unwrap();
    git(&root, &["add", "file"]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "base"],
    );
    let old = String::from_utf8(git(&root, &["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    git(&root, &["switch", "-c", "source"]);
    std::fs::write(root.join("file"), "new\n").unwrap();
    git(&root, &["add", "file"]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "next"],
    );
    let next = String::from_utf8(git(&root, &["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    git(&root, &["update-ref", "refs/remotes/origin/main", &next]);
    git(&root, &["switch", "main"]);
    (root, old)
}
#[test]
fn clean_fast_forward_updates_current_checkout_but_refuses_dirty_stale_or_diverged() {
    for case in 0..3 {
        let (root, old) = fixture();
        let cancel = AtomicBool::new(false);
        let review = inspect(&root, "origin", "main", &cancel).unwrap();
        assert_eq!(review.head, old);
        assert_eq!(review.files.len(), 1);
        if case == 1 {
            std::fs::write(root.join("file"), "dirty\n").unwrap();
        }
        if case == 2 {
            crate::workspace::tests::git(&root, &["update-ref", "refs/remotes/origin/main", &old]);
        }
        let result = apply(review, "origin", "main", &cancel);
        if case == 0 {
            result.unwrap();
            assert_eq!(std::fs::read(root.join("file")).unwrap(), b"new\n");
            assert!(
                inspect(&root, "origin", "main", &cancel)
                    .unwrap_err()
                    .contains("already")
            );
        } else {
            assert!(result.is_err());
            assert_eq!(
                crate::workspace::tests::git(&root, &["rev-parse", "HEAD"]),
                format!("{old}\n").as_bytes()
            );
        }
    }
}
