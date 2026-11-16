use super::*;
use std::{fs, sync::atomic::Ordering};

#[test]
fn status_parser_bounds_paths_bytes_utf8_and_framing_without_partial_results() {
    let mut bytes = Vec::new();
    for n in 0..MAX_FILES {
        bytes.extend_from_slice(format!(" M src/{n}.rs\0").as_bytes());
    }
    assert_eq!(status(&bytes).unwrap().len(), MAX_FILES);
    bytes.extend_from_slice(b"?? extra\0");
    assert!(status(&bytes).unwrap_err().contains("256 paths"));
    let mut bytes = Vec::new();
    for n in 0..17 {
        bytes.extend_from_slice(format!(" M {n:02}{}\0", "x".repeat(4094)).as_bytes());
    }
    assert!(status(&bytes).unwrap_err().contains("64 KiB"));
    for bytes in [
        b" M bad\xff\0".as_slice(),
        b" M ../outside\0",
        b" M file\nname\0",
        b" M ok",
        b" M ok\0\0",
        b"R  target\0",
        b"!x path\0",
    ] {
        assert!(status(bytes).is_err(), "{bytes:?}");
    }
    let files = status(b"R  new[abc]*.rs\0old.rs\0?? untracked\0").unwrap();
    assert_eq!(
        files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["new[abc]*.rs", "untracked"]
    );
    let mut files = status(b"R  target\0source\0 M other\0").unwrap();
    stats(b"12\t3\t\0source\0target\x001\t0\tother\0", &mut files).unwrap();
    assert_eq!(
        (&files[0].added, &files[0].removed),
        (&"12".to_owned(), &"3".to_owned())
    );
    assert_eq!(files[1].added, "1");
    assert!(stats(b"not-number\t0\tother\0", &mut files).is_err());
    assert!(line(b"/root\n", MAX_ROOT_BYTES, "root").is_ok());
    assert!(line(b"/root\nextra\n", MAX_ROOT_BYTES, "root").is_err());
    assert!(line(b"invalid\xff\n", MAX_ROOT_BYTES, "root").is_err());
}

#[test]
fn real_status_errors_unborn_and_oversize_are_honest_and_never_mutate_index_head() {
    let root = tests::fixture();
    let cancel = AtomicBool::new(false);
    let before = tests::snapshot(&root);
    let workspace = read(&root, &cancel);
    assert!(
        workspace.is_git && workspace.error.is_none(),
        "{:?}",
        workspace.error
    );
    assert_eq!(workspace.files.len(), 1);
    assert_eq!(workspace.files[0].path, "README.md");
    assert_ne!(workspace.files[0].status, "??");
    assert_eq!(tests::snapshot(&root), before);
    let empty = std::env::temp_dir().join(format!("pui-status-unborn-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&empty).unwrap();
    assert!(
        read_only::run(
            read_only::command(&empty).args(["init", "--quiet"]),
            &cancel,
            Instant::now() + DEADLINE,
            1024,
            "fixture init",
            "fixture output too large"
        )
        .unwrap()
        .1
        .success()
    );
    fs::write(empty.join("untracked"), "fixture").unwrap();
    let head = fs::read(empty.join(".git/HEAD")).unwrap();
    let unborn = read(&empty, &cancel);
    assert!(
        unborn.is_git
            && unborn
                .error
                .as_deref()
                .unwrap()
                .contains("counts unavailable")
    );
    assert_eq!(unborn.files.len(), 1);
    assert!(
        unborn
            .files
            .iter()
            .all(|file| file.added == "?" && file.removed == "?")
    );
    assert!(!empty.join(".git/index").exists());
    assert_eq!(fs::read(empty.join(".git/HEAD")).unwrap(), head);
    for n in 0..MAX_FILES {
        fs::write(
            root.join(format!("pui-untracked-{n:04}")),
            "private fixture\n",
        )
        .unwrap();
    }
    let before = tests::snapshot(&root);
    let workspace = read(&root, &cancel);
    assert!(workspace.is_git && workspace.files.is_empty());
    assert!(workspace.error.as_deref().unwrap().contains("256 paths"));
    assert_eq!(tests::snapshot(&root), before);
    cancel.store(true, Ordering::Release);
    let workspace = read(&root, &cancel);
    assert!(workspace.error.as_deref().unwrap().contains("cancelled"));
    assert!(workspace.files.is_empty());
    assert_eq!(tests::snapshot(&root), before);
}
