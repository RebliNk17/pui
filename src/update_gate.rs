//! A lifetime shared GUI lock prevents bundle replacement beneath another window.
//! Keep this outside the bundle and never unlink the lock file (inode identity).
use fs2::FileExt;
use std::{
    fs::{File, OpenOptions},
    path::Path,
    time::{Duration, Instant},
};

fn open_at(home: &Path) -> Result<File, String> {
    std::fs::create_dir_all(home).map_err(|_| "Cannot create update coordination directory.")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::symlink_metadata(home)
            .map_err(|_| "Cannot inspect update coordination directory.")?
            .file_type()
            .is_symlink()
        {
            return Err("Update coordination directory cannot be a symlink.".into());
        }
        std::fs::set_permissions(home, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Cannot secure update coordination directory.")?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    options
        .open(home.join("installation.lock"))
        .map_err(|_| "Cannot open update coordination lock.".into())
}

pub(crate) fn open() -> Result<File, String> {
    let home = dirs::data_local_dir()
        .ok_or("No application data directory for update coordination.")?
        .join("pui/update-v1");
    open_at(&home)
}

/// Called before persistence or Chromium is opened, retained through their teardown.
pub(crate) fn enter() -> Result<File, String> {
    let file = open()?;
    let until = Instant::now() + Duration::from_secs(65);
    loop {
        match FileExt::try_lock_shared(&file) {
            Ok(()) => return Ok(file),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < until =>
            {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(_) => return Err("Pui is being updated. Try opening it again shortly.".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_window_prevents_exclusive_install_until_all_guards_drop() {
        let home =
            std::env::temp_dir().join(format!("pui-update-gate-test-{}", uuid::Uuid::new_v4()));
        let first = open_at(&home).unwrap();
        let second = open_at(&home).unwrap();
        let installer = open_at(&home).unwrap();
        FileExt::try_lock_shared(&first).unwrap();
        FileExt::try_lock_shared(&second).unwrap();
        assert!(installer.try_lock_exclusive().is_err());
        drop(first);
        assert!(installer.try_lock_exclusive().is_err());
        drop(second);
        installer.try_lock_exclusive().unwrap();
        let late = open_at(&home).unwrap();
        assert!(FileExt::try_lock_shared(&late).is_err());
        drop(installer);
        FileExt::try_lock_shared(&late).unwrap();
    }
}
