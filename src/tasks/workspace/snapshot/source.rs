//! Descriptor-relative source reads: parent symlink swaps must never expose outside files.
use super::*;

pub(in crate::tasks) enum Entry {
    Missing,
    Link(PathBuf),
    File(fs::File),
}

#[cfg(unix)]
pub(in crate::tasks) fn entry(root: &Path, relative: &Path) -> Result<Entry, String> {
    use std::{
        ffi::{CString, OsStr},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{ffi::OsStrExt, fs::OpenOptionsExt},
        },
    };
    fn name(s: &OsStr) -> Result<CString, String> {
        CString::new(s.as_bytes()).map_err(|_| "Invalid snapshot path.".into())
    }
    fn missing(e: &std::io::Error) -> bool {
        matches!(
            e.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
        )
    }
    let mut dir = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)
        .map_err(|e| e.to_string())?;
    let components: Vec<_> = relative.iter().collect();
    for component in &components[..components.len() - 1] {
        let component = name(component)?;
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                component.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            let e = std::io::Error::last_os_error();
            // A symlinked parent must be refused even where openat reports ENOTDIR.
            let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
            if unsafe {
                libc::fstatat(
                    dir.as_raw_fd(),
                    component.as_ptr(),
                    stat.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } == 0
                && unsafe { stat.assume_init() }.st_mode & libc::S_IFMT == libc::S_IFLNK
            {
                return Err("Symlinked source directory cannot be snapshotted safely.".into());
            }
            if missing(&e) {
                return Ok(Entry::Missing);
            }
            return Err(e.to_string());
        }
        dir = unsafe { fs::File::from_raw_fd(fd) };
    }
    let component = name(components.last().unwrap())?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            component.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } < 0
    {
        let e = std::io::Error::last_os_error();
        return if missing(&e) {
            Ok(Entry::Missing)
        } else {
            Err(e.to_string())
        };
    }
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT == libc::S_IFLNK {
        let mut buffer = vec![0u8; 65536];
        let len = unsafe {
            libc::readlinkat(
                dir.as_raw_fd(),
                component.as_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
        if len < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if len as usize == buffer.len() {
            return Err("Snapshot symlink target is too long.".into());
        }
        buffer.truncate(len as usize);
        return Ok(Entry::Link(PathBuf::from(OsStr::from_bytes(&buffer))));
    }
    if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                component.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err("Source directory changed during snapshotting.".into());
        }
        let child = unsafe { fs::File::from_raw_fd(fd) };
        let marker = name(OsStr::new(".git"))?;
        let mut meta = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                child.as_raw_fd(),
                marker.as_ptr(),
                meta.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0
        {
            return Err("Nested repositories cannot be included in a workflow snapshot.".into());
        }
        return Ok(Entry::Missing);
    }
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err("Special file cannot be included in a workflow snapshot.".into());
    }
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            component.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let file = unsafe { fs::File::from_raw_fd(fd) };
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Source changed during snapshotting.".into());
    }
    Ok(Entry::File(file))
}

#[cfg(not(unix))]
pub(in crate::tasks) fn entry(_root: &Path, _relative: &Path) -> Result<Entry, String> {
    Err("Safe workflow snapshots are not yet supported on this platform.".into())
}
