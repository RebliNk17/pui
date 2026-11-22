use super::*;
#[cfg(unix)]
#[test]
fn push_timeout_bounds_slow_hook_without_retry_and_longer_budget_succeeds() {
    use std::os::unix::fs::PermissionsExt;
    let root = fixture();
    let remote = root.with_extension("bare");
    std::fs::create_dir(&remote).unwrap();
    super::super::tests::git(&remote, &["init", "--bare"]);
    super::super::tests::git(
        &root,
        &["config", "remote.origin.url", remote.to_str().unwrap()],
    );
    let hook = root.join(".git/hooks/pre-push");
    std::fs::write(&hook, "#!/bin/sh\ncat >/dev/null\nsleep 2\nexit 0\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let cancel = AtomicBool::new(false);
    for details in [false, true] {
        let snapshot = prepare(&root, &cancel).unwrap();
        let error = apply_timed(snapshot, true, details, 1, &cancel).unwrap_err();
        assert!(error.contains("uncertain"));
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&remote)
            .args(["show-ref", "--verify", "refs/heads/master"])
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
    assert_eq!(
        apply_timed(prepare(&root, &cancel).unwrap(), true, false, 10, &cancel).unwrap(),
        Outcome::Updated
    );
    for seconds in [0, 301] {
        assert!(
            apply_timed(
                prepare(&root, &cancel).unwrap(),
                false,
                false,
                seconds,
                &cancel
            )
            .unwrap_err()
            .contains("1–300")
        );
    }
}
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
fn batched_routing_uses_literal_subsections_and_rejects_duplicate_or_empty_values() {
    let root = fixture();
    let branch = String::from_utf8(super::super::tests::git(
        &root,
        &["symbolic-ref", "--short", "HEAD"],
    ))
    .unwrap();
    // Unrelated custom routing is not an authority to reroute this selected branch.
    for (key, value) in [
        ("remote.other.push", "refs/heads/other"),
        ("branch.unrelated.pushRemote", "other"),
    ] {
        super::super::tests::git(&root, &["config", key, value]);
    }
    prepare(&root, &AtomicBool::new(false)).unwrap();
    for key in [
        format!("branch.{}.pushRemote", branch.trim()),
        "remote.pushDefault".into(),
        "remote.origin.push".into(),
        "remote.origin.mirror".into(),
    ] {
        super::super::tests::git(&root, &["config", "--add", &key, ""]);
        super::super::tests::git(&root, &["config", "--add", &key, "other"]);
        let before = std::fs::read(root.join(".git/config")).unwrap();
        assert!(
            prepare(&root, &AtomicBool::new(false)).is_err(),
            "Must refuse {key}"
        );
        assert_eq!(std::fs::read(root.join(".git/config")).unwrap(), before);
        super::super::tests::git(&root, &["config", "--unset-all", &key]);
    }
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
    let details =
        apply_diagnostics(prepare(&root, &cancel).unwrap(), false, true, &cancel).unwrap_err();
    assert!(details.contains("secret-token-must-not-escape"));
    assert!(details.contains("Opted-in untrusted"));
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
fn opted_in_pre_push_hook_failure_stops_remote_update_without_bypass() {
    let root = fixture();
    let remote = root.with_extension("prepush-bare");
    std::fs::create_dir(&remote).unwrap();
    super::super::tests::git(&remote, &["init", "--bare"]);
    super::super::tests::git(
        &root,
        &["config", "remote.origin.url", remote.to_str().unwrap()],
    );
    let hook = root.join(".git/hooks/pre-push");
    let marker = root.join(".git/hook-ran");
    std::fs::write(
        &hook,
        "#!/bin/sh\ncat >/dev/null\ntouch .git/hook-ran\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let cancel = AtomicBool::new(false);
    assert!(apply_with_hooks(prepare(&root, &cancel).unwrap(), true, &cancel).is_err());
    assert!(marker.exists());
    assert!(!remote.join("refs/heads/master").exists());
    std::fs::write(&hook, "#!/bin/sh\ncat >/dev/null\nexit 0\n").unwrap();
    assert_eq!(
        apply_with_hooks(prepare(&root, &cancel).unwrap(), true, &cancel).unwrap(),
        Outcome::Updated
    );
}
#[test]
fn diagnostics_dual_pipes_escape_controls_and_refuse_oversize() {
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.args(["-c", "printf 'out'; printf '\\033[31merror' >&2; exit 1"]);
    let (out, err, status) = diagnostics::run(
        &mut cmd,
        &AtomicBool::new(false),
        Instant::now() + DEADLINE,
        "test",
    )
    .unwrap();
    assert_eq!(out, b"out");
    assert!(err.starts_with(b"\x1b"));
    assert!(!status.success());
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.args(["-c", "yes secret | head -c 20000 >&2"]);
    assert!(
        diagnostics::run(
            &mut cmd,
            &AtomicBool::new(false),
            Instant::now() + DEADLINE,
            "test"
        )
        .unwrap_err()
        .contains("output exceeds")
    );
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
#[test]
fn signing_config_drift_refuses_even_when_target_is_unchanged() {
    let root = fixture();
    let cancel = AtomicBool::new(false);
    let review = prepare(&root, &cancel).unwrap();
    super::super::tests::git(
        &root,
        &["config", "user.signingkey", "changed-after-review"],
    );
    assert!(
        apply_signed(review, false, false, true, 30, 30, &cancel)
            .unwrap_err()
            .contains("changed")
    );
}
#[test]
fn signed_push_requires_configured_signer_and_refuses_bad_timeout() {
    let root = fixture();
    let remote = root.with_extension("sign-bare");
    std::fs::create_dir(&remote).unwrap();
    super::super::tests::git(&remote, &["init", "--bare"]);
    super::super::tests::git(
        &root,
        &["config", "remote.origin.url", remote.to_str().unwrap()],
    );
    let cancel = AtomicBool::new(false);
    // No signing key configured: signing must refuse without publishing or
    // silently falling back to an unsigned push.
    let snapshot = prepare(&root, &cancel).unwrap();
    let err = apply_signed(snapshot, false, false, true, 30, 30, &cancel).unwrap_err();
    // A signed push without a working signer must fail rather than fall back.
    assert!(!err.is_empty());
    assert!(!remote.join("refs/heads/master").exists());
    // The same ref pushes fine unsigned, proving signing was the differentiator.
    assert_eq!(
        apply_signed(
            prepare(&root, &cancel).unwrap(),
            false,
            false,
            false,
            30,
            30,
            &cancel
        )
        .unwrap(),
        Outcome::Updated
    );
    for bad in [0, 301] {
        assert!(
            apply_signed(
                prepare(&root, &cancel).unwrap(),
                false,
                false,
                true,
                30,
                bad,
                &cancel
            )
            .unwrap_err()
            .contains("1–300")
        );
    }
}
