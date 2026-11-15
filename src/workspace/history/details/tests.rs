use super::*;
fn hash() -> String {
    "a".repeat(40)
}
fn encoded(oid: &str, time: &str, message: &str) -> Vec<u8> {
    format!("{oid}\0{time}\0\0{message}\0\n").into_bytes()
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
            let bytes =
                format!("{selected}\x001\x00{}\x00message\x00\n", parents.join(" ")).into_bytes();
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
                format!("{selected}\x001\x00{parents}\x00message\x00\n").as_bytes(),
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
        "--no-show-signature",
        "--no-notes",
        "--no-use-mailmap",
        "--no-ext-diff",
        "--no-textconv",
        "--end-of-options",
    ] {
        assert!(args.contains(&flag));
    }
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
