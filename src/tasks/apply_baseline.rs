//! Durable preparation evidence. This is captured before the builder, never reconstructed from HEAD.
use super::*;
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineReceipt {
    pub version: u32,
    pub task_id: String,
    pub directory: PathBuf,
    pub manifest_hash: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Snapshot {
    pub tree: Tree,
    pub index: Option<FileState>,
    pub fingerprint: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Baseline {
    pub version: u32,
    pub task_id: String,
    pub identity: Identity,
    pub original: Snapshot,
    pub candidate: Snapshot,
}
// JSON byte arrays cost up to four bytes per source byte, plus path/metadata framing.
const MAX_MANIFEST: u64 = 280 * 1024 * 1024;

/// Pin each absolute directory component. Checking a path and then calling
/// create_dir_all/open would allow an ancestor symlink swap to redirect backups.
#[cfg(unix)]
fn directory(path: &Path, create: bool) -> Result<fs::File, String> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{ffi::OsStrExt, fs::OpenOptionsExt},
        },
    };
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    };
    let mut dir = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")
        .map_err(|e| e.to_string())?;
    for component in absolute.components() {
        let Component::Normal(part) = component else {
            if component == Component::RootDir {
                continue;
            }
            return Err("Unsafe evidence directory".into());
        };
        let part = CString::new(part.as_bytes()).map_err(|_| "Invalid evidence directory")?;
        if create && unsafe { libc::mkdirat(dir.as_raw_fd(), part.as_ptr(), 0o700) } < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(error.to_string());
            }
        }
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                part.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err("Evidence directory contains a symlink or inaccessible component.".into());
        }
        if create {
            dir.sync_all().map_err(|e| e.to_string())?;
        }
        dir = unsafe { fs::File::from_raw_fd(fd) };
    }
    Ok(dir)
}
pub(super) fn open_directory(path: &Path) -> Result<fs::File, String> {
    directory(path, false)
}
pub(super) fn private_dir(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let dir = directory(path, true)?;
    dir.set_permissions(fs::Permissions::from_mode(0o700))
        .and_then(|_| dir.sync_all())
        .map_err(|e| e.to_string())
}
pub(super) fn durable_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
    };
    let parent = open_directory(path.parent().ok_or("Missing evidence parent")?)?;
    let leaf = CString::new(
        path.file_name()
            .ok_or("Missing evidence filename")?
            .as_bytes(),
    )
    .map_err(|_| "Invalid evidence filename")?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            leaf.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut file = unsafe { fs::File::from_raw_fd(fd) };
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    parent.sync_all().map_err(|e| e.to_string())
}
pub(super) fn sync_dir(path: &Path) -> Result<(), String> {
    open_directory(path)?.sync_all().map_err(|e| e.to_string())
}
pub(super) fn load_bytes(directory: &Path, path: &str) -> Result<Vec<u8>, String> {
    match source::entry(directory, Path::new(path))? {
        source::Entry::File(mut file) => {
            if file.metadata().map_err(|e| e.to_string())?.len() > MAX_MANIFEST {
                return Err("Apply evidence exceeds its read budget.".into());
            }
            let mut bytes = Vec::new();
            (&mut file)
                .take(MAX_MANIFEST + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() as u64 > MAX_MANIFEST {
                return Err("Apply evidence exceeds its read budget.".into());
            }
            Ok(bytes)
        }
        _ => Err("Apply evidence is missing or is not a regular file.".into()),
    }
}
pub(super) fn snapshot(
    root: &Path,
    git_dir: &Path,
    paths: &BTreeSet<String>,
) -> Result<Snapshot, String> {
    let (tree, fingerprint) = capture(root, git_dir, paths)?;
    let index = read(git_dir, "index", &mut 0)?;
    Ok(Snapshot {
        tree,
        index,
        fingerprint,
    })
}
/// Capture immediately after `workspace::prepare`, save the returned receipt in Task,
/// and acknowledge that save before starting any builder. `home` is the task store.
/// Legacy workflows without this receipt are deliberately not applicable.
pub fn capture_receipt(
    source: &Path,
    candidate: &Path,
    base_head: &str,
    task_id: &str,
    home: &Path,
) -> Result<BaselineReceipt, String> {
    uuid::Uuid::parse_str(task_id).map_err(|_| "Invalid workflow identity")?;
    let identity = resolve_identity(source, candidate, base_head)?;
    let paths = all_paths(&identity)?;
    let original = snapshot(&identity.original, &identity.original_git_dir, &paths)?;
    let candidate = snapshot(&identity.candidate, &identity.candidate_git_dir, &paths)?;
    if original != candidate {
        return Err("Original and initial candidate are not the same preparation snapshot. No builder baseline was recorded.".into());
    }
    if resolve_identity(source, candidate_path(&identity), base_head)? != identity
        || all_paths(&identity)? != paths
        || snapshot(&identity.original, &identity.original_git_dir, &paths)? != original
        || snapshot(&identity.candidate, &identity.candidate_git_dir, &paths)? != candidate
    {
        return Err(
            "Source changed while recording preparation evidence. No builder may start.".into(),
        );
    }
    let directory = fs::canonicalize(home)
        .map_err(|e| e.to_string())?
        .join("apply-v1")
        .join(format!("baseline-{task_id}-{}", uuid::Uuid::new_v4()));
    private_dir(&directory)?;
    let baseline = Baseline {
        version: 1,
        task_id: task_id.into(),
        identity,
        original,
        candidate,
    };
    let bytes = serde_json::to_vec(&baseline).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_MANIFEST {
        return Err("Preparation evidence exceeds its storage budget.".into());
    }
    durable_file(&directory.join("baseline.json"), &bytes)?;
    sync_dir(directory.parent().unwrap())?;
    Ok(BaselineReceipt {
        version: 1,
        task_id: task_id.into(),
        directory,
        manifest_hash: digest(&bytes),
    })
}
fn candidate_path(identity: &Identity) -> &Path {
    &identity.candidate_scope
}

pub(super) fn load(task: &Task) -> Result<Baseline, String> {
    let receipt = task.preparation_receipt.as_ref().ok_or("No durable preparation baseline receipt. Older workflows cannot be applied; create a newly prepared workflow.")?;
    if receipt.version != 1 || receipt.task_id != task.id {
        return Err("Preparation receipt identity mismatch.".into());
    }
    let bytes = load_bytes(&receipt.directory, "baseline.json")?;
    if digest(&bytes) != receipt.manifest_hash {
        return Err("Preparation baseline receipt changed.".into());
    }
    let baseline: Baseline =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid preparation evidence")?;
    if baseline.version != 1
        || baseline.task_id != task.id
        || baseline.original != baseline.candidate
    {
        return Err("Preparation snapshot identity mismatch.".into());
    }
    Ok(baseline)
}
