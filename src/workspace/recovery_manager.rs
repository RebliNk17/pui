//! Persistent discovery, cryptographic path matching, reviewed restoration and deletion
//! of Pui recovery copies (`pui-discard-recovery-*` and `pui-conflict-recovery-*`).
use super::*;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::{Read, Write},
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_FILE: usize = 8 * 1024 * 1024;
const MAX_TOTAL_SCAN_BYTES: usize = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 128;
const MAX_CANDIDATE_PATHS: usize = 2048;
const MAX_INDEX: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Discard,
    Conflict,
}
impl Kind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Discard => "Discard backup",
            Self::Conflict => "Conflict backup",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub file_name: String,
    pub recovery_path: PathBuf,
    pub kind: Kind,
    pub matched_path: Option<String>,
    pub size_bytes: usize,
    pub preview: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Summary {
    pub entries: Vec<Entry>,
    pub total_bytes: usize,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RestoreReview {
    pub root: PathBuf,
    pub target_path: String,
    pub recovery_path: PathBuf,
    pub kind: Kind,
    pub size_bytes: usize,
    pub preview: String,
    index_path: PathBuf,
    index: Vec<u8>,
    current: Option<Vec<u8>>,
    saved: Vec<u8>,
    mode: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeleteReview {
    pub root: PathBuf,
    pub recovery_path: PathBuf,
    pub file_name: String,
    pub kind: Kind,
    pub matched_path: Option<String>,
    pub size_bytes: usize,
    pub preview: String,
    saved_sha256: [u8; 32],
}

struct OwnedPath(PathBuf);
impl Drop for OwnedPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn discard_recovery_name(path: &str, content: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(path.as_bytes());
    digest.update(Sha256::digest(content));
    format!(
        "pui-discard-recovery-{}",
        crate::hex::lower(digest.finalize())
    )
}

fn conflict_recovery_name(path: &str, content: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"conflict\0");
    digest.update(path.as_bytes());
    digest.update(b"\0");
    digest.update(content);
    format!(
        "pui-conflict-recovery-{}",
        crate::hex::lower(digest.finalize())
    )
}

fn parse_recovery_filename(name: &str) -> Option<(Kind, &str)> {
    let (kind, hex) = if let Some(rest) = name.strip_prefix("pui-discard-recovery-") {
        (Kind::Discard, rest)
    } else if let Some(rest) = name.strip_prefix("pui-conflict-recovery-") {
        (Kind::Conflict, rest)
    } else {
        return None;
    };
    if hex.len() == 64
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Some((kind, hex))
    } else {
        None
    }
}

fn matches_filename(kind: Kind, file_name: &str, path: &str, content: &[u8]) -> bool {
    match kind {
        Kind::Discard => discard_recovery_name(path, content) == file_name,
        Kind::Conflict => conflict_recovery_name(path, content) == file_name,
    }
}

fn format_preview(bytes: &[u8], max_bytes: usize, max_lines: usize) -> String {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return "(binary or non-UTF-8 content)".into();
    };
    let mut out = String::new();
    for (idx, line) in text.lines().enumerate() {
        if idx >= max_lines || out.len() >= max_bytes {
            out.push_str("\n…");
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        for ch in line.chars() {
            if out.len() >= max_bytes {
                break;
            }
            if ch.is_control() && ch != '\t' {
                out.push('\u{FFFD}');
            } else {
                out.push(ch);
            }
        }
    }
    out
}

fn bytes(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "Recovery file unavailable.")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > max as u64 {
        return Err("Recovery action requires bounded regular files, not symlinks.".into());
    }
    let mut out = Vec::new();
    fs::File::open(path)
        .map_err(|_| "Recovery file unavailable.")?
        .take(max as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|_| "Recovery file read failed.")?;
    if out.len() > max {
        return Err("Recovery file exceeds limit.".into());
    }
    Ok(out)
}

fn resolve_index_path(
    root: &Path,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<PathBuf, String> {
    if !root.is_absolute() || fs::canonicalize(root).map_err(|_| "Root unavailable.")? != root {
        return Err("Exact canonical repository root required.".into());
    }
    let (out, status) = read_only::run(
        read_only::command(root).args([
            "rev-parse",
            "--show-toplevel",
            "--path-format=absolute",
            "--git-path",
            "index",
        ]),
        cancel,
        deadline,
        16384,
        "Recovery root inspection",
        "Recovery root output exceeds limit.",
    )?;
    if !status.success() {
        return Err("Git repository unavailable.".into());
    }
    let text = std::str::from_utf8(&out).map_err(|_| "Invalid Git metadata.")?;
    let mut lines = text.lines();
    let toplevel = lines.next().ok_or("Missing repository root.")?;
    if Path::new(toplevel) != root {
        return Err("Repository root changed.".into());
    }
    let index = PathBuf::from(lines.next().ok_or("Missing index path.")?);
    if !index.is_absolute() || lines.next().is_some() {
        return Err("Invalid Git index path.".into());
    }
    Ok(index)
}

fn candidate_paths(root: &Path, cancel: &AtomicBool, deadline: Instant) -> Vec<String> {
    let mut set = BTreeSet::new();
    if let Ok((out, status)) = read_only::run(
        read_only::command(root).args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ]),
        cancel,
        deadline,
        256 * 1024,
        "Candidate paths",
        "Candidate paths exceed limit",
    ) && status.success()
    {
        for raw in out.split(|b| *b == 0) {
            if set.len() >= MAX_CANDIDATE_PATHS {
                break;
            }
            if let Ok(p) = std::str::from_utf8(raw)
                && valid_diff_path(p)
            {
                set.insert(p.to_owned());
            }
        }
    }
    if set.len() < MAX_CANDIDATE_PATHS
        && let Ok((out, status)) = read_only::run(
            read_only::command(root)
                .env("GIT_NO_REPLACE_OBJECTS", "1")
                .args([
                    "log",
                    "-n",
                    "32",
                    "--name-only",
                    "--format=",
                    "-z",
                    "HEAD",
                    "--",
                ]),
            cancel,
            deadline,
            256 * 1024,
            "Historical candidate paths",
            "Historical paths exceed limit",
        )
        && status.success()
    {
        for raw in out.split(|b| *b == 0) {
            if set.len() >= MAX_CANDIDATE_PATHS {
                break;
            }
            if let Ok(p) = std::str::from_utf8(raw)
                && valid_diff_path(p)
            {
                set.insert(p.to_owned());
            }
        }
    }
    set.into_iter().collect()
}

pub(crate) fn list(root: &Path, cancel: &AtomicBool) -> Result<Summary, String> {
    let deadline = Instant::now() + DEADLINE;
    let index_path = resolve_index_path(root, cancel, deadline)?;
    let index_dir = index_path.parent().ok_or("Index directory unavailable.")?;
    let candidates = candidate_paths(root, cancel, deadline);
    let read_dir = fs::read_dir(index_dir).map_err(|_| "Cannot read Git index directory.")?;
    let mut files = Vec::new();
    for item in read_dir {
        if cancel.load(Ordering::Acquire) {
            return Err("Recovery discovery cancelled.".into());
        }
        let Ok(item) = item else { continue };
        let Ok(file_name) = item.file_name().into_string() else {
            continue;
        };
        let Some((kind, _)) = parse_recovery_filename(&file_name) else {
            continue;
        };
        let path = item.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_FILE as u64 {
            continue;
        }
        files.push((file_name, path, kind));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut summary = Summary::default();
    for (file_name, recovery_path, kind) in files {
        if summary.entries.len() >= MAX_ENTRIES {
            summary.truncated = true;
            break;
        }
        let Ok(content) = bytes(&recovery_path, MAX_FILE) else {
            continue;
        };
        if summary.total_bytes.saturating_add(content.len()) > MAX_TOTAL_SCAN_BYTES {
            summary.truncated = true;
            break;
        }
        summary.total_bytes += content.len();
        let matched_path = candidates
            .iter()
            .find(|candidate| matches_filename(kind, &file_name, candidate, &content))
            .cloned();
        let preview = format_preview(&content, 4096, 64);
        summary.entries.push(Entry {
            file_name,
            recovery_path,
            kind,
            matched_path,
            size_bytes: content.len(),
            preview,
        });
    }
    Ok(summary)
}

pub(crate) fn inspect_restore(
    root: &Path,
    recovery_path: &Path,
    target_path: &str,
    cancel: &AtomicBool,
) -> Result<RestoreReview, String> {
    if cancel.load(Ordering::Acquire) || !valid_diff_path(target_path) {
        return Err("Provide a valid relative target file path.".into());
    }
    let deadline = Instant::now() + DEADLINE;
    let index_path = resolve_index_path(root, cancel, deadline)?;
    let index_dir = index_path.parent().ok_or("Index directory unavailable.")?;
    if recovery_path.parent() != Some(index_dir) {
        return Err("Recovery file is outside this repository's index directory.".into());
    }
    let file_name = recovery_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid recovery filename.")?;
    let (kind, _) = parse_recovery_filename(file_name).ok_or("Not a valid Pui recovery file.")?;
    let saved = bytes(recovery_path, MAX_FILE)?;
    if !matches_filename(kind, file_name, target_path, &saved) {
        return Err(
            "Target path and content do not match this recovery file's cryptographic digest."
                .into(),
        );
    }

    // Reject symlink ancestors along target_path.
    let relative = Path::new(target_path);
    let components: Vec<_> = relative.components().collect();
    let mut walk = root.to_path_buf();
    for comp in &components[..components.len().saturating_sub(1)] {
        walk.push(comp);
        let meta =
            fs::symlink_metadata(&walk).map_err(|_| "Target parent directory does not exist.")?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err("Symlinked target parent directories refuse recovery restore.".into());
        }
    }
    let full = root.join(target_path);
    let (current, mode) = match fs::symlink_metadata(&full) {
        Ok(meta) => {
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err("Target path exists and is not an ordinary file.".into());
            }
            #[cfg(unix)]
            let m = {
                use std::os::unix::fs::PermissionsExt;
                Some(meta.permissions().mode())
            };
            #[cfg(not(unix))]
            let m = Some(u32::from(meta.permissions().readonly()));
            (Some(bytes(&full, MAX_FILE)?), m)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (None, None),
        Err(_) => return Err("Cannot inspect target path.".into()),
    };
    let index = match fs::symlink_metadata(&index_path) {
        Ok(_) => bytes(&index_path, MAX_INDEX)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(_) => return Err("Index unavailable.".into()),
    };
    let preview = format_preview(&saved, 64 * 1024, 512);
    Ok(RestoreReview {
        root: root.into(),
        target_path: target_path.to_owned(),
        recovery_path: recovery_path.to_path_buf(),
        kind,
        size_bytes: saved.len(),
        preview,
        index_path,
        index,
        current,
        saved,
        mode,
    })
}

pub(crate) fn apply_restore(review: RestoreReview, cancel: &AtomicBool) -> Result<(), String> {
    let validate = || -> Result<(), String> {
        let fresh = inspect_restore(
            &review.root,
            &review.recovery_path,
            &review.target_path,
            cancel,
        )?;
        if fresh != review {
            return Err("Working file, index or recovery copy changed; review again.".into());
        }
        Ok(())
    };
    validate()?;
    let lock_path = review.index_path.with_file_name("index.lock");
    let _lock = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|_| "Git index is locked; nothing restored.")?;
    let _guard = OwnedPath(lock_path);
    validate()?;

    if let Some(current_bytes) = &review.current {
        let backup = review
            .index_path
            .with_file_name(discard_recovery_name(&review.target_path, current_bytes));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&backup) {
            Ok(mut f) => {
                f.write_all(current_bytes)
                    .and_then(|_| f.sync_all())
                    .map_err(|_| "Cannot retain current file backup; nothing restored.")?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if bytes(&backup, MAX_FILE)? != *current_bytes {
                    return Err("Current-file backup collision; nothing restored.".into());
                }
            }
            Err(_) => return Err("Cannot preserve current file; nothing restored.".into()),
        }
    }

    let full = review.root.join(&review.target_path);
    let temp = full.with_file_name(format!(".pui-restore-{}", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .map_err(|_| "Cannot prepare restored file.")?;
    let _temp = OwnedPath(temp.clone());
    file.write_all(&review.saved)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Cannot sync restored file.")?;
    drop(file);
    if review.mode.is_some()
        && let Ok(meta) = fs::metadata(&full)
    {
        fs::set_permissions(&temp, meta.permissions())
            .map_err(|_| "Cannot retain target permissions.")?;
    }
    validate()?;
    fs::rename(&temp, &full).map_err(|_| "Cannot replace working file.")?;
    Ok(())
}

pub(crate) fn inspect_delete(
    root: &Path,
    recovery_path: &Path,
    cancel: &AtomicBool,
) -> Result<DeleteReview, String> {
    if cancel.load(Ordering::Acquire) {
        return Err("Recovery delete inspection cancelled.".into());
    }
    let deadline = Instant::now() + DEADLINE;
    let index_path = resolve_index_path(root, cancel, deadline)?;
    let index_dir = index_path.parent().ok_or("Index directory unavailable.")?;
    if recovery_path.parent() != Some(index_dir) {
        return Err("Recovery file is outside this repository's index directory.".into());
    }
    let file_name = recovery_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid recovery filename.")?
        .to_owned();
    let (kind, _) = parse_recovery_filename(&file_name).ok_or("Not a valid Pui recovery file.")?;
    let saved = bytes(recovery_path, MAX_FILE)?;
    let candidates = candidate_paths(root, cancel, deadline);
    let matched_path = candidates
        .iter()
        .find(|candidate| matches_filename(kind, &file_name, candidate, &saved))
        .cloned();
    let saved_sha256: [u8; 32] = Sha256::digest(&saved).into();
    let preview = format_preview(&saved, 64 * 1024, 512);
    Ok(DeleteReview {
        root: root.into(),
        recovery_path: recovery_path.to_path_buf(),
        file_name,
        kind,
        matched_path,
        size_bytes: saved.len(),
        preview,
        saved_sha256,
    })
}

pub(crate) fn apply_delete(review: DeleteReview, cancel: &AtomicBool) -> Result<(), String> {
    let fresh = inspect_delete(&review.root, &review.recovery_path, cancel)?;
    if fresh != review {
        return Err(
            "Recovery copy changed or was already removed; refresh and review again.".into(),
        );
    }
    fs::remove_file(&review.recovery_path).map_err(|_| "Cannot delete recovery copy.".into())
}

#[cfg(test)]
mod tests;
