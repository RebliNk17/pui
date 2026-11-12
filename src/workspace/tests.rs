use super::*;
use std::fs;

pub(crate) fn file(path: &str) -> FileChange {
    FileChange {
        path: path.into(),
        status: " M".into(),
        ..Default::default()
    }
}

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .current_dir(root)
        .arg("--no-optional-locks")
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "Git fixture failed: {args:?}");
    output.stdout
}

// Setup writes only the disposable fixture's index using existing local objects.
// No commits, network, checkout, or changes to the source repository/index.
pub(crate) fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!("pui-git-review-{}", uuid::Uuid::new_v4()));
    assert!(
        Command::new("git")
            .args(["clone", "--quiet", "--shared", "--no-checkout"])
            .arg(std::env::current_dir().unwrap())
            .arg(&root)
            .status()
            .unwrap()
            .success()
    );
    git(&root, &["read-tree", "HEAD"]);
    fs::write(root.join("README.md"), "fixture modified README\n").unwrap();
    root
}

pub(crate) fn snapshot(root: &Path) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    (
        git(root, &["status", "--porcelain=v1", "-z"]),
        fs::read(root.join(".git/index")).unwrap(),
        git(root, &["rev-parse", "HEAD"]),
    )
}

#[test]
fn non_repository_is_handled() {
    let workspace = inspect(Path::new("/this-path-does-not-exist-pui"));
    assert!(!workspace.is_git);
    assert!(workspace.files.is_empty());
}

#[test]
fn bounded_literal_utf8_diff_and_failures_never_change_status_index_or_head() {
    let root = fixture();
    // A configured external driver/textconv would otherwise replace the patch.
    fs::write(root.join(".git/info/attributes"), "* diff=pui-fixture\n").unwrap();
    git(&root, &["config", "diff.pui-fixture.command", "false"]);
    git(&root, &["config", "diff.pui-fixture.textconv", "false"]);
    let oid = String::from_utf8(git(&root, &["rev-parse", "HEAD:README.md"])).unwrap();
    for path in ["literal[abc]*.md", "literalax.md"] {
        git(
            &root,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{},{}", oid.trim(), path),
            ],
        );
    }
    fs::write(root.join("literal[abc]*.md"), "literal target only\n").unwrap();
    fs::write(root.join("literalax.md"), "unrelated wildcard match\n").unwrap();
    let before = snapshot(&root);
    let patch = diff(&root, &file("literal[abc]*.md")).unwrap();
    assert!(patch.contains("+literal target only"));
    assert!(!patch.contains("unrelated wildcard match") && !patch.contains("literalax.md"));
    assert_eq!(snapshot(&root), before);

    for (bytes, error) in [
        (vec![b'x'; MAX_DIFF_BYTES + 1], "200,000-byte"),
        (b"not valid utf8: \xff\n".to_vec(), "not UTF-8"),
    ] {
        fs::write(root.join("README.md"), bytes).unwrap();
        let before = snapshot(&root);
        assert!(diff(&root, &file("README.md")).unwrap_err().contains(error));
        assert_eq!(snapshot(&root), before);
    }
    let before = snapshot(&root);
    for path in ["", "../outside", "/absolute", "file\nname", "a/../b"] {
        assert!(
            diff(&root, &file(path))
                .unwrap_err()
                .contains("relative file path")
        );
    }
    assert!(
        diff(&root.join("missing"), &file("README.md"))
            .unwrap_err()
            .contains("Cannot start")
    );
    assert_eq!(snapshot(&root), before);

    let empty = std::env::temp_dir().join(format!("pui-git-no-head-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&empty).unwrap();
    git(&empty, &["init", "--quiet"]);
    let before = git(&empty, &["status", "--porcelain=v1", "-z"]);
    assert!(
        diff(&empty, &file("README.md"))
            .unwrap_err()
            .contains("unavailable")
    );
    assert_eq!(git(&empty, &["status", "--porcelain=v1", "-z"]), before);
    assert!(!empty.join(".git/index").exists());
}

#[test]
fn untracked_read_never_opens_worktree_file() {
    let root = Path::new("/this-path-does-not-exist-pui");
    let mut file = file("untracked.txt");
    file.status = "??".into();
    assert!(diff(root, &file).unwrap().contains("Untracked"));
}
