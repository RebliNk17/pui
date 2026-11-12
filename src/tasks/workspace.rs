//! Bounded source fingerprints and deliberate Git isolation. Never commit/merge/delete.
#[path = "workspace/snapshot.rs"]
mod snapshot;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const MAX_FILES: usize = 30000;
const MAX_BYTES: u64 = 256 * 1024 * 1024;
fn git(cwd: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git")
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|e| format!("Cannot inspect Git: {e}"))
}
#[derive(Debug)]
pub struct Inspection {
    pub root: PathBuf,
    pub git: bool,
    pub dirty: bool,
    pub head: String,
}
pub fn inspect(source: &Path) -> Result<Inspection, String> {
    let source = fs::canonicalize(source).map_err(|e| format!("Workspace unavailable: {e}"))?;
    if !source.is_dir() {
        return Err("Workspace is not a directory.".into());
    }
    let marker = source.ancestors().any(|p| p.join(".git").exists());
    let root = match git(&source, &["rev-parse", "--show-toplevel"]) {
        Ok(o) if o.status.success() => PathBuf::from(
            String::from_utf8(o.stdout)
                .map_err(|_| "Non-UTF-8 repository path")?
                .trim(),
        ),
        _ if marker => {
            return Err(
                "Git metadata exists but cannot be inspected; refusing an in-place fallback."
                    .into(),
            );
        }
        _ => {
            return Ok(Inspection {
                root: source,
                git: false,
                dirty: false,
                head: String::new(),
            });
        }
    };
    let head = git(&source, &["rev-parse", "--verify", "HEAD"])?;
    if !head.status.success() {
        return Err(
            "A Git task needs a committed HEAD for isolation. Nothing was committed.".into(),
        );
    }
    let status = git(
        &root,
        &["status", "--porcelain", "--untracked-files=normal"],
    )?;
    if !status.status.success() {
        return Err("Cannot establish whether the source repository is dirty.".into());
    }
    Ok(Inspection {
        root,
        git: true,
        dirty: !status.stdout.is_empty(),
        head: String::from_utf8_lossy(&head.stdout).trim().into(),
    })
}
fn validate_source(inspection: &Inspection, expected_head: &str) -> Result<(), String> {
    if inspection.head != expected_head {
        return Err("Committed HEAD changed since the proposal. Create a new task so checks are proposed against the correct source.".into());
    }
    Ok(())
}
pub fn prepare(
    source: &Path,
    id: &str,
    home: &Path,
    expected_head: &str,
) -> Result<(PathBuf, bool), String> {
    let inspection = inspect(source)?;
    if !inspection.git {
        if !expected_head.is_empty() {
            return Err(
                "Repository disappeared since proposal. Refusing an in-place fallback.".into(),
            );
        }
        return Ok((inspection.root, false));
    }
    validate_source(&inspection, expected_head)?;
    let before = snapshot::capture(&inspection.root, None)?;
    fs::create_dir_all(home).map_err(|e| e.to_string())?;
    let target = home.join(format!("task-{id}"));
    let output = Command::new("git")
        // Populate files ourselves: no checkout filters, hooks, or destination symlinks.
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "worktree",
            "add",
            "--detach",
            "--no-checkout",
        ])
        .arg(&target)
        .arg(&inspection.head)
        .current_dir(&inspection.root)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(
            "Cannot create isolated worktree. Source files were not changed; no fallback was used."
                .into(),
        );
    }
    let copied = (|| {
        let copied = snapshot::capture(&inspection.root, Some(&target))?;
        let after = snapshot::capture(&inspection.root, None)?;
        validate_source(&inspect(source)?, expected_head)?;
        if before != copied || copied != after || snapshot::capture(&target, None)? != copied {
            return Err("Source changed while creating the workflow snapshot. Start a new workflow when edits have settled.".into());
        }
        Ok::<_, String>(())
    })();
    copied.map_err(|e| {
        format!(
            "{e} Original files and index were not modified. Worktree retained at {}.",
            target.display()
        )
    })?;
    let relative = fs::canonicalize(source)
        .map_err(|e| e.to_string())?
        .strip_prefix(&inspection.root)
        .map_err(|_| "Workspace not inside Git root")?
        .to_path_buf();
    let cwd = target.join(relative);
    if !cwd.is_dir() {
        return Err(
            "Source subfolder is absent from the snapshot. Retained worktree was not deleted."
                .into(),
        );
    }
    Ok((cwd, true))
}
fn excluded(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str(),
        Some(
            ".git"
                | ".hg"
                | ".svn"
                | "node_modules"
                | "target"
                | "dist"
                | "build"
                | ".next"
                | ".venv"
                | "venv"
                | "__pycache__"
                | ".pytest_cache"
                | ".DS_Store"
        )
    )
}
fn paths(root: &Path) -> Result<Vec<PathBuf>, String> {
    if let Ok(output) = git(
        root,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    ) {
        if output.status.success() {
            let mut paths = Vec::new();
            for raw in output.stdout.split(|b| *b == 0).filter(|s| !s.is_empty()) {
                let s = std::str::from_utf8(raw)
                    .map_err(|_| "Non-UTF-8 source path cannot be fingerprinted")?;
                let p = PathBuf::from(s);
                if p.is_absolute()
                    || p.components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    return Err("Unsafe source path".into());
                }
                paths.push(p);
                if paths.len() > MAX_FILES {
                    return Err("Too many source files to fingerprint safely.".into());
                }
            }
            paths.sort();
            paths.dedup();
            return Ok(paths);
        }
    }
    let mut found = Vec::new();
    let mut queue = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = queue.pop() {
        if depth > 64 {
            return Err("Source tree is too deep.".into());
        }
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let e = entry.map_err(|e| e.to_string())?;
            if excluded(&e.file_name()) {
                continue;
            }
            let meta = fs::symlink_metadata(e.path()).map_err(|e| e.to_string())?;
            if meta.is_dir() {
                queue.push((e.path(), depth + 1));
            } else {
                found.push(
                    e.path()
                        .strip_prefix(root)
                        .map_err(|e| e.to_string())?
                        .to_path_buf(),
                );
            }
            if found.len() + queue.len() > MAX_FILES {
                return Err("Too many source entries to fingerprint safely.".into());
            }
        }
    }
    found.sort();
    Ok(found)
}
/// SHA-256 over path, length, and bytes. Symlinks are hashed as links, never followed.
/// Git: tracked + nonignored untracked files; non-Git: bounded tree excluding generated directories.
pub fn fingerprint(root: &Path) -> Result<String, String> {
    let mut canonical = fs::canonicalize(root).map_err(|e| e.to_string())?;
    if let Ok(output) = git(&canonical, &["rev-parse", "--show-toplevel"]) {
        if output.status.success() {
            canonical = PathBuf::from(
                String::from_utf8(output.stdout)
                    .map_err(|_| "Non-UTF-8 repository root")?
                    .trim(),
            );
        }
    }
    let root = canonical.as_path();
    let initial_paths = paths(root)?;
    let mut hash = Sha256::new();
    let mut total = 0;
    for relative in &initial_paths {
        let name = relative.to_str().ok_or("Non-UTF-8 source path")?;
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        let path = root.join(relative);
        // Refuse a symlinked parent, including tracked paths below an external directory link.
        let mut parent = path.parent();
        while let Some(p) = parent.filter(|p| *p != root) {
            match fs::symlink_metadata(p) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err("Symlinked source directory cannot be fingerprinted safely.".into());
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            }
            parent = p.parent();
        }
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                hash.update(b"missing");
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        if meta.file_type().is_symlink() {
            hash.update(b"link");
            hash.update(
                fs::read_link(path)
                    .map_err(|e| e.to_string())?
                    .as_os_str()
                    .as_encoded_bytes(),
            );
            continue;
        }
        if !meta.is_file() {
            return Err("Special source file cannot be fingerprinted.".into());
        }
        total += meta.len();
        if total > MAX_BYTES {
            return Err(
                "Source exceeds the 256 MiB fingerprint budget. No checked result was claimed."
                    .into(),
            );
        }
        hash.update(b"file");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            hash.update((meta.permissions().mode() & 0o777).to_le_bytes());
        }
        hash.update(meta.len().to_le_bytes());
        let mut file = fs::File::open(&path).map_err(|e| e.to_string())?;
        let mut buffer = [0; 65536];
        let mut read = 0;
        loop {
            let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            read += n as u64;
            if read > meta.len() {
                return Err("Source changed during fingerprinting.".into());
            }
            hash.update(&buffer[..n]);
        }
        let after = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if read != meta.len() || after.modified().ok() != meta.modified().ok() {
            return Err("Source changed during fingerprinting.".into());
        }
    }
    if paths(root)? != initial_paths {
        return Err("Source paths changed during fingerprinting.".into());
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn dir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("pui-task-ws-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&p).unwrap();
        p
    }
    #[test]
    fn non_git_works_in_place_and_fingerprints_content_not_generated_outputs() {
        let root = dir();
        fs::write(root.join("app.rs"), "draft remains").unwrap();
        assert!(!inspect(&root).unwrap().git);
        assert_eq!(
            prepare(&root, "fixture", &root.join("unused"), "").unwrap(),
            (fs::canonicalize(&root).unwrap(), false)
        );
        let a = fingerprint(&root).unwrap();
        fs::create_dir(root.join("target")).unwrap();
        fs::write(root.join("target/output"), "ignored").unwrap();
        assert_eq!(a, fingerprint(&root).unwrap());
        fs::write(root.join("app.rs"), "draft deleted").unwrap();
        assert_ne!(a, fingerprint(&root).unwrap());
        fs::write(root.join("new.rs"), "new").unwrap();
        let b = fingerprint(&root).unwrap();
        fs::remove_file(root.join("new.rs")).unwrap();
        assert_ne!(b, fingerprint(&root).unwrap());
    }
    #[test]
    fn dirty_source_is_allowed_but_head_must_match() {
        let i = Inspection {
            root: PathBuf::from("/fixture"),
            git: true,
            dirty: true,
            head: "head-a".into(),
        };
        assert!(validate_source(&i, "head-a").is_ok());
        assert!(validate_source(&i, "head-b").is_err());
    }
    #[test]
    fn broken_git_metadata_never_falls_back_in_place() {
        let root = dir();
        fs::write(root.join(".git"), "gitdir: /does/not/exist").unwrap();
        assert!(inspect(&root).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn source_symlinks_are_not_followed() {
        let root = dir();
        std::os::unix::fs::symlink("/does/not/exist", root.join("link")).unwrap();
        assert!(fingerprint(&root).is_ok());
    }
}
