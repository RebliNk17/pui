use super::*;
fn fixture() -> PathBuf {
    let root = std::fs::canonicalize(super::super::tests::fixture()).unwrap();
    let branch = String::from_utf8(super::super::tests::git(
        &root,
        &["symbolic-ref", "--short", "HEAD"],
    ))
    .unwrap();
    super::super::tests::git(
        &root,
        &[
            "config",
            "remote.origin.url",
            "git@example.invalid:team/repo.git",
        ],
    );
    super::super::tests::git(
        &root,
        &[
            "config",
            &format!("branch.{}.remote", branch.trim()),
            "origin",
        ],
    );
    super::super::tests::git(
        &root,
        &[
            "config",
            &format!("branch.{}.merge", branch.trim()),
            "refs/heads/master",
        ],
    );
    root
}
#[test]
fn local_review_exact_destination_without_remote_contact_or_repo_mutation() {
    let root = fixture();
    let before = super::super::tests::snapshot(&root);
    let s = prepare(&root, &AtomicBool::new(false)).unwrap();
    assert_eq!(s.remote, "origin");
    assert_eq!(s.destination, "refs/heads/master");
    assert_eq!(s.endpoint, "git@example.invalid:team/repo.git");
    assert_eq!(super::super::tests::snapshot(&root), before);
}
#[test]
fn custom_routing_multiple_endpoints_and_embedded_credentials_refuse() {
    for case in 0..4 {
        let root = fixture();
        match case {
            0 => {
                super::super::tests::git(&root, &["config", "remote.pushDefault", "other"]);
            }
            1 => {
                super::super::tests::git(
                    &root,
                    &[
                        "config",
                        "--add",
                        "remote.origin.pushurl",
                        "https://one.invalid/repo",
                    ],
                );
                super::super::tests::git(
                    &root,
                    &[
                        "config",
                        "--add",
                        "remote.origin.pushurl",
                        "https://two.invalid/repo",
                    ],
                );
            }
            2 => {
                super::super::tests::git(
                    &root,
                    &[
                        "config",
                        "remote.origin.url",
                        "https://secret-token@example.invalid/repo",
                    ],
                );
            }
            _ => {
                super::super::tests::git(&root, &["config", "remote.origin.mirror", "true"]);
            }
        }
        assert!(prepare(&root, &AtomicBool::new(false)).is_err());
    }
}
#[test]
fn confirmed_push_updates_only_disposable_remote_and_stale_review_refuses() {
    let root = fixture();
    let remote = root.with_extension("bare");
    std::fs::create_dir(&remote).unwrap();
    super::super::tests::git(&remote, &["init", "--bare"]);
    super::super::tests::git(
        &root,
        &["config", "remote.origin.url", remote.to_str().unwrap()],
    );
    let cancel = AtomicBool::new(false);
    let snapshot = prepare(&root, &cancel).unwrap();
    let head = snapshot.head.clone();
    assert_eq!(inspect_remote(&snapshot, &cancel).unwrap(), None);
    let inspected = snapshot.clone();
    assert_eq!(apply(snapshot, &cancel).unwrap(), Outcome::Updated);
    assert_eq!(
        inspect_remote(&inspected, &cancel).unwrap(),
        Some(head.clone())
    );
    assert_eq!(
        apply(prepare(&root, &cancel).unwrap(), &cancel).unwrap(),
        Outcome::UpToDate
    );
    assert_eq!(
        String::from_utf8(super::super::tests::git(
            &remote,
            &["rev-parse", "refs/heads/master"]
        ))
        .unwrap()
        .trim(),
        head
    );
    let stale = prepare(&root, &cancel).unwrap();
    super::super::tests::git(
        &root,
        &[
            "config",
            "remote.origin.url",
            "https://different.invalid/repo",
        ],
    );
    assert!(apply(stale, &cancel).is_err());
}
#[test]
fn rejected_remote_hook_preserves_destination_and_never_discloses_output() {
    let root = fixture();
    let remote = root.with_extension("reject-bare");
    std::fs::create_dir(&remote).unwrap();
    super::super::tests::git(&remote, &["init", "--bare"]);
    super::super::tests::git(
        &root,
        &["config", "remote.origin.url", remote.to_str().unwrap()],
    );
    let hook = remote.join("hooks/pre-receive");
    std::fs::write(
        &hook,
        "#!/bin/sh\necho secret-token-must-not-escape >&2\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let cancel = AtomicBool::new(false);
    let error = apply(prepare(&root, &cancel).unwrap(), &cancel).unwrap_err();
    assert!(error.contains("Push rejected"));
    assert!(!error.contains("secret-token"));
    assert!(!remote.join("refs/heads/master").exists());
}
#[test]
fn porcelain_is_fixed_outcome_not_remote_text_and_ambiguous_responses_refuse() {
    assert_eq!(
        outcome(
            b"To secret-url\n*\tOID:refs/heads/main\t[new branch]\nDone\n",
            true
        )
        .unwrap(),
        Outcome::Updated
    );
    assert_eq!(
        outcome(b"=\tOID:refs/heads/main\t[up to date]\n", true).unwrap(),
        Outcome::UpToDate
    );
    let error = outcome(b"!\tOID:refs/heads/main\tsecret rejection reason\n", false).unwrap_err();
    assert!(!error.contains("secret"));
    for bytes in [
        b"".as_slice(),
        b"+\tref\tforced\n",
        b"=\ta\tx\n=\tb\tx\n",
        b"*\tbad\n",
    ] {
        assert!(outcome(bytes, true).is_err());
    }
}
#[test]
fn explicit_destination_without_upstream_pushes_only_selected_ref_and_leaves_config() {
    let root = fixture();
    let branch = String::from_utf8(super::super::tests::git(
        &root,
        &["symbolic-ref", "--short", "HEAD"],
    ))
    .unwrap();
    super::super::tests::git(
        &root,
        &[
            "config",
            "--remove-section",
            &format!("branch.{}", branch.trim()),
        ],
    );
    let remote = root.with_extension("explicit-bare");
    std::fs::create_dir(&remote).unwrap();
    super::super::tests::git(&remote, &["init", "--bare"]);
    super::super::tests::git(
        &root,
        &["config", "remote.origin.url", remote.to_str().unwrap()],
    );
    let config = std::fs::read(root.join(".git/config")).unwrap();
    let cancel = AtomicBool::new(false);
    assert!(prepare(&root, &cancel).is_err());
    let snapshot = prepare_target(&root, Some(("origin", "refs/heads/chosen")), &cancel).unwrap();
    let head = snapshot.head.clone();
    assert!(snapshot.explicit_target);
    assert_eq!(apply(snapshot, &cancel).unwrap(), Outcome::Updated);
    assert_eq!(
        String::from_utf8(super::super::tests::git(
            &remote,
            &["rev-parse", "refs/heads/chosen"]
        ))
        .unwrap()
        .trim(),
        head
    );
    assert!(!remote.join("refs/heads/master").exists());
    assert_eq!(std::fs::read(root.join(".git/config")).unwrap(), config);
    for (name, destination) in [
        ("missing", "refs/heads/chosen"),
        ("origin", "refs/heads/../bad"),
        ("origin", "refs/tags/not-branch"),
    ] {
        assert!(prepare_target(&root, Some((name, destination)), &cancel).is_err());
    }
}
#[test]
fn remote_oid_requires_exact_unique_ref_and_never_returns_injected_text() {
    assert_eq!(remote_oid(b"", "refs/heads/main").unwrap(), None);
    let oid = "a".repeat(40);
    assert_eq!(
        remote_oid(
            format!("{oid}\trefs/heads/main\n").as_bytes(),
            "refs/heads/main"
        )
        .unwrap(),
        Some(oid.clone())
    );
    for bytes in [
        format!("{oid}\trefs/heads/wrong\n"),
        format!("{oid}\trefs/heads/main\n{oid}\trefs/heads/main\n"),
        "secret text\trefs/heads/main\n".into(),
    ] {
        let error = remote_oid(bytes.as_bytes(), "refs/heads/main").unwrap_err();
        assert!(!error.contains("secret"));
    }
}
#[test]
fn local_comparison_equal_ahead_behind_diverged_and_missing_without_fetch() {
    let root = fixture();
    let cancel = AtomicBool::new(false);
    let mut s = prepare(&root, &cancel).unwrap();
    let base = s.head.clone();
    assert_eq!(compare(&s, None, &cancel), Relation::Absent);
    assert_eq!(compare(&s, Some(&base), &cancel), Relation::Equal);
    let tree = String::from_utf8(super::super::tests::git(
        &root,
        &["rev-parse", "HEAD^{tree}"],
    ))
    .unwrap();
    let make = |message: &str| {
        String::from_utf8(super::super::tests::git(
            &root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit-tree",
                tree.trim(),
                "-p",
                &base,
                "-m",
                message,
            ],
        ))
        .unwrap()
        .trim()
        .to_string()
    };
    let one = make("one");
    let two = make("two");
    s.head = one.clone();
    assert_eq!(
        compare(&s, Some(&base), &cancel),
        Relation::Counts {
            ahead: 1,
            behind: 0
        }
    );
    s.head = base.clone();
    assert_eq!(
        compare(&s, Some(&one), &cancel),
        Relation::Counts {
            ahead: 0,
            behind: 1
        }
    );
    s.head = one;
    assert_eq!(
        compare(&s, Some(&two), &cancel),
        Relation::Counts {
            ahead: 1,
            behind: 1
        }
    );
    assert_eq!(
        compare(&s, Some(&"f".repeat(40)), &cancel),
        Relation::Unavailable
    );
    for bad in [
        b"1\\t2\\n".as_slice(),
        b"-1\t2\n",
        b"1\t2\nextra",
        b"4294967296\t0\n",
    ] {
        assert!(parse_counts(bad).is_none());
    }
}
#[test]
fn transport_policy_refuses_helpers_queries_and_passwords() {
    for bad in [
        "ext::touch marker",
        "file:///tmp/repo",
        "https://token@host/path",
        "ssh://user:password@host/path",
        "https://host/path?token=secret",
        "git@host:path#secret",
    ] {
        assert!(endpoint(bad).is_err());
    }
    for good in [
        "https://host/team/repo.git",
        "ssh://host/team/repo.git",
        "git@host:team/repo.git",
    ] {
        assert_eq!(endpoint(good).unwrap(), good);
    }
}
