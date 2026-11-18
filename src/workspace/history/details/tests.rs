use super::*;
fn hash() -> String {
    "a".repeat(40)
}
fn encoded(oid: &str, time: &str, message: &str) -> Vec<u8> {
    format!("{oid}\0{time}\0\0N\0\0{message}\0\n").into_bytes()
}
#[test]
fn exact_message_unicode_newlines_empty_literal_markup_and_sha256_are_preserved() {
    for oid in [hash(), "b".repeat(64)] {
        for message in [
            "",
            "Subject 世界\n\nBody <script>literal</script>\r\n\t=1+2\n",
        ] {
            let detail = parse(&encoded(&oid, "1700000000", message), &oid).unwrap();
            assert_eq!(detail.oid, oid);
            assert_eq!(detail.message, message);
        }
    }
}
#[test]
fn malformed_mismatch_controls_oversize_and_truncated_messages_fail_whole() {
    let oid = hash();
    for bytes in [
        encoded(&"b".repeat(40), "1", "other"),
        encoded(&oid, "-1", "bad"),
        encoded(&oid, "NaN", "bad"),
        encoded(&oid, "1", "bad\u{1b}escape"),
        encoded(&oid, "1", "bad\0injected"),
        encoded(&oid, "1", &"x".repeat(MAX_MESSAGE + 1)),
        vec![b'x'; MAX_WIRE + 1],
        vec![0xff],
        b"truncated".to_vec(),
    ] {
        assert!(parse(&bytes, &oid).is_err());
    }
    assert!(read(Path::new("/unused"), "--all", &AtomicBool::new(false)).is_err());
}
#[test]
fn parent_topology_preserves_order_root_linear_merge_sha256_and_whole_refusal() {
    for width in [40, 64] {
        let selected = "a".repeat(width);
        for count in [0, 1, 2, 16] {
            let parents = (0..count)
                .map(|index| format!("{index:0width$x}"))
                .collect::<Vec<_>>();
            let bytes = format!(
                "{selected}\x001\x00{}\x00N\x00\x00message\x00\n",
                parents.join(" ")
            )
            .into_bytes();
            assert_eq!(parse(&bytes, &selected).unwrap().parents, parents);
        }
    }
    let selected = hash();
    for parents in [
        "b".repeat(64),
        selected.clone(),
        format!("{} {}", "b".repeat(40), "b".repeat(40)),
        format!(" {}", "b".repeat(40)),
        "bad".into(),
        (0..17)
            .map(|index| format!("{index:040x}"))
            .collect::<Vec<_>>()
            .join(" "),
    ] {
        assert!(
            parse(
                format!("{selected}\x001\x00{parents}\x00N\x00\x00message\x00\n").as_bytes(),
                &selected
            )
            .is_err()
        );
    }
}
#[test]
fn parent_topology_matches_local_raw_commit_headers_without_mutation_or_extra_read() {
    let root = crate::workspace::tests::fixture();
    let snapshot = super::super::read(&root, &AtomicBool::new(false)).unwrap();
    let before = crate::workspace::tests::snapshot(&root);
    let detail = read(&root, &snapshot.head, &AtomicBool::new(false)).unwrap();
    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["cat-file", "commit", &snapshot.head])
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    let raw = String::from_utf8(output.stdout).unwrap();
    let parents = raw
        .split("\n\n")
        .next()
        .unwrap()
        .lines()
        .filter_map(|line| line.strip_prefix("parent "))
        .collect::<Vec<_>>();
    assert_eq!(detail.parents, parents);
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
}
#[test]
fn production_detail_arguments_never_select_refs_patch_helpers_notes_signature_or_network() {
    let oid = hash();
    let command = command(Path::new("/fixture"), &oid);
    let args = command
        .get_args()
        .map(|arg| arg.to_str().unwrap())
        .collect::<Vec<_>>();
    for flag in [
        "show",
        "--no-patch",
        "--no-notes",
        "--no-use-mailmap",
        "--no-ext-diff",
        "--no-textconv",
        "--end-of-options",
    ] {
        assert!(args.contains(&flag));
    }
    // Passive reads must never use signature verification placeholders.
    assert!(args.contains(&"--format=%H%x00%ct%x00%P%x00N%x00%x00%B%x00"));
    assert_eq!(args[args.len() - 2], oid);
    assert_eq!(args.last(), Some(&"--"));
    assert!(
        command
            .get_envs()
            .any(|(key, value)| key == "GIT_NO_LAZY_FETCH"
                && value == Some(std::ffi::OsStr::new("1")))
    );
    assert!(
        command
            .get_envs()
            .any(|(key, value)| key == "GIT_NO_REPLACE_OBJECTS"
                && value == Some(std::ffi::OsStr::new("1")))
    );
}
#[test]
fn retained_commit_parent_reads_local_object_outside_snapshot_without_mutation() {
    let root = crate::workspace::tests::fixture();
    let cancel = AtomicBool::new(false);
    let mut snapshot = super::super::read(&root, &cancel).unwrap();
    // Retain one real commit: do not depend on the source checkout having 101 ancestors.
    snapshot.commits.truncate(1);
    let before = crate::workspace::tests::snapshot(&root);
    let oldest = read(&root, &snapshot.commits.last().unwrap().oid, &cancel).unwrap();
    let parent = oldest.parents.first().expect("fixture HEAD has a parent");
    assert!(!snapshot.commits.iter().any(|commit| &commit.oid == parent));
    let outside = read(&root, parent, &cancel).unwrap();
    assert_eq!(&outside.oid, parent);
    assert!(read(&root, parent, &AtomicBool::new(true)).is_err());
    assert!(read(&root, &"0".repeat(40), &cancel).is_err());
    assert_eq!(before, crate::workspace::tests::snapshot(&root));
}
#[test]
fn real_existing_commit_detail_and_missing_object_cancel_leave_index_head_and_worktree_unchanged() {
    let root = crate::workspace::tests::fixture();
    let cancel = AtomicBool::new(false);
    let snapshot = super::super::read(&root, &cancel).unwrap();
    let before = crate::workspace::tests::snapshot(&root);
    let detail = read(&root, &snapshot.head, &cancel).unwrap();
    assert_eq!(detail.oid, snapshot.head);
    assert_eq!(
        detail.message.lines().next().unwrap_or(""),
        snapshot.commits[0].subject
    );
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    assert!(read(&root, &"0".repeat(40), &cancel).is_err());
    assert!(
        read(&root, &snapshot.head, &AtomicBool::new(true))
            .unwrap_err()
            .contains("cancelled")
    );
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
}
#[test]
fn commit_detail_never_invokes_configured_clean_diff_or_signature_helpers() {
    let root = crate::workspace::tests::fixture();
    let snapshot = super::super::read(&root, &AtomicBool::new(false)).unwrap();
    let head = std::fs::read(root.join(".git/HEAD")).unwrap();
    let index = std::fs::read(root.join(".git/index")).unwrap();
    let config = root.join(".git/config");
    let mut contents = std::fs::read_to_string(&config).unwrap();
    contents.push_str("\n[filter \"detail-unsafe\"]\n clean = touch .git/detail-filter-ran\n process = touch .git/detail-filter-ran\n[diff]\n external = touch .git/detail-diff-ran\n[log]\n showSignature = true\n[gpg]\n program = touch .git/detail-signature-ran\n");
    std::fs::write(config, contents).unwrap();
    std::fs::write(
        root.join(".git/info/attributes"),
        "* filter=detail-unsafe\n",
    )
    .unwrap();
    assert!(read(&root, &snapshot.head, &AtomicBool::new(false)).is_ok());
    for marker in [
        "detail-filter-ran",
        "detail-diff-ran",
        "detail-signature-ran",
    ] {
        assert!(!root.join(".git").join(marker).exists());
    }
    assert_eq!(std::fs::read(root.join(".git/HEAD")).unwrap(), head);
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
}
#[cfg(unix)]
#[test]
fn detail_deadline_overflow_and_cancel_reap_direct_child_without_partial_result() {
    use std::process::Stdio;
    for (script, cancel, timeout) in [
        ("exec sleep 30", false, std::time::Duration::from_millis(30)),
        ("exec sleep 30", true, std::time::Duration::from_secs(1)),
        (
            "while :; do printf '0123456789abcdef'; done",
            false,
            std::time::Duration::from_secs(2),
        ),
    ] {
        let mut child = Command::new("sh")
            .args(["-c", script])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        assert!(
            read_only::supervise(
                &mut child,
                &AtomicBool::new(cancel),
                Instant::now() + timeout,
                MAX_WIRE,
                "Commit detail fixture",
                "too large"
            )
            .is_err()
        );
        assert!(child.try_wait().unwrap().is_some());
    }
}
#[test]
fn maximum_message_parse_has_bounded_owned_capacity_and_no_partial_output() {
    let oid = hash();
    let bytes = encoded(&oid, "1700000000", &"x".repeat(MAX_MESSAGE));
    let start = Instant::now();
    for _ in 0..1000 {
        let detail = parse(&bytes, &oid).unwrap();
        assert_eq!(detail.message.len(), MAX_MESSAGE);
        assert_eq!(detail.message.capacity(), MAX_MESSAGE);
    }
    eprintln!(
        "1000x24KiBcommitdetailparse {:?}; wire{}B",
        start.elapsed(),
        bytes.len()
    );
}
#[test]
fn signed_object_detection_never_executes_configured_verifier() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let root = crate::workspace::tests::fixture();
    let git = crate::workspace::tests::git;
    let tree = String::from_utf8(git(&root, &["rev-parse", "HEAD^{tree}"])).unwrap();
    let raw = format!(
        "tree {}\nauthor Fixture <f@example.invalid> 1700000000 +0000\ncommitter Fixture <f@example.invalid> 1700000000 +0000\ngpgsig -----BEGIN PGP SIGNATURE-----\n invalid untrusted payload\n -----END PGP SIGNATURE-----\n\nmessage\n",
        tree.trim()
    );
    let marker = root.join("verifier-ran");
    let script = root.join("verifier");
    std::fs::write(
        &script,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    git(&root, &["config", "gpg.program", script.to_str().unwrap()]);
    let mut p = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["hash-object", "-t", "commit", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    p.stdin.take().unwrap().write_all(raw.as_bytes()).unwrap();
    let out = p.wait_with_output().unwrap();
    assert!(out.status.success());
    let oid = String::from_utf8(out.stdout).unwrap();
    let loaded = read(&root, oid.trim(), &AtomicBool::new(false)).unwrap();
    assert_eq!(loaded.signature, "P");
    assert!(!marker.exists());
}
#[test]
fn signature_status_signer_parsed_and_unsigned_is_empty() {
    let oid = hash();
    for (status, signer, expected) in [
        ("N", "", ""),
        (
            "G",
            "Alice <a@example.invalid>",
            "G Alice <a@example.invalid>",
        ),
        ("B", "Bad", "B Bad"),
        ("X", "Expired", "X Expired"),
    ] {
        let bytes = format!("{oid}\x001\x00\x00{status}\x00{signer}\x00msg\x00\n").into_bytes();
        let detail = parse(&bytes, &oid).unwrap();
        assert_eq!(detail.signature, expected);
    }
    // Malformed signer with controls is stripped to status-only.
    let bytes = format!("{oid}\x001\x00\x00G\x00al\x1bice\x00msg\x00\n").into_bytes();
    assert_eq!(parse(&bytes, &oid).unwrap().signature, "G");
}
