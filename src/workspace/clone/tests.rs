use super::*;
fn clone_fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!("pui-clone-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let git = crate::workspace::tests::git;
    let src = root.join("src");
    fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    fs::write(src.join("file.txt"), "content\n").unwrap();
    git(&src, &["add", "file.txt"]);
    git(
        &src,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "first",
        ],
    );
    root
}
#[test]
fn local_clone_into_absent_destination_checks_out_branch_and_refuses_collisions() {
    for branch in [None, Some("main")] {
        let root = clone_fixture();
        let src = root.join("src");
        fs::create_dir(root.join("parent")).unwrap();
        let parent = fs::canonicalize(root.join("parent")).unwrap();
        let cancel = AtomicBool::new(false);
        let review = inspect(&parent, "dest", src.to_str().unwrap(), branch, &cancel).unwrap();
        apply(review, 30, &cancel).unwrap();
        assert!(parent.join("dest/file.txt").is_file());
        assert_eq!(
            crate::workspace::tests::git(&parent.join("dest"), &["rev-parse", "HEAD"]),
            crate::workspace::tests::git(&src, &["rev-parse", "HEAD"])
        );
        assert!(
            crate::workspace::tests::git(&parent.join("dest"), &["status", "--porcelain"])
                .is_empty()
        );
        // Re-inspection after creation refuses the now-existing destination.
        assert!(inspect(&parent, "dest", src.to_str().unwrap(), branch, &cancel).is_err());
    }
}
#[test]
fn clone_refuses_bad_url_dest_name_and_noncanonical_parent_and_times_out() {
    let root = clone_fixture();
    let src = root.join("src");
    fs::create_dir(root.join("parent")).unwrap();
    let parent = fs::canonicalize(root.join("parent")).unwrap();
    let cancel = AtomicBool::new(false);
    for url in ["", "https://user:pass@example.invalid/repo", "not-a-url"] {
        assert!(inspect(&parent, "dest", url, None, &cancel).is_err());
    }
    for name in ["", ".", "..", "a/b", "a\0b", "has control\n"] {
        assert!(inspect(&parent, name, src.to_str().unwrap(), None, &cancel).is_err());
    }
    assert!(
        inspect(
            &parent.join("missing"),
            "dest",
            src.to_str().unwrap(),
            None,
            &cancel
        )
        .is_err()
    );
    assert!(
        inspect(
            &root.join("relative"),
            "dest",
            src.to_str().unwrap(),
            None,
            &cancel
        )
        .is_err()
    );
    for seconds in [0, 301] {
        assert!(
            apply(
                inspect(&parent, "dest-t", src.to_str().unwrap(), None, &cancel).unwrap(),
                seconds,
                &cancel
            )
            .is_err()
        );
    }
}
