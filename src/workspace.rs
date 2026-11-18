use std::{
    path::{Component, Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Instant,
};

pub(crate) mod branches;
pub(crate) mod clone;
pub(crate) mod commit;
pub(crate) mod fast_forward;
pub(crate) mod fetch;
pub(crate) mod history;
pub(crate) mod index_action;
pub(crate) mod initialize;
pub(crate) mod push;
pub(crate) mod read_only;
use read_only::DEADLINE;

pub(crate) const MAX_FILES: usize = 256;
pub(crate) const MAX_PATH_BYTES: usize = 64 * 1024;
const MAX_STATUS_BYTES: usize = 128 * 1024;
const MAX_ROOT_BYTES: usize = 8192;
const MAX_BRANCH_BYTES: usize = 1024;
pub(crate) const MAX_DIFF_BYTES: usize = 200_000;

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
    /// Incomplete/unavailable reads must never masquerade as a clean worktree.
    pub error: Option<String>,
}

fn git(
    cwd: &Path,
    args: &[&str],
    cancel: &AtomicBool,
    deadline: Instant,
    max_bytes: usize,
    over_limit: &str,
) -> Result<(Vec<u8>, std::process::ExitStatus), String> {
    read_only::run(
        read_only::command(cwd).args(args),
        cancel,
        deadline,
        max_bytes,
        "Git workspace read",
        over_limit,
    )
}

fn line(bytes: &[u8], max_bytes: usize, label: &str) -> Result<String, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| format!("Git {label} is not UTF-8."))?;
    let text = text
        .strip_suffix('\n')
        .filter(|value| !value.is_empty() && value.len() <= max_bytes)
        .filter(|value| !value.chars().any(char::is_control))
        .ok_or_else(|| format!("Git {label} response is invalid or too large."))?;
    Ok(text.into())
}
fn status(bytes: &[u8]) -> Result<Vec<FileChange>, String> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut records = bytes.strip_suffix(&[0]).unwrap_or(bytes).split(|b| *b == 0);
    let mut files = Vec::new();
    let mut path_bytes = 0;
    if !bytes.is_empty() && bytes.last() != Some(&0) {
        return Err("Invalid Git status framing. No partial file list is shown.".into());
    }
    while let Some(record) = records.next() {
        if record.len() < 4
            || record[2] != b' '
            || !record[..2].iter().all(|b| b" MADRCU?!T".contains(b))
        {
            return Err("Invalid Git status record. No partial file list is shown.".into());
        }
        let path = std::str::from_utf8(&record[3..])
            .map_err(|_| "Git status path is not UTF-8. No partial file list is shown.")?;
        if !valid_diff_path(path) {
            return Err("Git status requires regular relative paths up to 4 KiB without control characters. No partial file list is shown.".into());
        }
        if record[..2].contains(&b'R') || record[..2].contains(&b'C') {
            let source = records.next().ok_or("Invalid Git rename status framing.")?;
            let source =
                std::str::from_utf8(source).map_err(|_| "Git rename path is not UTF-8.")?;
            if !valid_diff_path(source) {
                return Err("Invalid Git rename path. No partial file list is shown.".into());
            }
        }
        path_bytes += path.len();
        if files.len() == MAX_FILES || path_bytes > MAX_PATH_BYTES {
            return Err(
                "Git status exceeds 256 paths or 64 KiB of paths. No partial file list is shown."
                    .into(),
            );
        }
        files.push(FileChange {
            path: path.into(),
            status: String::from_utf8(record[..2].to_vec()).unwrap(),
            added: "?".into(),
            removed: "?".into(),
        });
    }
    Ok(files)
}
fn stats(bytes: &[u8], files: &mut [FileChange]) -> Result<(), String> {
    if !bytes.is_empty() && bytes.last() != Some(&0) {
        return Err("Invalid Git change-count framing.".into());
    }
    let mut records = bytes.strip_suffix(&[0]).unwrap_or(bytes).split(|b| *b == 0);
    while let Some(record) = records.next() {
        if bytes.is_empty() {
            break;
        }
        let record = std::str::from_utf8(record).map_err(|_| "Git change counts are not UTF-8.")?;
        let mut fields = record.splitn(3, '\t');
        let (Some(added), Some(removed), Some(mut path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            return Err("Invalid Git change counts.".into());
        };
        if ![added, removed].iter().all(|value| {
            *value == "-"
                || (!value.is_empty()
                    && value.len() <= 20
                    && value.bytes().all(|b| b.is_ascii_digit()))
        }) {
            return Err("Invalid Git change counts.".into());
        }
        if path.is_empty() {
            let source = records.next().ok_or("Invalid Git rename counts.")?;
            let target = records.next().ok_or("Invalid Git rename counts.")?;
            if !std::str::from_utf8(source).is_ok_and(valid_diff_path) {
                return Err("Invalid Git rename path.".into());
            }
            path = std::str::from_utf8(target).map_err(|_| "Git rename path is not UTF-8.")?;
        }
        if !valid_diff_path(path) {
            return Err("Invalid Git change-count path.".into());
        }
        if let Some(file) = files.iter_mut().find(|file| file.path == path) {
            file.added = added.into();
            file.removed = removed.into();
        }
    }
    for file in files {
        if file.added == "?" {
            file.added = "0".into();
            file.removed = "0".into();
        }
    }
    Ok(())
}

/// One shared three-second deadline covers root, branch, status and counts.
/// Failed status is explicit and empty, never an apparently clean partial list.
pub(crate) fn read(cwd: &Path, cancel: &AtomicBool) -> Workspace {
    let mut workspace = Workspace {
        root: cwd.to_path_buf(),
        ..Default::default()
    };
    let deadline = Instant::now() + DEADLINE;
    let result = (|| {
        if cwd.as_os_str().len() > MAX_ROOT_BYTES {
            workspace.root = PathBuf::new();
            return Err("Workspace root exceeds 8 KiB.".into());
        }
        let (root, exit) = git(
            cwd,
            &["rev-parse", "--show-toplevel"],
            cancel,
            deadline,
            MAX_ROOT_BYTES + 1,
            "Git root exceeds 8 KiB.",
        )?;
        if !exit.success() {
            return Err("Git repository unavailable at this session's workspace root.".into());
        }
        let root = line(&root, MAX_ROOT_BYTES, "root")?;
        if !Path::new(&root).is_absolute() {
            return Err("Git returned a non-absolute repository root.".into());
        }
        workspace.root = root.into();
        workspace.is_git = true;
        let (branch, exit) = git(
            &workspace.root,
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
            cancel,
            deadline,
            MAX_BRANCH_BYTES + 1,
            "Git branch exceeds 1 KiB.",
        )?;
        workspace.branch = if exit.code() == Some(1) && branch.is_empty() {
            "detached HEAD".into()
        } else if exit.success() {
            line(&branch, MAX_BRANCH_BYTES, "branch")?
        } else {
            return Err("Git current branch unavailable.".into());
        };
        read_only::filters_disabled(&workspace.root, cancel, deadline)?;
        let (bytes, exit) = git(
            &workspace.root,
            &[
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=normal",
                "--ignore-submodules=dirty",
            ],
            cancel,
            deadline,
            MAX_STATUS_BYTES,
            "Git status exceeds 128 KiB. No partial file list is shown.",
        )?;
        if !exit.success() {
            return Err("Git status unavailable. No partial file list is shown.".into());
        }
        workspace.files = status(&bytes)?;
        let (_, head) = commit::branch_head(&workspace.root, cancel, deadline)?;
        let initial = head.bytes().all(|b| b == b'0');
        let mut count_args = vec![
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--numstat",
            "-z",
            "--ignore-submodules=dirty",
        ];
        if initial {
            count_args.push("--cached");
        } else {
            count_args.push(&head);
        }
        count_args.push("--");
        let (bytes, exit) = git(
            &workspace.root,
            &count_args,
            cancel,
            deadline,
            MAX_STATUS_BYTES,
            "Git change counts exceed 128 KiB; counts are unavailable.",
        )?;
        if !exit.success() {
            return Err(
                "Git change counts unavailable (repository or HEAD may have changed).".into(),
            );
        }
        stats(&bytes, &mut workspace.files)
    })();
    if let Err(error) = result {
        workspace.error = Some(error);
        // Even a partially parsed count response cannot appear complete.
        for file in &mut workspace.files {
            file.added = "?".into();
            file.removed = "?".into();
        }
    }
    workspace
}
#[cfg(test)]
pub fn inspect(cwd: &Path) -> Workspace {
    read(cwd, &AtomicBool::new(false))
}

pub(crate) fn valid_diff_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.chars().any(char::is_control)
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

/// Read one literal path; never run diff helpers or open an untracked file.
#[cfg(test)]
pub(crate) fn diff_cancellable(
    root: &Path,
    file: &FileChange,
    cancel: &AtomicBool,
) -> Result<String, String> {
    diff_mode(root, file, cancel, DiffMode::Combined)
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum DiffMode {
    #[default]
    Combined,
    Staged,
    Unstaged,
}
pub(crate) fn diff_mode(
    root: &Path,
    file: &FileChange,
    cancel: &AtomicBool,
    mode: DiffMode,
) -> Result<String, String> {
    if !valid_diff_path(&file.path) {
        return Err("Git review requires one regular relative file path (up to 4 KiB, no control characters).".into());
    }
    if cancel.load(std::sync::atomic::Ordering::Acquire) {
        return Err("Git diff cancelled.".into());
    }
    if file.status == "??" {
        return Ok(
            "Untracked file — Git has no diff yet. Pui does not stage or modify files.".into(),
        );
    }
    if root.as_os_str().len() > MAX_ROOT_BYTES {
        return Err("Git diff root exceeds 8 KiB.".into());
    }
    let deadline = Instant::now() + DEADLINE;
    read_only::filters_disabled(root, cancel, deadline)?;
    let mut cmd = read_only::command(root);
    cmd.args(["--literal-pathspecs", "diff"]);
    match mode {
        DiffMode::Combined => {
            cmd.arg("HEAD");
        }
        DiffMode::Staged => {
            cmd.args(["--cached", "HEAD"]);
        }
        DiffMode::Unstaged => {}
    }
    let (bytes, exit) = read_only::run(
        cmd.args([
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--ignore-submodules=dirty",
            "--",
            &file.path,
        ]),
        cancel,
        deadline,
        MAX_DIFF_BYTES,
        "Git diff",
        "Git diff exceeds the 200,000-byte review limit. No file was modified.",
    )?;
    if !exit.success() {
        return Err("Git diff unavailable (repository or HEAD may have changed).".into());
    }
    String::from_utf8(bytes).map_err(|_| "Git diff is not UTF-8.".into())
}
#[cfg(test)]
pub fn diff(root: &Path, file: &FileChange) -> Result<String, String> {
    diff_cancellable(root, file, &AtomicBool::new(false))
}

#[cfg(test)]
mod filter_tests;
#[cfg(test)]
mod status_tests;
#[cfg(test)]
pub(crate) mod tests;
