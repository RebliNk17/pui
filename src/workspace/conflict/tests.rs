use super::*;
#[test]
fn real_merge_conflict_sides_are_exact_object_only_and_index_unchanged() {
    let root = std::env::temp_dir().join(format!("pui-conflict-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let git = crate::workspace::tests::git;
    git(&root, &["init", "-b", "main"]);
    git(&root, &["config", "user.name", "Fixture"]);
    git(&root, &["config", "user.email", "f@example.invalid"]);
    let save = |s: &str| {
        std::fs::write(root.join("conflict.txt"), s).unwrap();
        git(&root, &["add", "conflict.txt"]);
        git(&root, &["-c", "commit.gpgsign=false", "commit", "-qm", s]);
    };
    save("base\n");
    git(&root, &["switch", "-c", "other"]);
    save("theirs\n");
    git(&root, &["switch", "main"]);
    save("ours\n");
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "merge",
            "--no-edit",
            "other",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let before = crate::workspace::tests::snapshot(&root);
    let file = std::fs::read(root.join("conflict.txt")).unwrap();
    let review = inspect(&root, "conflict.txt", &AtomicBool::new(false)).unwrap();
    assert_eq!(review.sides[0].as_ref().unwrap().text, "base\n");
    assert_eq!(review.sides[1].as_ref().unwrap().text, "ours\n");
    assert_eq!(review.sides[2].as_ref().unwrap().text, "theirs\n");
    assert_eq!(crate::workspace::tests::snapshot(&root), before);
    assert_eq!(std::fs::read(root.join("conflict.txt")).unwrap(), file);
    assert!(inspect(&root, "../conflict.txt", &AtomicBool::new(false)).is_err());
    assert!(inspect(&root, "conflict.txt", &AtomicBool::new(true)).is_err());
}
#[test]
fn parser_rejects_duplicate_path_and_special_modes() {
    let oid = "a".repeat(40);
    for raw in [
        format!("120000 {oid} 2\tf\0"),
        format!("100644 {oid} 2\tother\0"),
        format!("100644 {oid} 2\tf{nul}100644 {oid} 2\tf{nul}", nul = '\0'),
    ] {
        assert!(entries(raw.as_bytes(), "f").is_err());
    }
    let raw = format!("100644 {oid} 2\tf\0");
    let result = entries(raw.as_bytes(), "f").unwrap();
    assert!(result[0].is_none());
    assert!(result[1].is_some());
    assert!(result[2].is_none());
}
