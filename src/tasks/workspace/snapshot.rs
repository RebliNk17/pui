//! Copy a bounded, verified working state, including the index, without touching the source.
use super::*;
use std::{collections::BTreeSet, io::Write};
#[path = "snapshot/source.rs"]
pub(in crate::tasks) mod source;
use source::{Entry, entry};

fn output(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = git(root, args)?;
    if !out.status.success() {
        return Err("Cannot enumerate the Git snapshot; no in-place fallback was used.".into());
    }
    Ok(out.stdout)
}

fn relative(raw: &[u8]) -> Result<PathBuf, String> {
    let name = std::str::from_utf8(raw).map_err(|_| "Non-UTF-8 snapshot path is unsupported.")?;
    let path = PathBuf::from(name);
    if path.as_os_str().is_empty()
        || path.components().any(|c| match c {
            std::path::Component::Normal(n) => n.eq_ignore_ascii_case(".git"),
            _ => true,
        })
    {
        return Err("Unsafe snapshot path.".into());
    }
    Ok(path)
}

fn entries(root: &Path) -> Result<(Vec<u8>, BTreeSet<PathBuf>), String> {
    let index = output(root, &["ls-files", "--stage", "--sparse", "-z"])?;
    if !output(root, &["rev-parse", "--shared-index-path"])?.is_empty() {
        return Err("Split indexes are not yet supported by workflow snapshots.".into());
    }
    let head = output(root, &["ls-tree", "-r", "-z", "HEAD"])?;
    let mut paths = BTreeSet::new();
    for (data, staged) in [(&index, true), (&head, false)] {
        for entry in data.split(|b| *b == 0).filter(|e| !e.is_empty()) {
            let tab = entry
                .iter()
                .position(|b| *b == b'\t')
                .ok_or("Invalid Git entry.")?;
            let header = &entry[..tab];
            if header.starts_with(b"040000 ") {
                return Err(
                    "Sparse directory indexes are not yet supported by workflow snapshots.".into(),
                );
            }
            if header.starts_with(b"160000 ") {
                return Err("Workflow snapshots do not yet support submodules. Original files were not changed.".into());
            }
            if staged && !header.ends_with(b" 0") {
                return Err("Resolve unmerged index entries before starting a workflow.".into());
            }
            paths.insert(relative(&entry[tab + 1..])?);
        }
    }
    let untracked = output(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    for name in untracked.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        paths.insert(relative(name)?);
    }
    if paths.len() > MAX_FILES {
        return Err("Too many files for a workflow snapshot.".into());
    }
    // Preserve all index flags (including intent-to-add), not just staged blob IDs.
    let index = index_path(root)?;
    let bytes = match fs::File::open(index) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() as u64 > MAX_BYTES {
                return Err("Index exceeds the snapshot budget.".into());
            }
            bytes
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e.to_string()),
    };
    Ok((bytes, paths))
}

fn index_path(root: &Path) -> Result<PathBuf, String> {
    let raw = output(root, &["rev-parse", "--git-path", "index"])?;
    let name = std::str::from_utf8(&raw)
        .map_err(|_| "Non-UTF-8 index path.")?
        .trim_end_matches('\n');
    Ok(root.join(name))
}

/// Content/mode/index digest. Missing tracked paths remain missing; ignored files stay out.
/// Copy only into a fresh --no-checkout worktree, never into the source or an existing tree.
pub(super) fn capture(root: &Path, target: Option<&Path>) -> Result<String, String> {
    let (index, paths) = entries(root)?;
    let mut hash = Sha256::new();
    hash.update((index.len() as u64).to_le_bytes());
    hash.update(&index);
    if let Some(target) = target {
        if !index.is_empty() {
            fs::write(index_path(target)?, &index).map_err(|e| e.to_string())?;
        }
    }
    let mut total = 0;
    for relative in paths {
        check_operation()?;
        let entry = entry(root, &relative)?;
        if matches!(entry, Entry::Missing) {
            continue;
        }
        let name = relative.to_str().ok_or("Non-UTF-8 snapshot path.")?;
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        let destination = target.map(|t| t.join(&relative));
        if let Some(dest) = &destination {
            fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
        }
        let (file, meta) = match entry {
            Entry::File(file) => {
                let meta = file.metadata().map_err(|e| e.to_string())?;
                (file, meta)
            }
            Entry::Link(link) => {
                total += link.as_os_str().len() as u64;
                if total > MAX_BYTES {
                    return Err("Source exceeds the 256 MiB snapshot budget.".into());
                }
                hash.update(b"link");
                hash.update(link.as_os_str().len().to_le_bytes());
                hash.update(link.as_os_str().as_encoded_bytes());
                if let Some(dest) = destination {
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(&link, dest).map_err(|e| e.to_string())?;
                    #[cfg(not(unix))]
                    {
                        let _ = dest;
                        return Err("Symlink snapshots are unsupported on this platform.".into());
                    }
                }
                continue;
            }
            Entry::Missing => unreachable!(),
        };
        total += meta.len();
        if total > MAX_BYTES {
            return Err("Source exceeds the 256 MiB snapshot budget.".into());
        }
        hash.update(b"file");
        hash.update(meta.len().to_le_bytes());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            hash.update((meta.permissions().mode() & 0o777).to_le_bytes());
        }
        let mut dest = destination
            .as_ref()
            .map(|p| fs::OpenOptions::new().write(true).create_new(true).open(p))
            .transpose()
            .map_err(|e| e.to_string())?;
        let mut read = 0;
        let mut reader = (&file).take(meta.len().saturating_add(1));
        let mut buffer = [0; 65536];
        loop {
            check_operation()?;
            let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            read += n as u64;
            hash.update(&buffer[..n]);
            if let Some(dest) = &mut dest {
                dest.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
            }
        }
        let after = file.metadata().map_err(|e| e.to_string())?;
        if read != meta.len()
            || after.modified().ok() != meta.modified().ok()
            || after.file_type() != meta.file_type()
        {
            return Err("Source changed during snapshotting.".into());
        }
        if let Some(dest) = dest {
            dest.set_permissions(meta.permissions())
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
#[path = "snapshot/tests.rs"]
mod tests;
