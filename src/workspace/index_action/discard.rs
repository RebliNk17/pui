//! Restore only existing ordinary text content from index, retaining original beside Git metadata.
use super::*;
#[cfg(test)]
mod tests;
pub(super) fn inspect(
    root: &Path,
    path: &str,
    entry: &[u8],
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<u8>, String> {
    let entry = std::str::from_utf8(entry).map_err(|_| "Invalid index entry.")?;
    let oid = entry
        .split_whitespace()
        .nth(1)
        .ok_or("Missing index object.")?;
    if !matches!(oid.len(), 40 | 64) || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid index blob.".into());
    }
    let attrs = run(
        root,
        &[
            "--literal-pathspecs",
            "check-attr",
            "-z",
            "--all",
            "--",
            path,
        ],
        cancel,
        deadline,
    )?;
    if !attrs.is_empty() {
        return Err(
            "Discard is unavailable when file attributes are configured. No conversion is guessed."
                .into(),
        );
    }
    let (_, status) = read_only::run(
        read_only::command(root).args(["config", "--get-regexp", "^core\\.(autocrlf|eol)$"]),
        cancel,
        deadline,
        8192,
        "Conversion configuration",
        "Configuration too large.",
    )?;
    if status.code() != Some(1) {
        return Err("Discard requires absent line-ending conversion configuration.".into());
    }
    let out = run(root, &["cat-file", "blob", oid], cancel, deadline)?;
    let text = std::str::from_utf8(&out).map_err(|_| "Discard requires a UTF-8 index version.")?;
    if out.len() > 128 * 1024
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err("Discard requires bounded text content.".into());
    }
    Ok(out)
}
pub(super) fn apply(
    snapshot: Snapshot,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<(), String> {
    if snapshot.hunk.0.is_some() {
        return Err("Hunk discard is not supported.".into());
    }
    let restore = snapshot
        .restore
        .as_ref()
        .ok_or("Missing inspected index content.")?;
    let full = snapshot.root.join(&snapshot.path);
    let original = bytes(&full, MAX_FILE)?;
    // Respect cooperative Git index writers during final validation and publication.
    let lock_path = snapshot.index_path.with_file_name("index.lock");
    let lock = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|_| "Git index locked; no working file changed.")?;
    let _lock_guard = OwnedPath(lock_path);
    let fresh = prepare_at(
        &snapshot.root,
        &snapshot.path,
        &snapshot.status,
        snapshot.action,
        cancel,
        deadline,
    )?;
    if fresh != snapshot {
        return Err("File, index or HEAD changed before discard. Nothing replaced.".into());
    }
    let recovery = snapshot.recovery_path();
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut backup = options
        .open(&recovery)
        .map_err(|_| "Cannot retain discard recovery copy; nothing replaced.")?;
    backup
        .write_all(&original)
        .and_then(|_| backup.sync_all())
        .map_err(|_| "Cannot sync recovery copy; nothing replaced.")?;
    drop(backup);
    let temp = full.with_file_name(format!(".pui-discard-{}", uuid::Uuid::new_v4()));
    let mut replacement = OpenOptions::new();
    replacement.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        replacement.mode(0o600);
    }
    let mut target = replacement
        .open(&temp)
        .map_err(|_| "Cannot prepare replacement file.")?;
    let _temp_guard = OwnedPath(temp.clone());
    target
        .write_all(restore)
        .and_then(|_| target.sync_all())
        .map_err(|_| "Cannot sync replacement file.")?;
    drop(target);
    fs::set_permissions(
        &temp,
        fs::metadata(&full)
            .map_err(|_| "Cannot read file mode.")?
            .permissions(),
    )
    .map_err(|_| "Cannot retain file mode.")?;
    if cancel.load(Ordering::Acquire)
        || Instant::now() >= deadline
        || bytes(&full, MAX_FILE)? != original
        || bytes(&snapshot.index_path, MAX_INDEX)? != snapshot.index
    {
        return Err(format!(
            "Discard cancelled or input changed. Recovery copy: {}",
            recovery.display()
        ));
    }
    fs::rename(&temp, &full)
        .map_err(|_| format!("Cannot replace file. Recovery copy: {}", recovery.display()))?;
    drop(lock);
    Ok(())
}

/// Restore a retained discard recovery copy back to the working file.
pub(super) fn restore(
    root: &Path,
    path: &str,
    recovery: &Path,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let deadline = Instant::now() + DEADLINE;
    let meta = fs::symlink_metadata(recovery).map_err(|_| "No recovery copy for this file.")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_FILE as u64 {
        return Err("Recovery copy is not a bounded regular file; nothing restored.".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(recovery)
        .map_err(|_| "Cannot read recovery copy.")?
        .take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read recovery copy.")?;
    if bytes.len() > MAX_FILE {
        return Err("Recovery copy exceeds limit; nothing restored.".into());
    }
    let full = root.join(path);
    if !matches!(
        fs::symlink_metadata(&full).map(|m| (m.is_file(), !m.file_type().is_symlink())),
        Ok((true, true))
    ) {
        return Err("Restore target is not a bounded regular file; nothing restored.".into());
    }
    if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Err("Restore cancelled; no file changed.".into());
    }
    // Cooperative index lock so Git writers do not observe a torn working file.
    let lock_path = root.join(".git/index.lock");
    let lock = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|_| "Git index locked; nothing restored.")?;
    let _lock_guard = OwnedPath(lock_path);
    let temp = full.with_file_name(format!(".pui-restore-{}", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut target = options
        .open(&temp)
        .map_err(|_| "Cannot prepare restored file.")?;
    let _temp_guard = OwnedPath(temp.clone());
    target
        .write_all(&bytes)
        .and_then(|_| target.sync_all())
        .map_err(|_| "Cannot write restored file.")?;
    drop(target);
    fs::set_permissions(&temp, meta.permissions()).map_err(|_| "Cannot retain recovery mode.")?;
    if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Err("Restore cancelled; no file changed.".into());
    }
    fs::rename(&temp, &full).map_err(|_| "Cannot replace working file with recovery copy.")?;
    drop(lock);
    Ok(())
}
