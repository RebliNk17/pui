use super::*;
use std::{fs, process::Command};

pub(crate) fn file(path: &str) -> FileChange {
    FileChange {
        path: path.into(),
        status: " M".into(),
        ..Default::default()
    }
}

// Fixture repositories are seeds copied by concurrent test processes. Git's
// detached auto-maintenance (after add/commit) can create and delete lock files
// in a seed while another process copies it, so fixtures never start it.
pub(crate) const NO_AUTO_MAINTENANCE: [&str; 4] =
    ["-c", "gc.auto=0", "-c", "maintenance.auto=false"];

pub(crate) fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let (bytes, status) = read_only::run(
        read_only::command(root)
            .args(NO_AUTO_MAINTENANCE)
            .args(args),
        &AtomicBool::new(false),
        Instant::now() + DEADLINE,
        1024 * 1024,
        "Git fixture",
        "Git fixture exceeds 1 MiB",
    )
    .unwrap();
    assert!(status.success(), "Git fixture failed: {args:?}");
    bytes
}

pub(crate) fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let ty = entry.file_type().unwrap();
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

// A tiny immutable seed decouples index/status deadlines from the live Pui
// checkout's file count and history. Build it once, entirely in disposable
// storage; tests still exercise real Git objects, merge ancestry and indexes.
pub(crate) fn fixture() -> PathBuf {
    static SEED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let clone_seed = SEED.get_or_init(|| {
        let root = history_fixture();
        fs::write(
            root.join("README.md"),
            (0..80)
                .map(|line| format!("fixture baseline line {line}\n"))
                .collect::<String>(),
        )
        .unwrap();
        git(&root, &["add", "--", "README.md"]);
        git(
            &root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "--quiet",
                "-m",
                "bounded index fixture",
            ],
        );
        // The immutable history has exactly README.md and side.txt. Prepare its
        // sparse index once, rather than querying/rewriting it for every clone.
        git(
            &root,
            &["update-index", "--skip-worktree", "--", "side.txt"],
        );
        let head = git(&root, &["rev-parse", "HEAD"]);
        let index = fs::read(root.join(".git/index")).unwrap();
        let template =
            std::env::temp_dir().join(format!("pui-git-review-seed-{}", uuid::Uuid::new_v4()));
        assert!(
            read_only::run(
                read_only::command(&std::env::current_dir().unwrap())
                    .args([
                        "clone",
                        "--template=",
                        "--quiet",
                        "--shared",
                        "--no-checkout",
                    ])
                    .arg(&root)
                    .arg(&template),
                &AtomicBool::new(false),
                Instant::now() + DEADLINE,
                1024,
                "fixture clone",
                "fixture output too large",
            )
            .unwrap()
            .1
            .success()
        );
        fs::create_dir_all(template.join(".git/hooks")).unwrap();
        fs::create_dir_all(template.join(".git/info")).unwrap();
        fs::write(template.join(".git/refs/heads/pui-fixture-root"), head).unwrap();
        fs::write(
            template.join(".git/HEAD"),
            "ref: refs/heads/pui-fixture-root\n",
        )
        .unwrap();
        fs::write(template.join(".git/index"), index).unwrap();
        fs::write(template.join("README.md"), "fixture modified README\n").unwrap();
        template
    });
    let root = std::env::temp_dir().join(format!("pui-git-review-{}", uuid::Uuid::new_v4()));
    copy_tree(clone_seed, &root);
    root
}

// A small deterministic local history, independent of the checkout's latest
// integration commit size or which side of a merge contains a bounded patch.
// Every write/commit is confined to this freshly created disposable repository.
pub(crate) fn history_fixture() -> PathBuf {
    static SEED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let seed = SEED.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("pui-history-seed-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_TERMINAL_PROMPT", "0")
                .args([
                    "-C",
                    root.to_str().unwrap(),
                    "-c",
                    "core.hooksPath=/dev/null",
                    "-c",
                    "gc.auto=0",
                    "-c",
                    "maintenance.auto=false",
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "history fixture command failed: {args:?}"
            );
        };
        run(&["init", "--quiet", "--template=", "--initial-branch=main"]);
        fs::create_dir_all(root.join(".git/hooks")).unwrap();
        fs::create_dir_all(root.join(".git/info")).unwrap();
        fs::write(root.join("README.md"), "base\n").unwrap();
        fs::write(root.join("side.txt"), "base\n").unwrap();
        run(&["add", "--", "README.md", "side.txt"]);
        run(&["commit", "--quiet", "-m", "base"]);
        run(&["branch", "side"]);
        fs::write(root.join("README.md"), "main change\n").unwrap();
        run(&["commit", "--quiet", "-am", "main change"]);
        run(&["checkout", "--quiet", "side"]);
        fs::write(root.join("side.txt"), "side change\n").unwrap();
        run(&["commit", "--quiet", "-am", "side change"]);
        run(&["checkout", "--quiet", "main"]);
        run(&[
            "merge",
            "--quiet",
            "--no-ff",
            "-m",
            "merge both small changes",
            "side",
        ]);
        root
    });
    let root = std::env::temp_dir().join(format!("pui-history-fixture-{}", uuid::Uuid::new_v4()));
    copy_tree(seed, &root);
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
fn fixture_clones_have_independent_sparse_indexes_refs_and_working_files() {
    let first = fixture();
    let second = fixture();
    let before = snapshot(&second);
    let second_file = fs::read(second.join("README.md")).unwrap();
    assert_eq!(
        git(&first, &["ls-files", "-v"]),
        b"H README.md\nS side.txt\n"
    );
    assert_eq!(
        git(&second, &["ls-files", "-v"]),
        b"H README.md\nS side.txt\n"
    );
    assert_eq!(before.0, b" M README.md\0");
    assert!(!first.join("side.txt").exists());
    assert!(!second.join("side.txt").exists());
    fs::write(first.join("README.md"), "first clone only\n").unwrap();
    git(&first, &["add", "--", "README.md"]);
    git(
        &first,
        &["update-index", "--no-skip-worktree", "--", "side.txt"],
    );
    git(
        &first,
        &["update-ref", "refs/heads/pui-fixture-root", "HEAD^"],
    );
    assert_eq!(snapshot(&second), before);
    assert_eq!(fs::read(second.join("README.md")).unwrap(), second_file);
    // A later fixture still starts from the immutable baseline, not first's edits.
    let third = fixture();
    assert_eq!(git(&third, &["status", "--porcelain=v1", "-z"]), before.0);
    assert_eq!(git(&third, &["rev-parse", "HEAD"]), before.2);
    assert_eq!(
        git(&third, &["ls-files", "-v"]),
        b"H README.md\nS side.txt\n"
    );
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
