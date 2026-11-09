use std::{
    path::{Path, PathBuf},
    process::Command,
};

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

pub fn diff(root: &Path, file: &FileChange) -> String {
    if file.status == "??" {
        return "Untracked file — Git has no diff yet. Add the file to the index outside Pui to compare it. Pui does not stage or modify files.".into();
    }
    git(
        root,
        &[
            "diff",
            "HEAD",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--",
            &file.path,
        ],
    )
    .map(|b| String::from_utf8_lossy(&b[..b.len().min(200_000)]).into_owned())
    .unwrap_or_else(|| "No diff available (the repository may not have an initial commit).".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn non_repository_is_handled() {
        let workspace = inspect(Path::new("/this-path-does-not-exist-pui"));
        assert!(!workspace.is_git);
        assert!(workspace.files.is_empty());
    }
}
