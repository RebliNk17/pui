use super::*;
use std::fs;
use std::path::PathBuf;
fn fixture() -> (PathBuf, String, String) {
    let root = fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
    // A tiny valid PNG (1x1) and a modified version.
    let png1: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8,
        0xcf, 0xc0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x02, 0xfd, 0x75, 0x35, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let mut png2 = png1.to_vec();
    // flip one pixel byte to differ
    png2[33] ^= 0x01;
    fs::write(root.join("img.png"), png1).unwrap();
    crate::workspace::tests::git(&root, &["add", "img.png"]);
    crate::workspace::tests::git(
        &root,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e.i",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "add image",
        ],
    );
    let before = String::from_utf8(crate::workspace::tests::git(&root, &["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    fs::write(root.join("img.png"), png2).unwrap();
    crate::workspace::tests::git(&root, &["add", "img.png"]);
    crate::workspace::tests::git(
        &root,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@e.i",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "modify image",
        ],
    );
    let after = String::from_utf8(crate::workspace::tests::git(&root, &["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    (root, before, after)
}
#[test]
fn image_diff_extracts_before_after_bounded() {
    let (root, before, after) = fixture();
    let cancel = AtomicBool::new(false);
    let diff = read_image_diff(&root, &after, Some(&before), "img.png", &cancel).unwrap();
    assert!(!diff.before.is_empty());
    assert!(!diff.after.is_empty());
    assert_ne!(diff.before, diff.after);
    // root commit has no before.
    let root_only = read_image_diff(&root, &before, None, "img.png", &cancel).unwrap();
    assert!(root_only.before.is_empty());
    assert!(!root_only.after.is_empty());
}
#[test]
fn image_diff_refuses_bad_path_identity_and_nonimage() {
    let (root, before, after) = fixture();
    let cancel = AtomicBool::new(false);
    for bad in ["", "img", "a/b", "../img.png", "/img.png"] {
        assert!(read_image_diff(&root, &after, Some(&before), bad, &cancel).is_err());
    }
    assert!(read_image_diff(&root, &after, Some(&before), "README.md", &cancel).is_err());
    assert!(read_image_diff(&root, "bad", Some(&before), "img.png", &cancel).is_err());
    assert!(read_image_diff(&root, &after, Some("bad"), "img.png", &cancel).is_err());
}
