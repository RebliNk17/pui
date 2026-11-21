//! Journaled creation of missing ordinary parent directories. Unknown creations
//! remain retained; rollback removes only recorded, unchanged, empty directories.
use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Directory {
    pub path: String,
    pub dev: u64,
    pub ino: u64,
}

pub(in crate::tasks::apply) fn missing(root: &Path, path: &str) -> Result<Vec<String>, String> {
    name(path.as_bytes())?;
    let mut found = Vec::new();
    let mut relative = PathBuf::new();
    let parts = Path::new(path).components().collect::<Vec<_>>();
    for part in &parts[..parts.len() - 1] {
        relative.push(part.as_os_str());
        let path = relative.to_str().ok_or("Unsupported parent path")?;
        match fs::symlink_metadata(root.join(path)) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
            Ok(_) => return Err("Target parent is a link or non-directory.".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => found.push(path.into()),
            Err(_) => return Err("Target parent is inaccessible.".into()),
        }
    }
    Ok(found)
}

pub(super) fn create(
    root: &Path,
    paths: &[String],
    journal: &Path,
) -> Result<Vec<Directory>, String> {
    let mut created = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        let p = parent(root, path)?;
        // Intent precedes mkdir; a lost creation acknowledgment is never guessed.
        marker(journal, &format!("directory-intent-{i}"))?;
        if unsafe { libc::mkdirat(p.directory.as_raw_fd(), p.leaf.as_ptr(), 0o755) } < 0 {
            return Err(
                "Directory creation collided or failed; existing data was preserved.".into(),
            );
        }
        p.directory.sync_all().map_err(|e| e.to_string())?;
        let child = parent(root, &format!("{path}/receipt"))?;
        let meta = child.directory.metadata().map_err(|e| e.to_string())?;
        let directory = Directory {
            path: path.clone(),
            dev: meta.dev(),
            ino: meta.ino(),
        };
        baseline::durable_file(
            &journal.join(format!("directory-created-{i}.json")),
            &serde_json::to_vec(&directory).map_err(|e| e.to_string())?,
        )?;
        created.push(directory);
    }
    Ok(created)
}
pub(super) fn receipts(
    root: &Path,
    paths: &[String],
    journal: &Path,
) -> Result<Vec<Directory>, String> {
    let mut found = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        let bytes = baseline::load_bytes(journal, &format!("directory-created-{i}.json"))?;
        let dir: Directory =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid directory receipt")?;
        name(path.as_bytes())?;
        if dir.path != *path {
            return Err("Directory receipt identity mismatch.".into());
        }
        match fs::symlink_metadata(root.join(path)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err("Cannot inspect directory recovery state.".into()),
            Ok(_) => {}
        }
        let p = parent(root, &format!("{path}/receipt"))?;
        let meta = p.directory.metadata().map_err(|e| e.to_string())?;
        if meta.dev() != dir.dev || meta.ino() != dir.ino {
            return Err("Created directory was replaced; inspect retained files.".into());
        }
        found.push(dir);
    }
    Ok(found)
}
pub(super) fn cleanup(root: &Path, created: &[Directory]) -> Result<(), String> {
    for dir in created.iter().rev() {
        let p = parent(root, &dir.path)?;
        let child = parent(root, &format!("{}/receipt", dir.path))?;
        let meta = child.directory.metadata().map_err(|e| e.to_string())?;
        if meta.dev() != dir.dev || meta.ino() != dir.ino {
            return Err("Created directory changed; it was preserved.".into());
        }
        // Move exclusively before destructive removal; validating a pathname
        // followed by unlinkat would race a different empty directory into it.
        let held = c(&format!(".pui-directory-{}", uuid::Uuid::new_v4()));
        move_exclusive(&p, &p.leaf, &held)?;
        let moved = parent(
            root,
            &format!(
                "{}/receipt",
                Path::new(&dir.path)
                    .parent()
                    .unwrap_or(Path::new(""))
                    .join(std::ffi::OsStr::from_bytes(held.as_bytes()))
                    .display()
            ),
        )?;
        let actual = moved.directory.metadata().map_err(|e| e.to_string())?;
        if actual.dev() != dir.dev || actual.ino() != dir.ino {
            let _ = move_exclusive(&p, &held, &p.leaf);
            return Err("Directory raced during rollback; unrelated directory preserved.".into());
        }
        // Removal refuses nonempty directories. Return external content to its
        // original path when possible; collisions never overwrite either copy.
        if unsafe { libc::unlinkat(p.directory.as_raw_fd(), held.as_ptr(), libc::AT_REMOVEDIR) } < 0
        {
            let restored = move_exclusive(&p, &held, &p.leaf).is_ok();
            return Err(format!(
                "Created directory has external contents or changed; retained{}.",
                if restored {
                    " at its original path"
                } else {
                    " under its temporary sibling name; inspect manually"
                }
            ));
        }
        p.directory.sync_all().map_err(|e| e.to_string())?;
    }
    Ok(())
}
