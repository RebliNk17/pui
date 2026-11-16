use super::*;
#[test]
fn nul_paths_preserve_literal_unicode_newline_tabs_and_whole_refusal() {
    assert_eq!(
        parse(b"M\0a\nb\t.csv\0A\0=literal\0").unwrap(),
        vec![
            Entry {
                status: 'M',
                path: "a\nb\t.csv".into()
            },
            Entry {
                status: 'A',
                path: "=literal".into()
            }
        ]
    );
    assert!(parse(&[]).unwrap().is_empty());
    for bytes in [
        b"M\0x".to_vec(),
        b"M\0x\0A\0x\0".to_vec(),
        b"M\0../x\0".to_vec(),
        b"R100\0x\0y\0".to_vec(),
        b"M\0\0".to_vec(),
        b"M\0/x\0".to_vec(),
        vec![0xff],
        vec![b'a'; MAX_BYTES + 1],
        format!("M\0{}\0", "x".repeat(4097)).into_bytes(),
        "M\0x\0".repeat(257).into_bytes(),
    ] {
        assert!(parse(&bytes).is_err());
    }
}
#[test]
fn merge_second_parent_tree_and_per_file_patch_use_exact_base() {
    let root = crate::workspace::tests::fixture();
    let cancel = AtomicBool::new(false);
    let snapshot = super::super::read(&root, &cancel).unwrap();
    let (detail, first, second) = snapshot
        .commits
        .iter()
        .find_map(|c| {
            let d = super::super::details::read(&root, &c.oid, &cancel).ok()?;
            if d.parents.len() < 2 {
                return None;
            }
            let first = read(&root, &d.oid, Some(&d.parents[0]), &cancel).ok()?;
            let second = read(&root, &d.oid, Some(&d.parents[1]), &cancel).ok()?;
            (first != second && !second.is_empty()).then_some((d, first, second))
        })
        .expect("existing merge with two bounded distinct comparisons");
    let before = crate::workspace::tests::snapshot(&root);
    assert_ne!(first, second);
    let path = second
        .iter()
        .find_map(|e| {
            super::super::patch::read_path(
                &root,
                &detail.oid,
                Some(&detail.parents[1]),
                Some(&e.path),
                &cancel,
            )
            .ok()
            .filter(|patch| !patch.is_empty())
        })
        .unwrap();
    assert!(path.contains("diff --git"));
    assert_eq!(before, crate::workspace::tests::snapshot(&root));
}
#[test]
fn root_tree_comparison_uses_empty_base_and_is_bounded() {
    let root = crate::workspace::tests::fixture();
    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-list", "--max-parents=0", "HEAD"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let hash = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .next()
        .unwrap();
    let before = crate::workspace::tests::snapshot(&root);
    let entries = read(&root, hash, None, &AtomicBool::new(false)).unwrap();
    assert!(entries.iter().all(|e| e.status == 'A'));
    assert!(!entries.is_empty());
    assert_eq!(before, crate::workspace::tests::snapshot(&root));
}
#[test]
fn real_local_tree_comparison_missing_cancel_and_arguments_are_readonly() {
    let root = crate::workspace::tests::fixture();
    let cancel = AtomicBool::new(false);
    let snapshot = super::super::read(&root, &cancel).unwrap();
    let detail = super::super::details::read(&root, &snapshot.head, &cancel).unwrap();
    let before = crate::workspace::tests::snapshot(&root);
    let entries = read(
        &root,
        &detail.oid,
        detail.parents.first().map(String::as_str),
        &cancel,
    )
    .unwrap();
    assert!(!entries.is_empty());
    assert!(
        read(
            &root,
            &detail.oid,
            detail.parents.first().map(String::as_str),
            &AtomicBool::new(true)
        )
        .is_err()
    );
    assert!(read(&root, &"0".repeat(40), None, &cancel).is_err());
    assert!(read(&root, "--all", None, &cancel).is_err());
    assert_eq!(before, crate::workspace::tests::snapshot(&root));
    let cmd = command(
        &root,
        &detail.oid,
        detail.parents.first().map(String::as_str),
    );
    let args = cmd
        .get_args()
        .map(|a| a.to_string_lossy())
        .collect::<Vec<_>>();
    for flag in [
        "diff-tree",
        "--no-renames",
        "--no-ext-diff",
        "--no-textconv",
        "--end-of-options",
    ] {
        assert!(args.iter().any(|a| a == flag));
    }
    assert!(!args.iter().any(|a| a == "--stat" || a == "--patch"));
}
