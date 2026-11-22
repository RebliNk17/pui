use super::*;
use std::io::Write;
use std::process::Stdio;

fn command(root: &Path, args: &[&str]) -> Vec<u8> {
    let out = git(root, args).unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}
fn repo() -> PathBuf {
    let root = super::super::tests::dir();
    command(&root, &["init", "-b", "main"]);
    fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
    for name in [
        "modified",
        "deleted",
        "staged-delete",
        "rename-from",
        "typechange",
    ] {
        fs::write(root.join(name), "committed\n").unwrap();
    }
    fs::create_dir(root.join("sub")).unwrap();
    fs::write(root.join("sub/file"), "subfolder\n").unwrap();
    command(&root, &["add", "."]);
    // Only disposable test repositories receive a fixture commit.
    command(
        &root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "fixture",
        ],
    );
    root
}
fn head(root: &Path) -> String {
    String::from_utf8(command(root, &["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .into()
}
fn status(root: &Path) -> Vec<u8> {
    command(
        root,
        &["status", "--porcelain=v1", "--untracked-files=all", "-z"],
    )
}
fn prepare(root: &Path) -> (PathBuf, bool) {
    super::super::prepare(root, "snapshot", &super::super::tests::dir(), &head(root)).unwrap()
}
#[test]
fn dirty_snapshot_preserves_working_files_index_and_original_checkout() {
    let root = repo();
    fs::write(root.join("modified"), "staged\n").unwrap();
    command(&root, &["add", "modified"]);
    fs::write(root.join("modified"), "unstaged after staging\n").unwrap();
    fs::remove_file(root.join("deleted")).unwrap();
    command(&root, &["rm", "staged-delete"]);
    // A staged deletion with an ignored working file must not lose that file.
    fs::write(root.join("staged-delete"), "recreated\n").unwrap();
    fs::write(root.join(".gitignore"), "ignored/\nstaged-delete\n").unwrap();
    command(&root, &["mv", "rename-from", "rename-to"]);
    fs::write(root.join("added"), "staged new\n").unwrap();
    command(&root, &["add", "added"]);
    fs::write(root.join("added"), "new unstaged\n").unwrap();
    fs::create_dir(root.join("new folder")).unwrap();
    fs::write(root.join("new folder/file\nwith-tab\t"), b"binary\0payload").unwrap();
    fs::create_dir(root.join("ignored")).unwrap();
    fs::write(root.join("ignored/secret"), "do not copy").unwrap();
    let before_status = status(&root);
    let before_index = fs::read(root.join(".git/index")).unwrap();
    let before = capture(&root, None).unwrap();
    let original_head = head(&root);
    let (target, isolated) = prepare(&root);
    assert!(isolated);
    assert_eq!(capture(&target, None).unwrap(), before);
    assert_eq!(status(&target), before_status);
    assert_eq!(head(&target), original_head);
    assert_eq!(head(&root), original_head);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), before_index);
    assert_eq!(status(&root), before_status);
    assert!(!target.join("ignored").exists());
    assert!(!target.join("deleted").exists());
    assert_eq!(
        fs::read(target.join("new folder/file\nwith-tab\t")).unwrap(),
        b"binary\0payload"
    );
    fs::write(target.join("modified"), "workflow edit\n").unwrap();
    assert_eq!(
        fs::read_to_string(root.join("modified")).unwrap(),
        "unstaged after staging\n"
    );
}
#[test]
fn clean_repository_and_new_source_subfolder_get_isolated_worktrees() {
    let root = repo();
    let (target, isolated) = prepare(&root);
    assert!(isolated);
    assert_eq!(
        capture(&target, None).unwrap(),
        capture(&root, None).unwrap()
    );
    assert!(status(&target).is_empty());
    fs::create_dir(root.join("new-source")).unwrap();
    fs::write(root.join("new-source/file"), "new subfolder").unwrap();
    let target = super::super::prepare(
        &root.join("new-source"),
        "subfolder",
        &super::super::tests::dir(),
        &head(&root),
    )
    .unwrap()
    .0;
    assert_eq!(target.file_name().unwrap(), "new-source");
    assert_eq!(
        fs::read_to_string(target.join("file")).unwrap(),
        "new subfolder"
    );
}
#[test]
fn subfolder_evidence_scope_is_the_retained_worktree_root() {
    let root = repo();
    let target = super::super::prepare(
        &root.join("sub"),
        "scope",
        &super::super::tests::dir(),
        &head(&root),
    )
    .unwrap()
    .0;
    let scope = super::super::fingerprint_root(&target).unwrap();
    assert_eq!(scope, fs::canonicalize(target.parent().unwrap()).unwrap());
    assert_ne!(scope, fs::canonicalize(&root).unwrap());
    assert_eq!(
        super::super::fingerprint(&scope).unwrap(),
        super::super::fingerprint(&target).unwrap()
    );
    let before = super::super::fingerprint(&target).unwrap();
    fs::write(scope.join("modified"), "changed outside selected subfolder").unwrap();
    assert_ne!(before, super::super::fingerprint(&target).unwrap());
}

#[test]
fn prepared_file_to_directory_replacement_has_stable_content_sensitive_fingerprint() {
    let root = repo();
    fs::remove_file(root.join("typechange")).unwrap();
    fs::create_dir(root.join("typechange")).unwrap();
    fs::write(root.join("typechange/child"), "replacement directory").unwrap();
    let original_status = status(&root);
    let original_index = fs::read(root.join(".git/index")).unwrap();
    let (target, isolated) = prepare(&root);
    assert!(isolated);
    let fingerprint = super::super::fingerprint(&target)
        .expect("prepared file-to-directory replacement must fingerprint");
    assert_eq!(fingerprint, super::super::fingerprint(&target).unwrap());
    assert_eq!(fingerprint, super::super::fingerprint(&root).unwrap());
    fs::write(target.join("typechange/child"), "changed replacement").unwrap();
    assert_ne!(fingerprint, super::super::fingerprint(&target).unwrap());
    assert_eq!(status(&root), original_status);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), original_index);
}

#[test]
fn prepared_directory_to_file_replacement_has_stable_content_sensitive_fingerprint() {
    let root = repo();
    fs::remove_file(root.join("sub/file")).unwrap();
    fs::remove_dir(root.join("sub")).unwrap();
    fs::write(root.join("sub"), "replacement file").unwrap();
    let original_status = status(&root);
    let original_index = fs::read(root.join(".git/index")).unwrap();
    let (target, isolated) = prepare(&root);
    assert!(isolated);
    let fingerprint = super::super::fingerprint(&target)
        .expect("prepared directory-to-file replacement must fingerprint");
    assert_eq!(fingerprint, super::super::fingerprint(&target).unwrap());
    assert_eq!(fingerprint, super::super::fingerprint(&root).unwrap());
    fs::write(target.join("sub"), "changed replacement").unwrap();
    assert_ne!(fingerprint, super::super::fingerprint(&target).unwrap());
    assert_eq!(status(&root), original_status);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), original_index);
}

#[test]
fn fingerprint_distinguishes_empty_directory_replacement_deletion_and_ignored_contents() {
    let root = repo();
    let original = super::super::fingerprint(&root).unwrap();
    fs::remove_file(root.join("typechange")).unwrap();
    let deleted = super::super::fingerprint(&root).unwrap();
    assert_ne!(original, deleted);
    fs::create_dir(root.join("typechange")).unwrap();
    let directory = super::super::fingerprint(&root).unwrap();
    assert_ne!(deleted, directory);
    fs::write(root.join(".gitignore"), "ignored/\ntypechange/ignored\n").unwrap();
    let ignored = super::super::fingerprint(&root).unwrap();
    fs::write(root.join("typechange/ignored"), "excluded content").unwrap();
    assert_eq!(ignored, super::super::fingerprint(&root).unwrap());
    fs::write(root.join("typechange/included"), "included content").unwrap();
    assert_ne!(ignored, super::super::fingerprint(&root).unwrap());
}

#[cfg(unix)]
#[test]
fn fingerprint_rejects_symlinked_tracked_ancestors_even_above_replacement_files() {
    let root = repo();
    fs::create_dir(root.join("sub/deeper")).unwrap();
    fs::write(root.join("sub/deeper/child"), "tracked child").unwrap();
    command(&root, &["add", "sub/deeper/child"]);
    let outside = super::super::tests::dir();
    fs::write(outside.join("deeper"), "outside replacement file").unwrap();
    fs::rename(root.join("sub"), root.join("retained-sub")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("sub")).unwrap();
    let error = super::super::fingerprint(&root).unwrap_err();
    assert!(error.contains("Symlinked source directory"), "{error}");
}

#[test]
fn fingerprint_refuses_opaque_gitlink_and_embedded_repository_directories() {
    let root = repo();
    let (target, _) = prepare(&root);
    // An indexed gitlink introduced after preparation must not be hashed as
    // just a directory name, even without a .git marker in its working tree.
    fs::create_dir(target.join("opaque-module")).unwrap();
    fs::write(target.join("opaque-module/file"), "not enumerated by Git").unwrap();
    command(
        &target,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            "160000",
            &head(&root),
            "opaque-module",
        ],
    );
    assert!(
        super::super::fingerprint(&target)
            .unwrap_err()
            .contains("Opaque source directory")
    );
    command(
        &target,
        &["update-index", "--force-remove", "opaque-module"],
    );
    // Untracked embedded repositories are likewise opaque to ls-files.
    fs::create_dir(target.join("embedded")).unwrap();
    command(&target.join("embedded"), &["init", "-b", "main"]);
    fs::write(target.join("embedded/file"), "nested content").unwrap();
    assert!(
        super::super::fingerprint(&target)
            .unwrap_err()
            .contains("Nested repositories")
    );
}

#[test]
fn fingerprint_refuses_embedded_repository_in_tracked_file_replacement() {
    let root = repo();
    let (target, _) = prepare(&root);
    fs::remove_file(target.join("typechange")).unwrap();
    fs::create_dir(target.join("typechange")).unwrap();
    command(&target.join("typechange"), &["init", "-b", "main"]);
    fs::write(target.join("typechange/file"), "nested replacement").unwrap();
    assert!(
        super::super::fingerprint(&target)
            .unwrap_err()
            .contains("Nested repositories")
    );
}

#[cfg(unix)]
#[test]
fn snapshot_preserves_symlinks_executable_bits_and_file_directory_replacements() {
    use std::os::unix::{fs::PermissionsExt, fs::symlink};
    let root = repo();
    fs::remove_file(root.join("typechange")).unwrap();
    fs::create_dir(root.join("typechange")).unwrap();
    fs::write(root.join("typechange/child"), "replacement directory").unwrap();
    fs::remove_file(root.join("sub/file")).unwrap();
    fs::remove_dir(root.join("sub")).unwrap();
    fs::write(root.join("sub"), "replacement file").unwrap();
    fs::set_permissions(root.join("modified"), fs::Permissions::from_mode(0o755)).unwrap();
    let outside = super::super::tests::dir();
    fs::write(outside.join("file"), "external").unwrap();
    symlink(&outside, root.join("link")).unwrap();
    symlink("missing", root.join("broken-link")).unwrap();
    let (target, _) = prepare(&root);
    assert_eq!(fs::read_link(target.join("link")).unwrap(), outside);
    assert_eq!(
        fs::read_link(target.join("broken-link")).unwrap(),
        PathBuf::from("missing")
    );
    assert_eq!(
        fs::metadata(target.join("modified"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(
        fs::read_to_string(target.join("typechange/child")).unwrap(),
        "replacement directory"
    );
    assert_eq!(
        fs::read_to_string(target.join("sub")).unwrap(),
        "replacement file"
    );
}
#[cfg(unix)]
#[test]
fn snapshot_refuses_symlinked_tracked_parents_without_writing_outside() {
    let root = repo();
    let outside = super::super::tests::dir();
    fs::write(outside.join("file"), "external").unwrap();
    fs::remove_file(root.join("sub/file")).unwrap();
    fs::remove_dir(root.join("sub")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("sub")).unwrap();
    let home = super::super::tests::dir();
    let error = super::super::prepare(&root, "unsafe", &home, &head(&root)).unwrap_err();
    assert!(error.contains("Symlinked source directory"));
    assert!(!home.join("task-unsafe").exists());
    assert_eq!(
        fs::read_to_string(outside.join("file")).unwrap(),
        "external"
    );
}
#[test]
fn head_changes_conflicts_and_oversized_sources_fail_closed() {
    let root = repo();
    let home = super::super::tests::dir();
    assert!(
        super::super::prepare(&root, "wrong-head", &home, "not-head")
            .unwrap_err()
            .contains("HEAD changed")
    );
    assert!(!home.join("task-wrong-head").exists());
    let file = fs::File::create(root.join("huge")).unwrap();
    file.set_len(MAX_BYTES + 1).unwrap();
    assert!(
        super::super::prepare(&root, "oversized", &home, &head(&root))
            .unwrap_err()
            .contains("snapshot budget")
    );
    assert!(!home.join("task-oversized").exists());
    fs::remove_file(root.join("huge")).unwrap();
    let blob = String::from_utf8(command(&root, &["rev-parse", "HEAD:modified"])).unwrap();
    command(&root, &["update-index", "--force-remove", "modified"]);
    let mut child = Command::new("git")
        .args(["update-index", "--index-info"])
        .current_dir(&root)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            format!(
                "100644 {} 1\tmodified\n100644 {} 2\tmodified\n",
                blob.trim(),
                blob.trim()
            )
            .as_bytes(),
        )
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert!(
        super::super::prepare(&root, "conflict", &home, &head(&root))
            .unwrap_err()
            .contains("unmerged")
    );
}
#[test]
fn index_flags_and_intent_to_add_are_preserved_exactly() {
    let root = repo();
    fs::write(root.join("intent"), "not staged yet").unwrap();
    command(&root, &["add", "-N", "intent"]);
    command(&root, &["update-index", "--assume-unchanged", "modified"]);
    command(&root, &["update-index", "--skip-worktree", "deleted"]);
    let index = fs::read(root.join(".git/index")).unwrap();
    let before_status = status(&root);
    let (target, _) = prepare(&root);
    assert_eq!(fs::read(index_path(&target).unwrap()).unwrap(), index);
    assert_eq!(status(&target), before_status);
    assert_eq!(
        command(&target, &["ls-files", "-v"]),
        command(&root, &["ls-files", "-v"])
    );
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
}
#[cfg(unix)]
#[test]
fn preparation_never_executes_repository_hooks() {
    use std::os::unix::fs::PermissionsExt;
    let root = repo();
    let hooks = root.join(".git/hooks");
    for name in ["post-checkout", "post-index-change"] {
        let path = hooks.join(name);
        fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf executed > '{}'\n",
                root.join("hook-ran").display()
            ),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let monitor = root.join(".git/fsmonitor-test");
    fs::write(
        &monitor,
        format!(
            "#!/bin/sh\nprintf executed > '{}'\n",
            root.join("monitor-ran").display()
        ),
    )
    .unwrap();
    fs::set_permissions(&monitor, fs::Permissions::from_mode(0o755)).unwrap();
    command(
        &root,
        &["config", "core.fsmonitor", monitor.to_str().unwrap()],
    );
    fs::write(root.join("modified"), "dirty\n").unwrap();
    let (target, _) = prepare(&root);
    assert!(!root.join("monitor-ran").exists());
    assert!(!root.join("hook-ran").exists());
    assert!(!target.join("hook-ran").exists());
}
#[test]
fn split_and_sparse_indexes_are_explicitly_rejected() {
    let root = repo();
    command(&root, &["update-index", "--split-index"]);
    assert!(capture(&root, None).unwrap_err().contains("Split indexes"));
    command(&root, &["update-index", "--no-split-index"]);
    command(
        &root,
        &["sparse-checkout", "init", "--cone", "--sparse-index"],
    );
    command(&root, &["sparse-checkout", "set", "missing-folder"]);
    assert!(
        capture(&root, None)
            .unwrap_err()
            .contains("Sparse directory indexes")
    );
}
#[test]
fn snapshot_rejects_submodules_and_unsafe_paths() {
    let root = repo();
    command(
        &root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},module", head(&root)),
        ],
    );
    assert!(capture(&root, None).unwrap_err().contains("submodules"));
    for name in [
        "/absolute",
        "../outside",
        ".git/config",
        "safe/.GiT/config",
        "",
    ] {
        assert!(relative(name.as_bytes()).is_err(), "{name}");
    }
    assert!(relative(b"ordinary/path").is_ok());
}
