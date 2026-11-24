//! Explicit reviewed abort of an in-progress Git operation (merge, rebase, cherry-pick, revert).
//! Never guesses conflict resolution, commits, or executes hooks.
use super::*;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, sync::atomic::AtomicBool};

const MAX_INDEX: usize = 16 * 1024 * 1024;
const MAX_UNMERGED: usize = 256;

type OperationState = Vec<(String, Option<Vec<u8>>)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}
impl Kind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Rebase => "rebase",
            Self::CherryPick => "cherry-pick",
            Self::Revert => "revert",
        }
    }
    pub(crate) fn abort_title(self) -> &'static str {
        match self {
            Self::Merge => "Abort merge?",
            Self::Rebase => "Abort rebase?",
            Self::CherryPick => "Abort cherry-pick?",
            Self::Revert => "Abort revert?",
        }
    }
    fn abort_args(self) -> [&'static str; 2] {
        match self {
            Self::Merge => ["merge", "--abort"],
            Self::Rebase => ["rebase", "--abort"],
            Self::CherryPick => ["cherry-pick", "--abort"],
            Self::Revert => ["revert", "--abort"],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Review {
    pub root: PathBuf,
    pub kind: Kind,
    pub branch: String,
    pub head: String,
    pub target: Option<String>,
    pub unmerged_files: Vec<String>,
    index_path: PathBuf,
    index: Vec<u8>,
    operation: OperationState,
    status_bytes: Vec<u8>,
    unmerged_digests: Vec<(String, Option<[u8; 32]>)>,
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
        "Git operation inspection",
        "Git operation response exceeds limit.",
    )?;
    if !status.success() {
        return Err("Git operation state unavailable.".into());
    }
    Ok(out)
}

fn bytes(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "Operation file unavailable.")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > max as u64 {
        return Err("Operation metadata requires bounded regular files, not symlinks.".into());
    }
    let mut out = Vec::new();
    fs::File::open(path)
        .map_err(|_| "Operation file unavailable.")?
        .take(max as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|_| "Operation file read failed.")?;
    if out.len() > max {
        return Err("Operation file exceeds limit.".into());
    }
    Ok(out)
}

fn sanitize_single_line(raw: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(raw).ok()?.trim();
    (!text.is_empty() && text.len() <= 1024 && !text.chars().any(char::is_control))
        .then(|| text.to_owned())
}

fn freeze_operation_files(
    git_dir: &Path,
) -> Result<(Kind, Option<String>, OperationState), String> {
    for subdir in ["rebase-merge", "rebase-apply", "sequencer"] {
        let dir = git_dir.join(subdir);
        if let Ok(meta) = fs::symlink_metadata(&dir)
            && (!meta.is_dir() || meta.file_type().is_symlink())
        {
            return Err("Symlinked or invalid Git operation directory refused.".into());
        }
    }
    let merge = git_dir.join("MERGE_HEAD").exists();
    let rebase_merge = git_dir.join("rebase-merge").exists();
    let rebase_apply = git_dir.join("rebase-apply").exists();
    let rebase = rebase_merge || rebase_apply;
    let cherry = git_dir.join("CHERRY_PICK_HEAD").exists();
    let revert = git_dir.join("REVERT_HEAD").exists();
    let active_count =
        usize::from(merge) + usize::from(rebase) + usize::from(cherry) + usize::from(revert);
    if active_count == 0 {
        return Err("No active merge, rebase, cherry-pick or revert operation to abort.".into());
    }
    if active_count > 1 || (rebase_merge && rebase_apply) {
        return Err("Multiple concurrent Git operation states detected; abort refused.".into());
    }
    let kind = if merge {
        Kind::Merge
    } else if rebase {
        Kind::Rebase
    } else if cherry {
        Kind::CherryPick
    } else {
        Kind::Revert
    };

    let mut files = Vec::new();
    for name in [
        "MERGE_HEAD",
        "MERGE_MSG",
        "MERGE_MODE",
        "AUTO_MERGE",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "REBASE_HEAD",
        "ORIG_HEAD",
        "rebase-merge/head-name",
        "rebase-merge/onto",
        "rebase-merge/orig-head",
        "rebase-merge/msgnum",
        "rebase-merge/end",
        "rebase-merge/stopped-sha",
        "rebase-apply/head-name",
        "rebase-apply/onto",
        "rebase-apply/orig-head",
        "rebase-apply/next",
        "rebase-apply/last",
        "rebase-apply/original-commit",
        "sequencer/head",
        "sequencer/todo",
        "sequencer/abort-safety",
    ] {
        let path = git_dir.join(name);
        let data = match fs::symlink_metadata(&path) {
            Ok(_) => Some(bytes(&path, 16 * 1024)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err("Operation state unavailable.".into()),
        };
        files.push((name.into(), data));
    }
    let lookup = |key: &str| -> Option<String> {
        files
            .iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.as_deref())
            .and_then(sanitize_single_line)
    };
    let target = match kind {
        Kind::Merge => lookup("MERGE_HEAD"),
        Kind::CherryPick => lookup("CHERRY_PICK_HEAD"),
        Kind::Revert => lookup("REVERT_HEAD"),
        Kind::Rebase => {
            let head_name =
                lookup("rebase-merge/head-name").or_else(|| lookup("rebase-apply/head-name"));
            let onto = lookup("rebase-merge/onto").or_else(|| lookup("rebase-apply/onto"));
            match (head_name, onto) {
                (Some(h), Some(o)) => Some(format!("{h} onto {o}")),
                (Some(h), None) => Some(h),
                (None, Some(o)) => Some(o),
                (None, None) => lookup("REBASE_HEAD"),
            }
        }
    };
    Ok((kind, target, files))
}

pub(crate) fn active_summary(root: &Path, cancel: &AtomicBool) -> Result<Option<Kind>, String> {
    let deadline = Instant::now() + DEADLINE;
    let out = run(
        root,
        &["rev-parse", "--absolute-git-dir"],
        cancel,
        deadline,
        8192,
    )?;
    let git_dir = PathBuf::from(line(&out, 8192, "git dir")?);
    let merge = git_dir.join("MERGE_HEAD").exists();
    let rebase_merge = git_dir.join("rebase-merge").exists();
    let rebase_apply = git_dir.join("rebase-apply").exists();
    let rebase = rebase_merge || rebase_apply;
    let cherry = git_dir.join("CHERRY_PICK_HEAD").exists();
    let revert = git_dir.join("REVERT_HEAD").exists();
    let count =
        usize::from(merge) + usize::from(rebase) + usize::from(cherry) + usize::from(revert);
    if count > 1 || (rebase_merge && rebase_apply) {
        return Err("Multiple concurrent Git operation states detected; abort refused.".into());
    }
    Ok(if merge {
        Some(Kind::Merge)
    } else if rebase {
        Some(Kind::Rebase)
    } else if cherry {
        Some(Kind::CherryPick)
    } else if revert {
        Some(Kind::Revert)
    } else {
        None
    })
}

pub(crate) fn inspect(root: &Path, cancel: &AtomicBool) -> Result<Review, String> {
    if !root.is_absolute() || fs::canonicalize(root).map_err(|_| "Root unavailable.")? != root {
        return Err("Exact canonical repository root required.".into());
    }
    let deadline = Instant::now() + DEADLINE;
    read_only::checkout_filters_disabled(root, cancel, deadline)?;
    let meta = run(
        root,
        &[
            "rev-parse",
            "--show-toplevel",
            "--absolute-git-dir",
            "--path-format=absolute",
            "--git-path",
            "index",
            "--shared-index-path",
        ],
        cancel,
        deadline,
        16384,
    )?;
    let meta_str = std::str::from_utf8(&meta).map_err(|_| "Invalid repository metadata.")?;
    let mut lines = meta_str.lines();
    let toplevel = lines.next().ok_or("Repository root unavailable.")?;
    if Path::new(toplevel) != root {
        return Err("Repository root changed.".into());
    }
    let git_dir = PathBuf::from(lines.next().ok_or("Git directory unavailable.")?);
    let index_path = PathBuf::from(lines.next().ok_or("Index path unavailable.")?);
    if !git_dir.is_absolute() || !index_path.is_absolute() {
        return Err("Invalid Git directory or index path.".into());
    }
    if lines.next().is_some_and(|s| !s.is_empty()) || lines.next().is_some() {
        return Err("Split index is unsupported for operation abort.".into());
    }
    if index_path.with_file_name("index.lock").exists() {
        return Err("Git index is locked; wait or remove stale lock before aborting.".into());
    }
    let index = bytes(&index_path, MAX_INDEX)?;
    if !index.starts_with(b"DIRC") {
        return Err("Invalid Git index header.".into());
    }
    let (kind, target, operation) = freeze_operation_files(&git_dir)?;
    let (branch_raw, head) = commit::branch_head(root, cancel, deadline)?;
    if head.bytes().all(|b| b == b'0') {
        return Err("Operation abort requires an existing HEAD commit.".into());
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
        return Err("Repositories with submodule entries refuse automatic operation abort.".into());
    }
    let unmerged_raw = run(
        root,
        &["ls-files", "--unmerged", "-z"],
        cancel,
        deadline,
        128 * 1024,
    )?;
    let mut unmerged_files = Vec::new();
    if !unmerged_raw.is_empty() {
        if unmerged_raw.last() != Some(&0) {
            return Err("Invalid unmerged index framing.".into());
        }
        for entry in unmerged_raw[..unmerged_raw.len() - 1].split(|b| *b == 0) {
            let tab = entry
                .iter()
                .position(|b| *b == b'\t')
                .ok_or("Invalid unmerged entry.")?;
            let path =
                std::str::from_utf8(&entry[tab + 1..]).map_err(|_| "Non-UTF-8 unmerged path.")?;
            if !valid_diff_path(path) {
                return Err("Invalid unmerged path.".into());
            }
            if !unmerged_files.iter().any(|p| p == path) {
                if unmerged_files.len() >= MAX_UNMERGED {
                    return Err("Unmerged paths exceed 256 files; abort refused.".into());
                }
                unmerged_files.push(path.to_owned());
            }
        }
    }
    let status_bytes = run(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=normal",
            "--ignore-submodules=dirty",
        ],
        cancel,
        deadline,
        MAX_STATUS_BYTES,
    )?;
    let mut unmerged_digests = Vec::with_capacity(unmerged_files.len());
    for path in &unmerged_files {
        let full = root.join(path);
        let digest = match fs::symlink_metadata(&full) {
            Ok(_) => Some(Sha256::digest(bytes(&full, 8 * 1024 * 1024)?).into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err("Cannot inspect unmerged working file.".into()),
        };
        unmerged_digests.push((path.clone(), digest));
    }
    if bytes(&index_path, MAX_INDEX)? != index {
        return Err("Index changed during operation inspection; review again.".into());
    }
    Ok(Review {
        root: root.into(),
        kind,
        branch,
        head,
        target,
        unmerged_files,
        index_path,
        index,
        operation,
        status_bytes,
        unmerged_digests,
    })
}

pub(crate) fn abort(review: Review, cancel: &AtomicBool) -> Result<(), String> {
    let fresh = inspect(&review.root, cancel)?;
    if fresh != review {
        return Err(
            "Repository state, index, working tree or operation changed since review; inspect again before aborting."
                .into(),
        );
    }
    let deadline = Instant::now() + DEADLINE;
    let (_, status) = read_only::run(
        read_only::command(&review.root)
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "submodule.recurse=false",
            ])
            .args(review.kind.abort_args()),
        cancel,
        deadline,
        16384,
        "Abort Git operation",
        "Abort output exceeds limit; inspect repository state.",
    )?;
    if !status.success() {
        return Err(
            "Git abort failed or completion is uncertain. Inspect repository state and conflicts before retrying."
                .into(),
        );
    }
    if active_summary(&review.root, cancel)?.is_some() {
        return Err(
            "Git operation markers remain after abort; inspect repository state before continuing."
                .into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
