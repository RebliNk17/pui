use super::*;
fn fixture() -> PathBuf {
    static SEED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let seed = SEED.get_or_init(|| {
        let root = fs::canonicalize(super::super::tests::fixture()).unwrap();
        super::super::tests::git(&root, &["config", "user.name", "Pui Test"]);
        super::super::tests::git(&root, &["config", "user.email", "pui-test@example.invalid"]);
        super::super::tests::git(&root, &["add", "--", "README.md"]);
        root
    });
    let root = std::env::temp_dir().join(format!("pui-commit-fixture-{}", uuid::Uuid::new_v4()));
    super::super::tests::copy_tree(seed, &root);
    fs::canonicalize(root).unwrap()
}
#[test]
fn real_commit_moves_only_disposable_branch_preserves_index_worktree_and_skips_hooks() {
    let root = fixture();
    let cancel = AtomicBool::new(false);
    let s = prepare(&root, &cancel).unwrap();
    let old = s.head.clone();
    let index = fs::read(root.join(".git/index")).unwrap();
    let content = fs::read(root.join("README.md")).unwrap();
    let hook = root.join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\ntouch hook-ran\nexit 77\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let oid = apply(s, "explicit test commit", &cancel).unwrap();
    assert_ne!(oid, old);
    let authors = line(super::super::tests::git(
        &root,
        &["log", "-1", "--format=%an <%ae> | %cn <%ce>"],
    ))
    .unwrap();
    assert_eq!(
        authors,
        "Pui Test <pui-test@example.invalid> | Pui Test <pui-test@example.invalid>"
    );
    assert!(!root.join("hook-ran").exists());
    assert_eq!(
        line(super::super::tests::git(
            &root,
            &["log", "-1", "--format=%s"]
        ))
        .unwrap(),
        "explicit test commit"
    );
    assert_eq!(
        line(super::super::tests::git(&root, &["rev-parse", "HEAD"])).unwrap(),
        oid
    );
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(root.join("README.md")).unwrap(), content);
    assert!(prepare(&root, &cancel).is_err());
}
#[test]
fn identity_change_refuses_frozen_snapshot_before_branch_publication() {
    let root = fixture();
    let cancel = AtomicBool::new(false);
    let snapshot = prepare(&root, &cancel).unwrap();
    let head = snapshot.head.clone();
    let index = fs::read(root.join(".git/index")).unwrap();
    super::super::tests::git(&root, &["config", "user.email", "changed@example.invalid"]);
    let error = apply(snapshot, "not under a different identity", &cancel).unwrap_err();
    assert!(error.contains("identity changed"));
    assert_eq!(
        line(super::super::tests::git(&root, &["rev-parse", "HEAD"])).unwrap(),
        head
    );
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
}
#[test]
fn identity_parser_strips_date_not_name_and_rejects_malformed_or_oversize() {
    let parsed = identity(
        "世界 Name <name@example.invalid> 1234567890 +0530\n"
            .as_bytes()
            .to_vec(),
    )
    .unwrap();
    assert_eq!(parsed.name, "世界 Name");
    assert_eq!(parsed.email, "name@example.invalid");
    for bad in [
        "missing email".into(),
        "Name <> 12 +0000".into(),
        format!("{} <e> 12 +0000", "x".repeat(513)),
        "Name\0 <e> 12 +0000".into(),
    ] {
        assert!(identity(bad.into_bytes()).is_err());
    }
}
#[test]
fn file_list_exact_unicode_literal_paths_added_deleted_modified_and_revalidated() {
    let root = fixture();
    let cancel = AtomicBool::new(false);
    fs::write(root.join("世界 [literal].txt"), "new\n").unwrap();
    super::super::tests::git(&root, &["add", "--", "世界 [literal].txt"]);
    let snapshot = prepare(&root, &cancel).unwrap();
    assert!(
        snapshot
            .files
            .iter()
            .any(|e| e.path == "世界 [literal].txt" && e.status == 'A')
    );
    assert!(
        snapshot
            .files
            .iter()
            .any(|e| e.path == "README.md" && e.status == 'M')
    );
    assert_eq!(snapshot.files.len(), 2);
    let head = snapshot.head.clone();
    fs::write(root.join("another.txt"), "later\n").unwrap();
    super::super::tests::git(&root, &["add", "--", "another.txt"]);
    assert!(apply(snapshot, "stale list", &cancel).is_err());
    assert_eq!(
        line(super::super::tests::git(&root, &["rev-parse", "HEAD"])).unwrap(),
        head
    );
}
fn unborn() -> PathBuf {
    static SEED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let seed = SEED.get_or_init(|| {
        let root =
            std::env::temp_dir().join(format!("pui-initial-commit-seed-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        super::super::tests::git(&root, &["init", "--template=", "--initial-branch=main"]);
        super::super::tests::git(&root, &["config", "user.name", "Pui Test"]);
        super::super::tests::git(&root, &["config", "user.email", "pui@example.invalid"]);
        fs::write(root.join("first.txt"), "first staged version\n").unwrap();
        super::super::tests::git(&root, &["add", "--", "first.txt"]);
        root
    });
    let root = std::env::temp_dir().join(format!("pui-initial-commit-{}", uuid::Uuid::new_v4()));
    super::super::tests::copy_tree(seed, &root);
    fs::canonicalize(root).unwrap()
}
#[test]
fn initial_commit_is_parentless_and_preserves_unstaged_content_and_index() {
    let root = unborn();
    let cancel = AtomicBool::new(false);
    let snapshot = prepare(&root, &cancel).unwrap();
    assert_eq!(snapshot.head, "0".repeat(40));
    assert_eq!(snapshot.files[0].status, 'A');
    let index = fs::read(root.join(".git/index")).unwrap();
    fs::write(root.join("first.txt"), "unstaged second version\n").unwrap();
    let oid = apply(snapshot, "initial", &cancel).unwrap();
    assert_eq!(
        line(super::super::tests::git(
            &root,
            &["rev-list", "--parents", "-1", "HEAD"]
        ))
        .unwrap(),
        oid
    );
    assert_eq!(
        super::super::tests::git(&root, &["show", "HEAD:first.txt"]),
        b"first staged version\n"
    );
    assert_eq!(
        fs::read(root.join("first.txt")).unwrap(),
        b"unstaged second version\n"
    );
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
}
#[test]
fn branch_created_during_initial_review_refuses_without_replacing_it() {
    let root = unborn();
    let cancel = AtomicBool::new(false);
    let old = prepare(&root, &cancel).unwrap();
    let winner = apply(prepare(&root, &cancel).unwrap(), "winner", &cancel).unwrap();
    assert!(apply(old, "stale initial", &cancel).is_err());
    assert_eq!(
        line(super::super::tests::git(&root, &["rev-parse", "HEAD"])).unwrap(),
        winner
    );
}
#[test]
fn ssh_signed_commit_and_signer_failure_never_falls_back() {
    for fail in [false, true] {
        let root = fixture();
        let key = root.join(".git/disposable-sign-key");
        assert!(
            std::process::Command::new("/usr/bin/ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(&key)
                .status()
                .unwrap()
                .success()
        );
        super::super::tests::git(&root, &["config", "gpg.format", "ssh"]);
        super::super::tests::git(&root, &["config", "user.signingkey", key.to_str().unwrap()]);
        if fail {
            super::super::tests::git(&root, &["config", "gpg.ssh.program", "/usr/bin/false"]);
        }
        if !fail {
            let wrapper = root.join(".git/slow-signer");
            fs::write(
                &wrapper,
                "#!/bin/sh\nsleep 3.2\nexec /usr/bin/ssh-keygen \"$@\"\n",
            )
            .unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
            }
            super::super::tests::git(
                &root,
                &["config", "gpg.ssh.program", wrapper.to_str().unwrap()],
            );
        }
        let cancel = AtomicBool::new(false);
        let snapshot = prepare(&root, &cancel).unwrap();
        let head = snapshot.head.clone();
        let index = fs::read(root.join(".git/index")).unwrap();
        if !fail {
            let error = apply_timeouts(
                prepare(&root, &cancel).unwrap(),
                "timedout",
                true,
                false,
                false,
                30,
                1,
                &cancel,
            )
            .unwrap_err();
            assert!(error.contains("timed out"));
            assert_eq!(
                line(super::super::tests::git(&root, &["rev-parse", "HEAD"])).unwrap(),
                head
            );
        }
        let result = apply_timeouts(snapshot, "signed", true, false, false, 30, 10, &cancel);
        if fail {
            assert!(result.unwrap_err().contains("no unsigned fallback"));
            assert_eq!(
                line(super::super::tests::git(&root, &["rev-parse", "HEAD"])).unwrap(),
                head
            );
        } else {
            let oid = result.unwrap();
            let body = super::super::tests::git(&root, &["cat-file", "commit", &oid]);
            assert!(
                String::from_utf8(body)
                    .unwrap()
                    .contains("gpgsig -----BEGIN SSH SIGNATURE-----")
            );
        }
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    }
}
#[test]
fn changed_signing_config_refuses_review() {
    let root = fixture();
    let cancel = AtomicBool::new(false);
    let snapshot = prepare(&root, &cancel).unwrap();
    super::super::tests::git(&root, &["config", "user.signingkey", "changed-key"]);
    assert!(apply_with_signing(snapshot, "stale", true, &cancel).is_err());
}
#[test]
fn opted_in_pre_commit_success_failure_and_private_index_mutation() {
    for mode in 0..3 {
        let root = fixture();
        let hook = root.join(".git/hooks/pre-commit");
        let script = match mode {
            0 => "#!/bin/sh\ntouch .git/check-ran\nexit 0\n",
            1 => "#!/bin/sh\ntouch .git/check-ran\nexit 1\n",
            _ => "#!/bin/sh\ntouch .git/check-ran\ngit update-index --force-remove -- README.md\n",
        };
        fs::write(&hook, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let cancel = AtomicBool::new(false);
        let snapshot = prepare(&root, &cancel).unwrap();
        let head = snapshot.head.clone();
        let index = fs::read(root.join(".git/index")).unwrap();
        let result = apply_options(snapshot, "checked commit", false, true, &cancel);
        assert!(root.join(".git/check-ran").exists());
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
        if mode == 0 {
            assert!(result.is_ok(), "{result:?}");
        } else {
            assert!(result.is_err());
            assert_eq!(
                line(super::super::tests::git(&root, &["rev-parse", "HEAD"])).unwrap(),
                head
            );
        }
    }
}
#[test]
fn commit_message_check_success_rejection_and_rewrite_never_changes_confirmed_commit() {
    for mode in 0..3 {
        let root = fixture();
        let hook = root.join(".git/hooks/commit-msg");
        let script = match mode {
            0 => "#!/bin/sh\ngrep -q 'confirmed message' \"$1\"\n",
            1 => "#!/bin/sh\nexit 1\n",
            _ => "#!/bin/sh\nprintf 'rewritten' > \"$1\"\n",
        };
        fs::write(&hook, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let cancel = AtomicBool::new(false);
        let snapshot = prepare(&root, &cancel).unwrap();
        let head = snapshot.head.clone();
        let index = fs::read(root.join(".git/index")).unwrap();
        let result = apply_checks(snapshot, "confirmed message", false, false, true, &cancel);
        if mode == 0 {
            assert!(result.is_ok(), "{result:?}");
            assert_eq!(
                line(super::super::tests::git(
                    &root,
                    &["log", "-1", "--format=%s"]
                ))
                .unwrap(),
                "confirmed message"
            );
        } else {
            assert!(result.is_err());
            assert_eq!(
                line(super::super::tests::git(&root, &["rev-parse", "HEAD"])).unwrap(),
                head
            );
        }
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
        assert!(!fs::read_dir(root.join(".git")).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("pui-commit-message-")
        }));
    }
}
#[test]
fn prepare_message_runs_before_validation_and_rewrite_or_failure_refuses() {
    for mode in 0..3 {
        let root = fixture();
        let hook = root.join(".git/hooks/prepare-commit-msg");
        let script = match mode {
            0 => "#!/bin/sh\ntest \"$2\" = message || exit 9\nprintf prepared > .git/prepared\n",
            1 => "#!/bin/sh\nexit 1\n",
            _ => "#!/bin/sh\nprintf rewritten > \"$1\"\n",
        };
        fs::write(&hook, script).unwrap();
        let validate = root.join(".git/hooks/commit-msg");
        fs::write(
            &validate,
            "#!/bin/sh\ntest -f .git/prepared || exit 8\ngrep -q confirmed \"$1\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [&hook, &validate] {
                fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let cancel = AtomicBool::new(false);
        let s = prepare(&root, &cancel).unwrap();
        let head = s.head.clone();
        let result =
            apply_message_preparation(s, "confirmed", false, false, true, true, 30, 30, &cancel);
        if mode == 0 {
            assert!(result.is_ok(), "{result:?}");
        } else {
            assert!(result.is_err());
            assert_eq!(
                line(super::super::tests::git(&root, &["rev-parse", "HEAD"])).unwrap(),
                head
            );
        }
    }
}
#[test]
fn post_commit_failure_preserves_published_commit_and_reports_no_repeat() {
    let root = fixture();
    let hook = root.join(".git/hooks/post-commit");
    fs::write(
        &hook,
        "#!/bin/sh\nprintf observed > .git/post-observed\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let cancel = AtomicBool::new(false);
    let oid = apply(prepare(&root, &cancel).unwrap(), "published", &cancel).unwrap();
    assert!(!root.join(".git/post-observed").exists());
    let warning = post_commit(&root, &cancel, 30).unwrap_err();
    assert!(warning.contains("already published"));
    assert!(warning.contains("do not repeat"));
    assert_eq!(
        fs::read(root.join(".git/post-observed")).unwrap(),
        b"observed"
    );
    assert_eq!(
        line(super::super::tests::git(&root, &["rev-parse", "HEAD"])).unwrap(),
        oid
    );
}
#[test]
fn hook_timeout_validation_and_longer_check_does_not_expire_publication_budget() {
    let root = fixture();
    let hook = root.join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\nsleep 3.2\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let cancel = AtomicBool::new(false);
    for timeout in [0, 301] {
        assert!(
            apply_timed_checks(
                prepare(&root, &cancel).unwrap(),
                "invalid",
                false,
                true,
                false,
                timeout,
                &cancel
            )
            .unwrap_err()
            .contains("1–300")
        );
    }
    let snapshot = prepare(&root, &cancel).unwrap();
    let head = snapshot.head.clone();
    assert!(
        apply_timed_checks(snapshot, "too short", false, true, false, 1, &cancel)
            .unwrap_err()
            .contains("timed out")
    );
    assert_eq!(
        line(super::super::tests::git(&root, &["rev-parse", "HEAD"])).unwrap(),
        head
    );
    assert!(
        apply_timed_checks(
            prepare(&root, &cancel).unwrap(),
            "long check",
            false,
            true,
            false,
            10,
            &cancel
        )
        .is_ok()
    );
}
#[test]
fn changed_index_foreign_lock_cancel_and_empty_message_refuse() {
    for case in 0..4 {
        let root = fixture();
        let cancel = AtomicBool::new(false);
        let s = prepare(&root, &cancel).unwrap();
        let head = s.head.clone();
        if case == 0 {
            let mut bytes = fs::read(&s.index_path).unwrap();
            bytes.push(0);
            fs::write(&s.index_path, bytes).unwrap();
        }
        if case == 1 {
            fs::write(s.index_path.with_file_name("index.lock"), "foreign").unwrap();
        }
        assert!(
            apply(
                s,
                if case == 3 { " " } else { "message" },
                &AtomicBool::new(case == 2)
            )
            .is_err()
        );
        assert_eq!(
            line(super::super::tests::git(&root, &["rev-parse", "HEAD"])).unwrap(),
            head
        );
        if case == 1 {
            assert_eq!(fs::read(root.join(".git/index.lock")).unwrap(), b"foreign");
        }
    }
}
