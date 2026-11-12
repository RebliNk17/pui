use std::{
    path::{Component, Path, PathBuf},
    process::Command,
};

pub(crate) mod branches;

#[derive(Clone, Debug, Default)]
pub struct FileChange {
    pub path: String,
    pub status: String,
    pub added: String,
    pub removed: String,
}

#[derive(Clone, Debug, Default)]
pub struct Workspace {
    pub root: PathBuf,
    pub branch: String,
    pub files: Vec<FileChange>,
    pub is_git: bool,
}

fn git(cwd: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let mut command = Command::new("git");
    command
        .arg("--no-optional-locks")
        .args(args)
        .current_dir(cwd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().ok()?;
    output.status.success().then_some(output.stdout)
}

pub fn inspect(cwd: &Path) -> Workspace {
    let mut workspace = Workspace {
        root: cwd.to_path_buf(),
        ..Default::default()
    };
    let Some(root) = git(cwd, &["rev-parse", "--show-toplevel"]) else {
        return workspace;
    };
    workspace.is_git = true;
    workspace.root = PathBuf::from(String::from_utf8_lossy(&root).trim());
    workspace.branch = git(cwd, &["branch", "--show-current"])
        .map(|b| String::from_utf8_lossy(&b).trim().to_owned())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "detached HEAD".into());
    let status = git(
        &workspace.root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=normal"],
    )
    .unwrap_or_default();
    let mut records = status.split(|b| *b == 0).filter(|r| !r.is_empty());
    while let Some(record) = records.next() {
        if record.len() < 4 {
            continue;
        }
        let status = String::from_utf8_lossy(&record[..2]).into_owned();
        let path = String::from_utf8_lossy(&record[3..]).into_owned();
        if status.contains('R') || status.contains('C') {
            records.next();
        }
        workspace.files.push(FileChange {
            path,
            status,
            ..Default::default()
        });
    }
    let stats = git(
        &workspace.root,
        &[
            "diff",
            "HEAD",
            "--no-ext-diff",
            "--no-textconv",
            "--numstat",
            "-z",
        ],
    )
    .unwrap_or_default();
    for record in stats.split(|b| *b == 0) {
        let record = String::from_utf8_lossy(record);
        let mut fields = record.splitn(3, '\t');
        if let (Some(added), Some(removed), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        {
            if let Some(file) = workspace.files.iter_mut().find(|f| f.path == path) {
                file.added = added.into();
                file.removed = removed.into();
            }
        }
    }
    workspace
}

pub(crate) const MAX_DIFF_BYTES: usize = 200_000;

pub(crate) fn valid_diff_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.chars().any(char::is_control)
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

/// Read one literal working-tree path without running configured diff helpers.
/// Bound the pipe itself, not an already-allocated `Command::output` buffer.
/// Errors are separate from patch text in both review presentations.
pub fn diff(root: &Path, file: &FileChange) -> Result<String, String> {
    if !valid_diff_path(&file.path) {
        return Err("Git review requires one regular relative file path (up to 4 KiB, no control characters).".into());
    }
    if file.status == "??" {
        return Ok(
            "Untracked file — Git has no diff yet. Pui does not stage or modify files.".into(),
        );
    }
    use std::{io::Read, process::Stdio};
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args([
            "--no-optional-locks",
            "--literal-pathspecs",
            "diff",
            "HEAD",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--",
            &file.path,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Cannot start read-only Git diff.".to_owned())?;
    let mut bytes = Vec::with_capacity(MAX_DIFF_BYTES + 1);
    let read = child
        .stdout
        .take()
        .unwrap()
        .take((MAX_DIFF_BYTES + 1) as u64)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() > MAX_DIFF_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        return Err(if read.is_err() {
            "Cannot read Git diff. No file was modified.".into()
        } else {
            "Git diff exceeds the 200,000-byte review limit. No file was modified.".into()
        });
    }
    let status = child
        .wait()
        .map_err(|_| "Cannot finish read-only Git diff.".to_owned())?;
    if !status.success() {
        return Err("Git diff unavailable (repository or HEAD may have changed).".into());
    }
    String::from_utf8(bytes).map_err(|_| "Git diff is not UTF-8.".into())
}

#[cfg(test)]
pub(crate) mod tests;
