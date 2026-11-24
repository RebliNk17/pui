//! Explicit reviewed Git stash create, list, inspect, apply and drop.
//! Untracked files are never stashed or swept; apply and drop are separate OID-pinned actions.
use super::*;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, sync::atomic::AtomicBool};

const MAX_INDEX: usize = 16 * 1024 * 1024;
pub(crate) const MAX_STASHES: usize = 64;
const MAX_PATCH_BYTES: usize = 128 * 1024;
const MAX_MESSAGE_BYTES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub index: usize,
    pub oid: String,
    pub selector: String,
    pub summary: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryAction {
    Apply,
    Drop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PushReview {
    pub root: PathBuf,
    pub branch: String,
    pub head: String,
    pub message: String,
    pub files: Vec<String>,
    pub top_oid: Option<String>,
    index_path: PathBuf,
    index: Vec<u8>,
    status_bytes: Vec<u8>,
    file_digests: Vec<(String, Option<[u8; 32]>)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EntryReview {
    pub root: PathBuf,
    pub index: usize,
    pub selector: String,
    pub expected_oid: String,
    pub top_oid: String,
    pub action: EntryAction,
    pub summary: String,
    pub files: Vec<String>,
    pub preview: String,
}

pub(crate) fn valid_message(message: &str) -> bool {
    let trimmed = message.trim();
    trimmed.len() <= MAX_MESSAGE_BYTES && !trimmed.chars().any(char::is_control)
}

fn run(
    root: &Path,
    args: &[&str],
    cancel: &AtomicBool,
    deadline: Instant,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let (out, status) = read_only::run(
        read_only::command(root)
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .args(args),
        cancel,
        deadline,
        limit,
        "Git stash inspection",
        "Git stash response exceeds limit.",
    )?;
    if !status.success() {
        return Err("Git stash inspection unavailable.".into());
    }
    Ok(out)
}

fn bytes(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "Stash index unavailable.")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > max as u64 {
        return Err("Stash requires bounded regular files, not symlinks.".into());
    }
    let mut out = Vec::new();
    fs::File::open(path)
        .map_err(|_| "Stash index unavailable.")?
        .take(max as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|_| "Stash index read failed.")?;
    if out.len() > max {
        return Err("Stash index exceeds limit.".into());
    }
    Ok(out)
}

fn top_stash_oid(
    root: &Path,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Option<String>, String> {
    let (out, status) = read_only::run(
        read_only::command(root)
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .args(["rev-parse", "--verify", "--quiet", "refs/stash^{commit}"]),
        cancel,
        deadline,
        128,
        "Stash ref check",
        "Stash ref response exceeds limit.",
    )?;
    if !status.success() {
        return Ok(None);
    }
    let oid = line(&out, 128, "stash OID")?;
    if !history::oid(&oid) {
        return Err("Invalid refs/stash OID.".into());
    }
    Ok(Some(oid))
}

pub(crate) fn list(root: &Path, cancel: &AtomicBool) -> Result<Vec<Entry>, String> {
    if !root.is_absolute() || fs::canonicalize(root).map_err(|_| "Root unavailable.")? != root {
        return Err("Exact canonical repository root required.".into());
    }
    let deadline = Instant::now() + DEADLINE;
    let toplevel = line(
        &run(
            root,
            &["rev-parse", "--show-toplevel"],
            cancel,
            deadline,
            8193,
        )?,
        8192,
        "root",
    )?;
    if Path::new(&toplevel) != root {
        return Err("Repository root changed.".into());
    }
    if top_stash_oid(root, cancel, deadline)?.is_none() {
        return Ok(Vec::new());
    }
    let raw = run(
        root,
        &[
            "log",
            "-g",
            "--no-walk=unsorted",
            "-n",
            "65",
            "--format=%H%x00%gd%x00%gs%x00",
            "refs/stash",
            "--",
        ],
        cancel,
        deadline,
        128 * 1024,
    )?;
    let text = std::str::from_utf8(&raw).map_err(|_| "Stash reflog is not UTF-8.")?;
    let mut entries = Vec::new();
    for (expected_idx, row) in text.split_inclusive("\0\n").enumerate() {
        if entries.len() >= MAX_STASHES {
            return Err("Stash list exceeds 64 entries; drop old stashes first.".into());
        }
        let mut fields = row
            .strip_suffix("\0\n")
            .ok_or("Invalid stash reflog framing.")?
            .split('\0');
        let oid = fields.next().ok_or("Missing stash OID.")?;
        let selector = fields.next().ok_or("Missing stash selector.")?;
        let summary_raw = fields.next().ok_or("Missing stash summary.")?;
        if fields.next().is_some() || !history::oid(oid) {
            return Err("Invalid stash reflog entry.".into());
        }
        let expected_selector = format!("stash@{{{expected_idx}}}");
        if selector != expected_selector {
            return Err("Non-contiguous stash reflog selector.".into());
        }
        let summary: String = summary_raw
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .take(512)
            .collect();
        entries.push(Entry {
            index: expected_idx,
            oid: oid.to_owned(),
            selector: selector.to_owned(),
            summary,
        });
    }
    Ok(entries)
}

pub(crate) fn inspect_push(
    root: &Path,
    message: &str,
    cancel: &AtomicBool,
) -> Result<PushReview, String> {
    if !root.is_absolute() || fs::canonicalize(root).map_err(|_| "Root unavailable.")? != root {
        return Err("Exact canonical repository root required.".into());
    }
    if !valid_message(message) {
        return Err("Stash message must be at most 256 bytes without control characters.".into());
    }
    let message = message.trim().to_owned();
    let deadline = Instant::now() + DEADLINE;
    read_only::checkout_filters_disabled(root, cancel, deadline)?;
    let meta = run(
        root,
        &[
            "rev-parse",
            "--show-toplevel",
            "--path-format=absolute",
            "--git-path",
            "MERGE_HEAD",
            "--git-path",
            "CHERRY_PICK_HEAD",
            "--git-path",
            "REVERT_HEAD",
            "--git-path",
            "rebase-merge",
            "--git-path",
            "rebase-apply",
            "--git-path",
            "index",
            "--shared-index-path",
        ],
        cancel,
        deadline,
        16384,
    )?;
    let meta_str = std::str::from_utf8(&meta).map_err(|_| "Invalid Git metadata.")?;
    let mut lines = meta_str.lines();
    let toplevel = lines.next().ok_or("Repository root unavailable.")?;
    if Path::new(toplevel) != root {
        return Err("Repository root changed.".into());
    }
    for _ in 0..5 {
        let marker = lines.next().ok_or("Missing operation path.")?;
        if Path::new(marker).exists() {
            return Err("Finish or abort the active Git operation before stashing.".into());
        }
    }
    let index_path = PathBuf::from(lines.next().ok_or("Missing index path.")?);
    if !index_path.is_absolute() {
        return Err("Invalid index path.".into());
    }
    if lines.next().is_some_and(|s| !s.is_empty()) || lines.next().is_some() {
        return Err("Split index is unsupported for stashing.".into());
    }
    if index_path.with_file_name("index.lock").exists() {
        return Err("Git index is locked; wait before stashing.".into());
    }
    let index = bytes(&index_path, MAX_INDEX)?;
    if !index.starts_with(b"DIRC") {
        return Err("Invalid Git index header.".into());
    }
    let (branch_raw, head) = commit::branch_head(root, cancel, deadline)?;
    if head.bytes().all(|b| b == b'0') {
        return Err("Create an initial commit before stashing.".into());
    }
    let branch = if branch_raw.is_empty() {
        "detached HEAD".into()
    } else {
        branch_raw
    };
    let tree = run(
        root,
        &["ls-tree", "-r", "-z", &head, "--"],
        cancel,
        deadline,
        128 * 1024,
    )?;
    if tree
        .split(|b| *b == 0)
        .any(|entry| entry.starts_with(b"160000 "))
    {
        return Err("Repositories with submodule entries refuse stashing.".into());
    }
    let status_bytes = run(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=no",
            "--ignore-submodules=none",
        ],
        cancel,
        deadline,
        MAX_STATUS_BYTES,
    )?;
    if status_bytes.is_empty() {
        return Err(
            "No tracked modifications to stash (untracked files are preserved in place).".into(),
        );
    }
    if status_bytes.last() != Some(&0) {
        return Err("Invalid Git status framing.".into());
    }
    let mut files = Vec::new();
    for record in status_bytes[..status_bytes.len() - 1].split(|b| *b == 0) {
        if record.len() < 4 || record[2] != b' ' {
            return Err("Invalid status record for stash.".into());
        }
        let code = &record[..2];
        if code.iter().any(|b| !b" MAD".contains(b)) {
            return Err(
                "Stash push supports ordinary tracked modifications/additions/deletions only; resolve renames, conflicts or type changes first."
                    .into(),
            );
        }
        let path = std::str::from_utf8(&record[3..]).map_err(|_| "Non-UTF-8 path.")?;
        if !valid_diff_path(path) {
            return Err("Invalid path in working tree status.".into());
        }
        if files.len() >= MAX_FILES {
            return Err("Tracked changes exceed 256 files; stash refused.".into());
        }
        files.push(path.to_owned());
    }
    let mut file_digests = Vec::with_capacity(files.len());
    for path in &files {
        let full = root.join(path);
        let digest = match fs::symlink_metadata(&full) {
            Ok(_) => Some(Sha256::digest(bytes(&full, 8 * 1024 * 1024)?).into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err("Cannot inspect working file for stash.".into()),
        };
        file_digests.push((path.clone(), digest));
    }
    let top_oid = top_stash_oid(root, cancel, deadline)?;
    if bytes(&index_path, MAX_INDEX)? != index {
        return Err("Index changed during stash inspection; review again.".into());
    }
    Ok(PushReview {
        root: root.into(),
        branch,
        head,
        message,
        files,
        top_oid,
        index_path,
        index,
        status_bytes,
        file_digests,
    })
}

pub(crate) fn apply_push(review: PushReview, cancel: &AtomicBool) -> Result<String, String> {
    let fresh = inspect_push(&review.root, &review.message, cancel)?;
    if fresh != review {
        return Err(
            "Working tree, index, HEAD or stash stack changed since review; inspect again before stashing."
                .into(),
        );
    }
    let deadline = Instant::now() + DEADLINE;
    let mut cmd = read_only::command(&review.root);
    cmd.env("GIT_NO_REPLACE_OBJECTS", "1").args([
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "submodule.recurse=false",
        "-c",
        "user.name=Pui Stash",
        "-c",
        "user.email=stash@pui.local",
        "-c",
        "commit.gpgSign=false",
        "stash",
        "push",
        "--quiet",
        "--no-keep-index",
    ]);
    if !review.message.is_empty() {
        cmd.args(["-m", &review.message]);
    }
    let (_, status) = read_only::run(
        &mut cmd,
        cancel,
        deadline,
        16384,
        "Stash push",
        "Stash push output exceeds limit.",
    )?;
    if !status.success() {
        return Err(
            "Stash push failed or completion is uncertain; inspect status and stash list.".into(),
        );
    }
    let new_top = top_stash_oid(&review.root, cancel, deadline)?
        .ok_or("Stash push completed without a readable refs/stash commit.")?;
    if review.top_oid.as_deref() == Some(new_top.as_str()) {
        return Err("Stash ref did not advance; inspect working tree and stash list.".into());
    }
    Ok(new_top)
}

pub(crate) fn inspect_entry(
    root: &Path,
    index: usize,
    expected_oid: &str,
    action: EntryAction,
    cancel: &AtomicBool,
) -> Result<EntryReview, String> {
    if !history::oid(expected_oid) {
        return Err("Invalid stash commit ID.".into());
    }
    let entries = list(root, cancel)?;
    let top_oid = entries
        .first()
        .map(|e| e.oid.clone())
        .ok_or("No stashes available.")?;
    let entry = entries
        .get(index)
        .filter(|e| e.index == index && e.oid == expected_oid)
        .ok_or("Stash list changed or entry moved; refresh stashes and review again.")?;
    let deadline = Instant::now() + DEADLINE;
    read_only::checkout_filters_disabled(root, cancel, deadline)?;
    if root.join(".git/index.lock").exists() {
        return Err("Git index is locked.".into());
    }
    if action == EntryAction::Apply {
        // Require a clean worktree/index and no active operation so stash apply cannot clobber local work.
        let meta = run(
            root,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "MERGE_HEAD",
                "--git-path",
                "CHERRY_PICK_HEAD",
                "--git-path",
                "REVERT_HEAD",
                "--git-path",
                "rebase-merge",
                "--git-path",
                "rebase-apply",
                "--git-path",
                "index",
            ],
            cancel,
            deadline,
            16384,
        )?;
        let meta_str = std::str::from_utf8(&meta).map_err(|_| "Invalid Git metadata.")?;
        let paths: Vec<_> = meta_str.lines().collect();
        if paths.len() != 6 || paths[..5].iter().any(|p| Path::new(p).exists()) {
            return Err("Finish or abort the active Git operation before applying a stash.".into());
        }
        if Path::new(paths[5]).with_file_name("index.lock").exists() {
            return Err("Git index is locked.".into());
        }
        let (out, status) = read_only::run(
            read_only::command(root).args([
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--ignored",
                "--ignore-submodules=none",
            ]),
            cancel,
            deadline,
            MAX_STATUS_BYTES,
            "Stash apply cleanliness",
            "Status exceeds limit.",
        )?;
        if !status.success() || !out.is_empty() {
            return Err(
                "Applying a stash requires a clean working tree and index (no uncommitted, untracked, or ignored files)."
                    .into(),
            );
        }
    }
    let names_raw = run(
        root,
        &[
            "diff-tree",
            "--no-commit-id",
            "-m",
            "--first-parent",
            "-r",
            "--name-only",
            "-z",
            expected_oid,
            "--",
        ],
        cancel,
        deadline,
        64 * 1024,
    )?;
    let mut files = Vec::new();
    if !names_raw.is_empty() {
        for raw_path in names_raw
            .strip_suffix(&[0])
            .unwrap_or(&names_raw)
            .split(|b| *b == 0)
        {
            if raw_path.is_empty() {
                continue;
            }
            let path = std::str::from_utf8(raw_path).map_err(|_| "Non-UTF-8 stash path.")?;
            if !valid_diff_path(path) || files.len() >= MAX_FILES {
                return Err("Stash entry contains unsupported or excessive file paths.".into());
            }
            if !files.iter().any(|p| p == path) {
                files.push(path.to_owned());
            }
        }
    }
    let patch_raw = run(
        root,
        &[
            "show",
            "-m",
            "--first-parent",
            "--format=",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            expected_oid,
            "--",
        ],
        cancel,
        deadline,
        MAX_PATCH_BYTES,
    )?;
    let preview =
        String::from_utf8(patch_raw).unwrap_or_else(|_| "(binary or non-UTF-8 stash diff)".into());
    Ok(EntryReview {
        root: root.into(),
        index,
        selector: entry.selector.clone(),
        expected_oid: expected_oid.to_owned(),
        top_oid,
        action,
        summary: entry.summary.clone(),
        files,
        preview,
    })
}

pub(crate) fn apply_entry(review: EntryReview, cancel: &AtomicBool) -> Result<(), String> {
    let fresh = inspect_entry(
        &review.root,
        review.index,
        &review.expected_oid,
        review.action,
        cancel,
    )?;
    if fresh != review {
        return Err(
            "Stash stack or working tree changed since review; refresh and review again.".into(),
        );
    }
    let deadline = Instant::now() + DEADLINE;
    match review.action {
        EntryAction::Apply => {
            // Pin by exact immutable commit OID so a concurrent reflog shift cannot apply the wrong entry.
            let (_, status) = read_only::run(
                read_only::command(&review.root)
                    .env("GIT_NO_REPLACE_OBJECTS", "1")
                    .args([
                        "-c",
                        "core.hooksPath=/dev/null",
                        "-c",
                        "submodule.recurse=false",
                        "stash",
                        "apply",
                        "--quiet",
                        &review.expected_oid,
                    ]),
                cancel,
                deadline,
                16384,
                "Stash apply",
                "Stash apply output exceeds limit.",
            )?;
            if !status.success() {
                return Err(
                    "Stash apply failed or conflicted with current branch. Inspect status before retrying."
                        .into(),
                );
            }
            Ok(())
        }
        EntryAction::Drop => {
            let (_, status) = read_only::run(
                read_only::command(&review.root)
                    .env("GIT_NO_REPLACE_OBJECTS", "1")
                    .args([
                        "-c",
                        "core.hooksPath=/dev/null",
                        "stash",
                        "drop",
                        "--quiet",
                        &review.selector,
                    ]),
                cancel,
                deadline,
                16384,
                "Stash drop",
                "Stash drop output exceeds limit.",
            )?;
            if !status.success() {
                return Err("Stash drop failed; refresh stash list to inspect.".into());
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
