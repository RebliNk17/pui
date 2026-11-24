//! Frozen recovery restoration, never blind overwrite from a mutable pathname.
use super::*;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Review {
    pub root: PathBuf,
    pub path: String,
    pub recovery: PathBuf,
    index_path: PathBuf,
    index: Vec<u8>,
    current: Vec<u8>,
    saved: Vec<u8>,
    mode: u32,
}
fn recovery_name(path: &str, content: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(path.as_bytes());
    digest.update(Sha256::digest(content));
    format!("pui-discard-recovery-{:x}", digest.finalize())
}
fn mode(path: &Path) -> Result<u32, String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "File unavailable")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(meta.permissions().mode())
    }
    #[cfg(not(unix))]
    {
        Ok(u32::from(meta.permissions().readonly()))
    }
}
pub(crate) fn inspect(
    root: &Path,
    path: &str,
    recovery: &Path,
    cancel: &AtomicBool,
) -> Result<Review, String> {
    if cancel.load(Ordering::Acquire)
        || !root.is_absolute()
        || !valid_diff_path(path)
        || fs::canonicalize(root).map_err(|_| "Root unavailable")? != root
    {
        return Err("Invalid or cancelled recovery review.".into());
    }
    let full = root.join(path);
    let parent = full.parent().ok_or("Target parent unavailable")?;
    if fs::canonicalize(parent).map_err(|_| "Target parent unavailable")? != parent {
        return Err("Symlinked target parents refuse recovery restore.".into());
    }
    let deadline = Instant::now() + DEADLINE;
    let paths = String::from_utf8(run(
        root,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-path",
            "index",
        ],
        cancel,
        deadline,
    )?)
    .map_err(|_| "Invalid Git paths.")?;
    let mut path_lines = paths.lines();
    let actual = path_lines.next().ok_or("Missing repository root.")?;
    let index_raw = path_lines.next().ok_or("Missing index path.")?;
    if path_lines.next().is_some()
        || actual.is_empty()
        || actual.len() > 8192
        || actual.chars().any(char::is_control)
        || index_raw.is_empty()
        || index_raw.len() > 8192
        || index_raw.chars().any(char::is_control)
    {
        return Err("Invalid Git path inventory.".into());
    }
    if Path::new(actual) != root {
        return Err("Repository root changed.".into());
    }
    let index_path = PathBuf::from(index_raw);
    if recovery.parent() != index_path.parent() {
        return Err("Recovery is outside this repository's index directory.".into());
    }
    let saved = bytes(recovery, MAX_FILE)?;
    if recovery.file_name() != Some(std::ffi::OsStr::new(&recovery_name(path, &saved))) {
        return Err("Recovery copy does not match this path and content.".into());
    }
    Ok(Review {
        root: root.into(),
        path: path.into(),
        recovery: recovery.into(),
        index: bytes(&index_path, MAX_INDEX)?,
        index_path,
        current: bytes(&full, MAX_FILE)?,
        saved,
        mode: mode(&full)?,
    })
}
pub(crate) fn apply(review: Review, cancel: &AtomicBool) -> Result<(), String> {
    let validate = || -> Result<(), String> {
        if inspect(&review.root, &review.path, &review.recovery, cancel)? != review {
            return Err("Working file, index or recovery changed; review again.".into());
        }
        Ok(())
    };
    let lock_path = review.index_path.with_file_name("index.lock");
    let _lock = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|_| "Git index locked; nothing restored")?;
    let _guard = OwnedPath(lock_path);
    validate()?;
    // Preserve the reviewed overwritten bytes too. Existing identical backup is reusable, never overwritten.
    let backup = review
        .index_path
        .with_file_name(recovery_name(&review.path, &review.current));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(&backup) {
        Ok(mut f) => {
            f.write_all(&review.current)
                .and_then(|_| f.sync_all())
                .map_err(|_| "Cannot retain current file; nothing restored")?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if bytes(&backup, MAX_FILE)? != review.current {
                return Err("Current-file backup collision; nothing restored.".into());
            }
        }
        Err(_) => return Err("Cannot preserve current file; nothing restored.".into()),
    }
    let full = review.root.join(&review.path);
    let temp = full.with_file_name(format!(".pui-restore-{}", uuid::Uuid::new_v4()));
    let mut file = options
        .open(&temp)
        .map_err(|_| "Cannot prepare restored file")?;
    let _temp = OwnedPath(temp.clone());
    file.write_all(&review.saved)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Cannot sync restored file")?;
    drop(file);
    fs::set_permissions(
        &temp,
        fs::metadata(&full)
            .map_err(|_| "Target unavailable")?
            .permissions(),
    )
    .map_err(|_| "Cannot retain target permissions")?;
    validate()?;
    fs::rename(&temp, &full).map_err(|_| "Cannot replace working file")?;
    Ok(())
}
#[cfg(test)]
mod tests;
