use super::*;
use std::{fs, sync::atomic::Ordering, time::Duration};
fn head() -> String {
    "a".repeat(40)
}
fn row(hash: &str, time: &str, subject: &str) -> Vec<u8> {
    format!("{hash}\0{time}\0\0{subject}\0\n").into_bytes()
}
#[test]
fn strict_history_parser_caps_count_bytes_subjects_and_framing_without_partial_records() {
    let hash = head();
    let snapshot = parse(
        &row(&hash, "1700000000", "Unicode 世界 · literal <script>"),
        hash.clone(),
    )
    .unwrap();
    assert_eq!(snapshot.commits.len(), 1);
    assert!(!snapshot.limited);
    assert_eq!(
        snapshot.commits[0].subject,
        "Unicode 世界 · literal <script>"
    );
    for bytes in [
        row(&hash, "-1", "bad"),
        row(&hash, "not-time", "bad"),
        row(&hash, "1700000000", "bad\nsubject"),
        row(&hash, "1700000000", &"x".repeat(MAX_SUBJECT + 1)),
        row(&"z".repeat(40), "0", "bad"),
        b"broken".to_vec(),
        vec![0xff],
        vec![b'x'; MAX_BYTES + 1],
        Vec::new(),
    ] {
        assert!(parse(&bytes, hash.clone()).is_err());
    }
    let mut duplicate = row(&hash, "1", "first");
    duplicate.extend(row(&hash, "1", "again"));
    assert!(parse(&duplicate, hash.clone()).is_err());
    let sha256 = "f".repeat(64);
    assert!(parse(&row(&sha256, "0", ""), sha256).is_ok());
    let mut bytes = Vec::new();
    for n in 0..=MAX_COMMITS {
        bytes.extend(row(&format!("{n:040x}"), "1", "fixture"));
    }
    let snapshot = parse(&bytes, format!("{:040x}", 0)).unwrap();
    assert_eq!(snapshot.commits.len(), MAX_COMMITS);
    assert!(snapshot.limited);
    bytes.extend(row(&format!("{:040x}", MAX_COMMITS + 1), "1", "overflow"));
    assert!(parse(&bytes, format!("{:040x}", 0)).is_err());
}
#[test]
fn maximum_history_parse_is_bounded_and_unknown_metadata_is_literal_not_an_action() {
    let mut bytes = Vec::new();
    for n in 0..=MAX_COMMITS {
        bytes.extend(row(
            &format!("{n:040x}"),
            "1700000000",
            &"s".repeat(MAX_SUBJECT),
        ));
    }
    assert!(bytes.len() < MAX_BYTES);
    let start = Instant::now();
    for _ in 0..100 {
        let snapshot = parse(&bytes, format!("{:040x}", 0)).unwrap();
        assert_eq!(snapshot.commits.len(), MAX_COMMITS);
    }
    eprintln!(
        "100x101maxsubjecthistoryparse {:?}; wire{}B",
        start.elapsed(),
        bytes.len()
    );
}
#[test]
fn history_parent_metadata_validates_order_width_duplicates_and_count() {
    let hash = head();
    let parent = "b".repeat(40);
    let side = "c".repeat(40);
    let snapshot = parse(
        format!("{hash}\x001\x00{parent} {side}\x00merge\x00\n").as_bytes(),
        hash.clone(),
    )
    .unwrap();
    assert_eq!(
        snapshot.commits[0].first_parent.as_deref(),
        Some(parent.as_str())
    );
    assert_eq!(snapshot.commits[0].parent_count, 2);
    for parents in [
        hash.clone(),
        "z".repeat(40),
        "b".repeat(64),
        format!("{parent} {parent}"),
        format!(" {parent}"),
        (0..17)
            .map(|index| format!("{index:040x}"))
            .collect::<Vec<_>>()
            .join(" "),
    ] {
        assert!(
            parse(
                format!("{hash}\x001\x00{parents}\x00bad\x00\n").as_bytes(),
                hash.clone()
            )
            .is_err()
        );
    }
}
#[test]
fn real_history_first_parent_metadata_matches_explicit_details_without_mutation() {
    let root = crate::workspace::tests::fixture();
    let before = crate::workspace::tests::snapshot(&root);
    let snapshot = read(&root, &AtomicBool::new(false)).unwrap();
    let detail = details::read(&root, &snapshot.head, &AtomicBool::new(false)).unwrap();
    assert_eq!(
        snapshot.commits[0].first_parent.as_ref(),
        detail.parents.first()
    );
    assert_eq!(
        snapshot.commits[0].parent_count as usize,
        detail.parents.len()
    );
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
}
#[test]
fn merged_mode_uses_topological_pinned_read_with_same_limits_and_no_mutation() {
    let root = crate::workspace::tests::fixture();
    let before = crate::workspace::tests::snapshot(&root);
    let snapshot = read_mode(&root, &AtomicBool::new(false), true).unwrap();
    assert_eq!(snapshot.commits[0].oid, snapshot.head);
    assert!(snapshot.commits.len() <= MAX_COMMITS);
    let command = log_mode(&root, &snapshot.head, true);
    let args = command
        .get_args()
        .map(|arg| arg.to_str().unwrap())
        .collect::<Vec<_>>();
    assert!(args.contains(&"--topo-order"));
    assert!(!args.contains(&"--first-parent"));
    assert!(args.contains(&"--max-count=101"));
    assert_eq!(args[args.len() - 2], snapshot.head);
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    assert!(
        read_mode(&root, &AtomicBool::new(true), true)
            .unwrap_err()
            .contains("cancelled")
    );
}
#[test]
fn existing_merge_fixture_includes_side_history_only_in_explicit_merged_mode() {
    let root = crate::workspace::tests::fixture();
    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["log", "--merges", "--max-count=1", "--format=%H %P"])
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    let ids = text.split_whitespace().collect::<Vec<_>>();
    assert!(
        ids.len() >= 3,
        "fixture needs an existing local merge; no commits created"
    );
    fs::write(root.join(".git/HEAD"), format!("{}\n", ids[0])).unwrap();
    let before = crate::workspace::tests::snapshot(&root);
    let first = read(&root, &AtomicBool::new(false)).unwrap();
    let merged = read_mode(&root, &AtomicBool::new(false), true).unwrap();
    assert_eq!(first.head, merged.head);
    assert_eq!(merged.head, ids[0]);
    assert!(merged.commits.iter().any(|commit| commit.oid == ids[2]));
    assert!(
        merged.commits.iter().any(|commit| !first
            .commits
            .iter()
            .any(|ordinary| ordinary.oid == commit.oid)),
        "side history must add real local objects"
    );
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
}
#[test]
fn maximum_parent_metadata_is_bounded_by_wire_before_partial_result() {
    let mut bytes = Vec::new();
    for n in 0..=MAX_COMMITS {
        let parents = (0..16)
            .map(|parent| format!("{:040x}", 1000 + parent))
            .collect::<Vec<_>>()
            .join(" ");
        bytes.extend(format!("{n:040x}\x001\x00{parents}\x00s\x00\n").as_bytes());
    }
    let start = Instant::now();
    for _ in 0..100 {
        assert!(parse(&bytes, format!("{:040x}", 0)).is_ok());
    }
    eprintln!(
        "100x101sixteenparentparse {:?}, wire{}B",
        start.elapsed(),
        bytes.len()
    );
    let mut oversized = bytes;
    oversized.extend(vec![b'x'; MAX_BYTES]);
    assert!(parse(&oversized, format!("{:040x}", 0)).is_err());
}
#[test]
fn commands_are_pinned_local_no_patch_notes_signatures_mailmap_or_write() {
    let root = Path::new("/fixture");
    let command = log(root, &head());
    let args = command
        .get_args()
        .map(|arg| arg.to_str().unwrap())
        .collect::<Vec<_>>();
    for expected in [
        "--first-parent",
        "--max-count=101",
        "--no-patch",
        "--no-show-signature",
        "--no-notes",
        "--no-use-mailmap",
        "--no-ext-diff",
        "--no-textconv",
        "--end-of-options",
    ] {
        assert!(args.contains(&expected));
    }
    assert_eq!(args[args.len() - 2], head());
    assert_eq!(args.last(), Some(&"--"));
    assert!(
        command
            .get_envs()
            .any(|(key, value)| key == "GIT_NO_REPLACE_OBJECTS"
                && value == Some(std::ffi::OsStr::new("1")))
    );
    assert!(
        command
            .get_envs()
            .any(|(key, value)| key == "GIT_NO_LAZY_FETCH"
                && value == Some(std::ffi::OsStr::new("1")))
    );
}
#[test]
fn real_existing_local_history_detached_unborn_and_cancel_never_change_head_index_or_status() {
    let root = crate::workspace::tests::fixture();
    let before = crate::workspace::tests::snapshot(&root);
    let cancel = AtomicBool::new(false);
    let snapshot = read(&root, &cancel).unwrap();
    assert!(!snapshot.commits.is_empty());
    assert_eq!(snapshot.commits[0].oid, snapshot.head);
    assert!(snapshot.commits.len() <= MAX_COMMITS);
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    let original = fs::read(root.join(".git/HEAD")).unwrap();
    fs::write(root.join(".git/HEAD"), format!("{}\n", snapshot.head)).unwrap();
    let before = crate::workspace::tests::snapshot(&root);
    assert_eq!(read(&root, &cancel).unwrap().head, snapshot.head);
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    fs::write(root.join(".git/HEAD"), "ref: refs/heads/unborn-history\n").unwrap();
    let index = fs::read(root.join(".git/index")).unwrap();
    assert!(
        read(&root, &cancel)
            .unwrap_err()
            .contains("no readable commit")
    );
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    fs::write(root.join(".git/HEAD"), original).unwrap();
    cancel.store(true, Ordering::Release);
    assert!(read(&root, &cancel).unwrap_err().contains("cancelled"));
    assert!(read(&root.join("missing"), &AtomicBool::new(false)).is_err());
}
#[test]
fn object_only_history_does_not_execute_configured_worktree_filters_or_external_diff() {
    let root = crate::workspace::tests::fixture();
    let index = fs::read(root.join(".git/index")).unwrap();
    let head = fs::read(root.join(".git/HEAD")).unwrap();
    let config = root.join(".git/config");
    let mut text = fs::read_to_string(&config).unwrap();
    text.push_str("\n[filter \"history-unsafe\"]\n clean = touch .git/history-filter-ran\n process = touch .git/history-filter-ran\n[diff]\n external = touch .git/history-diff-ran\n[log]\n showSignature = true\n");
    fs::write(config, text).unwrap();
    fs::write(
        root.join(".git/info/attributes"),
        "* filter=history-unsafe\n",
    )
    .unwrap();
    assert!(read(&root, &AtomicBool::new(false)).is_ok());
    assert!(!root.join(".git/history-filter-ran").exists());
    assert!(!root.join(".git/history-diff-ran").exists());
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(root.join(".git/HEAD")).unwrap(), head);
}
#[cfg(unix)]
#[test]
fn supervised_history_child_deadline_cancel_and_output_overflow_reap_direct_child() {
    use std::process::Stdio;
    for (script, cancel, timeout, expected) in [
        (
            "exec sleep 30",
            false,
            Duration::from_millis(30),
            "timed out",
        ),
        ("exec sleep 30", true, Duration::from_secs(1), "cancelled"),
        (
            "while :; do printf '0123456789abcdef'; done",
            false,
            Duration::from_secs(2),
            "too large",
        ),
    ] {
        let mut child = Command::new("sh")
            .args(["-c", script])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let result = read_only::supervise(
            &mut child,
            &AtomicBool::new(cancel),
            Instant::now() + timeout,
            MAX_BYTES,
            "History fixture",
            "too large",
        );
        assert!(result.unwrap_err().contains(expected));
        assert!(child.try_wait().unwrap().is_some());
    }
}
