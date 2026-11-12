use super::*;
use std::fs;

fn encoded(names: &[&str]) -> Vec<u8> {
    names
        .iter()
        .flat_map(|name| format!("refs/heads/{name}\0\n").into_bytes())
        .collect()
}
#[test]
fn strict_branch_parser_bounds_names_count_framing_and_utf8() {
    let state = Current::Local("main".into());
    let snapshot = parse(encoded(&["main", "topic/日本語"]), state.clone()).unwrap();
    assert_eq!(snapshot.local, ["main", "topic/日本語"]);
    for bytes in [
        b"refs/heads/invalid\xff\0\n".to_vec(),
        b"refs/heads/main\n".to_vec(),
        b"refs/heads/main\0".to_vec(),
        encoded(&["a", "a"]),
        encoded(&["z", "a"]),
        encoded(&["foo..bar"]),
        encoded(&["foo.lock"]),
        encoded(&["foo\tbar"]),
        encoded(&[".hidden"]),
        encoded(&["foo@{x}"]),
        encoded(&["foo//bar"]),
        encoded(&[&"x".repeat(MAX_NAME_BYTES)]),
    ] {
        assert!(parse(bytes, state.clone()).is_err());
    }
    let names: Vec<_> = (0..MAX_BRANCHES).map(|n| format!("b{n:04}")).collect();
    let bytes = encoded(&names.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(
        parse(bytes.clone(), Current::Detached).unwrap().local.len(),
        MAX_BRANCHES
    );
    let mut too_many = bytes;
    too_many.extend(encoded(&["z"]));
    assert!(parse(too_many, state).unwrap_err().contains("256 branches"));
    assert_eq!(
        current(Vec::new(), false, Some(1)).unwrap(),
        Current::Detached
    );
    assert!(current(b"refs/heads/invalid\xff\n".to_vec(), true, Some(0)).is_err());
    assert!(current(b"refs/heads/main\r\n".to_vec(), true, Some(0)).is_err());
    assert!(current(Vec::new(), false, Some(128)).is_err());
}
#[test]
fn production_arguments_are_literal_local_and_never_contain_branch_output() {
    for list in [false, true] {
        let command = command(Path::new("/private-fixture"), list);
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        assert_eq!(
            args,
            if list {
                vec![
                    "--no-optional-locks",
                    "--no-pager",
                    "-c",
                    "core.fsmonitor=false",
                    "for-each-ref",
                    "--sort=refname",
                    "--count=257",
                    "--format=%(refname)%00",
                    "refs/heads/",
                ]
            } else {
                vec![
                    "--no-optional-locks",
                    "--no-pager",
                    "-c",
                    "core.fsmonitor=false",
                    "symbolic-ref",
                    "--quiet",
                    "HEAD",
                ]
            }
        );
    }
}
#[test]
fn real_local_git_current_detached_unborn_errors_and_overlimit_are_read_only() {
    let root = crate::workspace::tests::fixture();
    let head = fs::read(root.join(".git/HEAD")).unwrap();
    let oid = Command::new("git")
        .current_dir(&root)
        .args(["--no-optional-locks", "rev-parse", "HEAD"])
        .output()
        .unwrap()
        .stdout;
    // Test setup writes refs only inside the private local clone, using existing
    // object ids; no new commits, checkout, source index writes or network.
    fs::create_dir_all(root.join(".git/refs/heads/pui-fixture")).unwrap();
    fs::write(root.join(".git/refs/heads/pui-fixture/local-only"), &oid).unwrap();
    let before = crate::workspace::tests::snapshot(&root);
    let cancel = AtomicBool::new(false);
    let snapshot = read(&root, &cancel).unwrap();
    assert!(matches!(snapshot.current, Current::Local(_)));
    assert!(
        snapshot
            .local
            .iter()
            .any(|name| name == "pui-fixture/local-only")
    );
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    assert_eq!(fs::read(root.join(".git/HEAD")).unwrap(), head);

    fs::write(root.join(".git/HEAD"), &oid).unwrap();
    let before = crate::workspace::tests::snapshot(&root);
    assert_eq!(read(&root, &cancel).unwrap().current, Current::Detached);
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    fs::write(root.join(".git/HEAD"), "ref: refs/heads/pui-unborn\n").unwrap();
    let index = fs::read(root.join(".git/index")).unwrap();
    let snapshot = read(&root, &cancel).unwrap();
    assert_eq!(snapshot.current, Current::Local("pui-unborn".into()));
    assert!(!snapshot.local.contains(&"pui-unborn".into()));
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    fs::write(root.join(".git/HEAD"), &head).unwrap();
    for n in 0..=MAX_BRANCHES {
        fs::write(
            root.join(format!(".git/refs/heads/pui-fixture/b{n:04}")),
            &oid,
        )
        .unwrap();
    }
    let before = crate::workspace::tests::snapshot(&root);
    assert!(read(&root, &cancel).unwrap_err().contains("256 branches"));
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    assert!(read(&root.join("missing-root"), &cancel).is_err());
    cancel.store(true, Ordering::Release);
    assert!(read(&root, &cancel).unwrap_err().contains("cancelled"));
}
#[cfg(unix)]
#[test]
fn supervisor_bounds_hung_child_cancel_and_pipe_bytes_then_reaps() {
    for (script, deadline, cancelled, expected) in [
        (
            "exec sleep 30",
            Duration::from_millis(30),
            false,
            "timed out",
        ),
        ("exec sleep 30", Duration::from_secs(1), true, "cancelled"),
        (
            "while :; do printf '0123456789abcdef0123456789abcdef'; done",
            Duration::from_secs(2),
            false,
            "128 KiB",
        ),
    ] {
        let mut child = Command::new("sh")
            .args(["-c", script])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let result = supervise(&mut child, &AtomicBool::new(cancelled), started + deadline);
        let _ = child.kill();
        child.wait().unwrap();
        assert!(result.unwrap_err().contains(expected));
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(child.try_wait().unwrap().is_some());
    }
}
