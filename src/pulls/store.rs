//! Private per-PR records survive chat changes and app restarts.
use super::CachedStatus;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

const MAX_RECORD: u64 = 64 * 1024;

fn path(home: &Path, url: &str) -> PathBuf {
    home.join(format!(
        "{}.json",
        crate::hex::lower(Sha256::digest(url.as_bytes()))
    ))
}

pub(super) fn load(home: &Path, url: &str) -> Option<CachedStatus> {
    if home.symlink_metadata().ok()?.file_type().is_symlink() {
        return None;
    }
    let path = path(home, url);
    let meta = path.symlink_metadata().ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_RECORD {
        return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

pub(super) fn save(home: &Path, url: &str, entry: &CachedStatus) -> io::Result<CachedStatus> {
    use fs2::FileExt;
    if home
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err(io::Error::other("PR cache cannot be a symlink"));
    }
    fs::create_dir_all(home)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(home, fs::Permissions::from_mode(0o700))?;
    }
    let path = path(home, url);
    let lock_path = path.with_extension("lock");
    if lock_path
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err(io::Error::other("Invalid PR cache lock"));
    }
    let mut options = fs::OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(lock_path)?;
    lock.lock_exclusive()?;
    // A stale result from another window must never overwrite a merged PR.
    if let Some(previous) = load(home, url)
        && (previous.status.state == "Merged"
            || (entry.status.state != "Merged" && previous.checked_at > entry.checked_at))
    {
        return Ok(previous);
    }
    let bytes = serde_json::to_vec(entry)?;
    if bytes.len() as u64 > MAX_RECORD {
        return Err(io::Error::other("PR cache record too large"));
    }
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(temp, path)?;
    Ok(entry.clone())
}
