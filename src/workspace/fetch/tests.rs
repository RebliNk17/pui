use super::*;
#[test]
fn fetch_stale_endpoint_and_precancel_refuse_without_tracking_write() {
    let root = std::fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
    let git = crate::workspace::tests::git;
    git(
        &root,
        &[
            "config",
            "remote.origin.url",
            "https://example.invalid/repo",
        ],
    );
    let cancel = AtomicBool::new(false);
    let review = inspect(&root, "origin", "main", &cancel).unwrap();
    git(
        &root,
        &["config", "remote.origin.url", "https://other.invalid/repo"],
    );
    assert!(apply(review, 30, &cancel).unwrap_err().contains("changed"));
    assert!(inspect(&root, "origin", "main", &AtomicBool::new(true)).is_err());
    assert!(!root.join(".git/refs/remotes/origin/main").exists());
}
#[test]
fn fetch_exact_branch_updates_only_tracking_ref_preserves_dirty_worktree_and_refuses_rewind() {
    let root = std::fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
    let git = crate::workspace::tests::git;
    let remote = root.with_extension("fetch-bare");
    std::fs::create_dir(&remote).unwrap();
    git(&remote, &["init", "--bare"]);
    git(
        &root,
        &["config", "remote.origin.url", remote.to_str().unwrap()],
    );
    git(
        &root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "push",
            "--no-verify",
            "--",
            remote.to_str().unwrap(),
            "HEAD:refs/heads/chosen",
        ],
    );
    let cancel = AtomicBool::new(false);
    let before = crate::workspace::tests::snapshot(&root);
    let file = std::fs::read(root.join("README.md")).unwrap();
    let first = apply(
        inspect(&root, "origin", "chosen", &cancel).unwrap(),
        30,
        &cancel,
    )
    .unwrap();
    assert_eq!(
        git(&root, &["rev-parse", "refs/remotes/origin/chosen"]),
        format!("{first}\n").as_bytes()
    );
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    assert_eq!(std::fs::read(root.join("README.md")).unwrap(), file);
    assert!(!root.join(".git/FETCH_HEAD").exists());
    // Remote advances without changing this checkout.
    let tree = String::from_utf8(git(&root, &["rev-parse", "HEAD^{tree}"])).unwrap();
    let next = String::from_utf8(git(
        &root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=f@example.invalid",
            "commit-tree",
            tree.trim(),
            "-p",
            &first,
            "-m",
            "remote advance",
        ],
    ))
    .unwrap()
    .trim()
    .to_owned();
    git(
        &root,
        &[
            "push",
            "--no-verify",
            "--",
            remote.to_str().unwrap(),
            &format!("{next}:refs/heads/chosen"),
        ],
    );
    let review = inspect(&root, "origin", "chosen", &cancel).unwrap();
    assert_eq!(apply(review, 30, &cancel).unwrap(), next);
    git(
        &remote,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "update-ref",
            "refs/heads/chosen",
            &first,
            &next,
        ],
    );
    let error = apply(
        inspect(&root, "origin", "chosen", &cancel).unwrap(),
        30,
        &cancel,
    )
    .unwrap_err();
    assert!(error.contains("rewound"));
    assert_eq!(
        git(&root, &["rev-parse", "refs/remotes/origin/chosen"]),
        format!("{next}\n").as_bytes()
    );
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
}
