use super::*;
#[test]
fn literal_patch_limits_controls_and_empty_are_whole_or_reject() {
    for text in ["", "diff --git a/x b/x\n@@ -1 +1 @@\n-old\n+世界\t=1+2\r\n"] {
        assert_eq!(parse(text.as_bytes().to_vec()).unwrap(), text);
    }
    for bytes in [
        vec![0xff],
        vec![b'x'; MAX_BYTES + 1],
        b"+bad\x1b[31m\n".to_vec(),
        "x\n".repeat(MAX_LINES + 1).into_bytes(),
    ] {
        assert!(parse(bytes).is_err());
    }
    assert!(parse(vec![b'x'; MAX_BYTES]).is_ok());
}
#[test]
fn per_file_patch_is_literal_bounded_and_never_mutates_git() {
    let root = crate::workspace::tests::history_fixture();
    let cancel = AtomicBool::new(false);
    let snapshot = super::super::read(&root, &cancel).unwrap();
    let detail = super::super::details::read(&root, &snapshot.head, &cancel).unwrap();
    let before = crate::workspace::tests::snapshot(&root);
    let parent = detail.parents.first().map(String::as_str);
    assert_eq!(
        read_path(&root, &detail.oid, parent, Some(":(glob)**"), &cancel).unwrap(),
        ""
    );
    assert_eq!(
        read_path(
            &root,
            &detail.oid,
            parent,
            Some("not-present\n\t'*.rs"),
            &cancel
        )
        .unwrap(),
        ""
    );
    for path in ["../README.md", "/etc/passwd", "a//b", "", "a\0b"] {
        assert!(read_path(&root, &detail.oid, parent, Some(path), &cancel).is_err());
    }
    let files = super::super::files::read(&root, &detail.oid, parent, &cancel).unwrap();
    let small = files
        .iter()
        .find_map(|file| {
            read_path(&root, &detail.oid, parent, Some(&file.path), &cancel)
                .ok()
                .filter(|patch| !patch.is_empty())
        })
        .unwrap();
    assert!(small.contains("diff --git"));
    assert!(small.len() <= MAX_BYTES);
    assert_eq!(before, crate::workspace::tests::snapshot(&root));
}
#[test]
fn real_commit_patch_root_missing_cancel_and_arguments_never_mutate_git() {
    let root = crate::workspace::tests::history_fixture();
    let cancel = AtomicBool::new(false);
    let snapshot = super::super::read(&root, &cancel).unwrap();
    // Pick an existing small commit instead of assuming the integration merge fits.
    let detail = snapshot
        .commits
        .iter()
        .find_map(|commit| {
            let detail = super::super::details::read(&root, &commit.oid, &cancel).ok()?;
            read(
                &root,
                &detail.oid,
                detail.parents.first().map(String::as_str),
                &cancel,
            )
            .ok()
            .filter(|text| !text.is_empty())
            .map(|_| detail)
        })
        .expect("an existing bounded nonempty patch in local history");
    let before = crate::workspace::tests::snapshot(&root);
    let text = read(
        &root,
        &detail.oid,
        detail.parents.first().map(String::as_str),
        &cancel,
    )
    .unwrap();
    assert!(text.contains("diff --git"));
    assert!(text.len() <= MAX_BYTES);
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
    let args = command(&root, &detail.oid, None)
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    for flag in [
        "--root",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "--no-renames",
        "--end-of-options",
    ] {
        assert!(args.iter().any(|a| a == flag));
    }
    assert_eq!(before, crate::workspace::tests::snapshot(&root));
}
