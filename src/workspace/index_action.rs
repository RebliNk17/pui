//! Explicit single-file index transactions. Never commits or changes working files.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    sync::atomic::Ordering,
};
const MAX_INDEX: usize = 16 * 1024 * 1024;
const MAX_FILE: usize = 8 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Stage,
    Unstage,
    Discard,
}
impl Action {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Stage => "Stage file",
            Self::Unstage => "Unstage file",
            Self::Discard => "Discard unstaged changes",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub root: PathBuf,
    pub path: String,
    pub status: String,
    pub action: Action,
    pub preview: String,
    pub hunk: hunks::selection::Selection,
    pub lines: hunks::lines::LineSelection,
    index_path: PathBuf,
    index: Vec<u8>,
    head: String,
    branch: String,
    file: Option<[u8; 32]>,
    restore: Option<Vec<u8>>,
}
impl Snapshot {
    pub(crate) fn recovery_path(&self) -> PathBuf {
        let mut digest = Sha256::new();
        digest.update(self.path.as_bytes());
        if let Some(file) = self.file {
            digest.update(file);
        }
        self.index_path
            .with_file_name(format!("pui-discard-recovery-{:x}", digest.finalize()))
    }
}
fn bytes(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "Cannot inspect index action input.")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > max as u64 {
        return Err("Index action requires bounded regular files, not symlinks.".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "Cannot read index action input.")?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read index action input.")?;
    if bytes.len() > max {
        return Err("Index action input exceeds limit.".into());
    }
    Ok(bytes)
}
fn run(
    root: &Path,
    args: &[&str],
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<u8>, String> {
    let (out, status) = read_only::run(
        read_only::command(root).args(args),
        cancel,
        deadline,
        128 * 1024,
        "Index action inspection",
        "Index action response exceeds 128 KiB.",
    )?;
    if !status.success() {
        return Err("Git index action inspection failed. Refresh and inspect again.".into());
    }
    Ok(out)
}
pub(crate) fn available(status: &str, action: Action) -> bool {
    let s = status.as_bytes();
    s.len() == 2
        && !s.iter().any(|b| b"RCUT!".contains(b))
        && match action {
            Action::Stage => matches!(s[1], b'M' | b'D') || status == "??",
            Action::Unstage => matches!(s[0], b'M' | b'A' | b'D'),
            Action::Discard => matches!(status, " M" | "MM"),
        }
}
pub(crate) fn prepare(
    root: &Path,
    path: &str,
    status: &str,
    action: Action,
    cancel: &AtomicBool,
) -> Result<Snapshot, String> {
    let deadline = Instant::now() + DEADLINE;
    prepare_at(root, path, status, action, cancel, deadline)
}
fn prepare_at(
    root: &Path,
    path: &str,
    status: &str,
    action: Action,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Snapshot, String> {
    if !root.is_absolute() || !valid_diff_path(path) || !available(status, action) {
        return Err("Unsupported index action. Renames, conflicts, type changes and submodules are not supported.".into());
    }
    read_only::filters_disabled(root, cancel, deadline)?;
    let meta = run(
        root,
        &[
            "rev-parse",
            "--show-toplevel",
            "--path-format=absolute",
            "--git-path",
            "index",
            "--shared-index-path",
        ],
        cancel,
        deadline,
    )?;
    let meta_str = std::str::from_utf8(&meta).map_err(|_| "Invalid index metadata.")?;
    let mut meta_lines = meta_str.lines();
    let actual = meta_lines.next().ok_or("Repository root unavailable.")?;
    if Path::new(actual) != root {
        return Err("Repository root changed.".into());
    }
    let index_line = meta_lines.next().ok_or("Invalid index path.")?;
    if meta_lines.next().is_some_and(|s| !s.is_empty()) || meta_lines.next().is_some() {
        return Err("Split index is unsupported for index actions.".into());
    }
    let (branch, head) = super::commit::branch_head(root, cancel, deadline)?;
    let initial = head.bytes().all(|b| b == b'0');
    if initial && action == Action::Discard {
        return Err("Discard before first commit is unsupported.".into());
    }
    let index_path = PathBuf::from(line(
        format!("{index_line}\n").as_bytes(),
        8192,
        "index path",
    )?);
    if !index_path.is_absolute() {
        return Err("Invalid index path.".into());
    }
    let index = inspect_index(&index_path, initial && action == Action::Stage)?;
    if !index.is_empty() && !index.starts_with(b"DIRC") {
        return Err("Invalid index header.".into());
    }
    let mut cmd = read_only::command(root);
    literal(&mut cmd);
    cmd.args([
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--ignore-submodules=none",
        "--",
        path,
    ]);
    let (out, exit) = read_only::run(
        &mut cmd,
        cancel,
        deadline,
        8192,
        "Exact file status",
        "Exact file status exceeds limit.",
    )?;
    let expected = format!("{status} {path}\0");
    if !exit.success() || out != expected.as_bytes() {
        return Err("File status changed. Refresh and inspect again.".into());
    }
    let mut cmd = read_only::command(root);
    literal(&mut cmd);
    cmd.args(["ls-files", "--stage", "-z", "--", path]);
    let (out, exit) = read_only::run(
        &mut cmd,
        cancel,
        deadline,
        8192,
        "Index entry",
        "Index entry exceeds limit.",
    )?;
    if !exit.success()
        || (!out.is_empty()
            && (!out.starts_with(b"100644 ") && !out.starts_with(b"100755 ")
                || out.iter().filter(|b| **b == 0).count() != 1))
    {
        return Err("Only ordinary non-conflicted files are supported.".into());
    }
    let restore = if action == Action::Discard {
        Some(discard::inspect(root, path, &out, cancel, deadline)?)
    } else {
        None
    };
    // Reject symlink ancestors even for deleted paths. This is not a hostile filesystem sandbox.
    let relative = Path::new(path);
    let mut full = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    for component in &components[..components.len() - 1] {
        full.push(component);
        let m = fs::symlink_metadata(&full).map_err(|_| "Missing file parent.")?;
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err("Symlink file parents are unsupported.".into());
        }
    }
    let full = root.join(path);
    let preview = preview(root, path, status, action, cancel, deadline, initial)?;
    if action == Action::Discard
        && preview
            .lines()
            .any(|l| l.starts_with("old mode ") || l.starts_with("new mode "))
    {
        return Err("Discard does not restore file modes.".into());
    }
    let file = match fs::symlink_metadata(&full) {
        Ok(_) => Some(Sha256::digest(bytes(&full, MAX_FILE)?).into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("Cannot inspect working file.".into()),
    };
    if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Err("Index inspection cancelled or timed out.".into());
    }
    Ok(Snapshot {
        root: root.into(),
        path: path.into(),
        status: status.into(),
        action,
        preview,
        hunk: Default::default(),
        lines: Default::default(),
        index_path,
        index,
        head,
        branch,
        file,
        restore,
    })
}
fn inspect_index(path: &Path, allow_missing: bool) -> Result<Vec<u8>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if allow_missing && e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        _ => bytes(path, MAX_INDEX),
    }
}
fn preview(
    root: &Path,
    path: &str,
    status: &str,
    action: Action,
    cancel: &AtomicBool,
    deadline: Instant,
    initial: bool,
) -> Result<String, String> {
    let bytes = if status == "??" {
        bytes(&root.join(path), 128 * 1024)?
    } else {
        let mut cmd = read_only::command(root);
        literal(&mut cmd);
        cmd.args([
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--no-renames",
            "--ignore-submodules=none",
        ]);
        if action == Action::Unstage {
            cmd.arg("--cached");
            if !initial {
                cmd.arg("HEAD");
            }
        }
        cmd.args(["--", path]);
        let (out, exit) = read_only::run(
            &mut cmd,
            cancel,
            deadline,
            128 * 1024,
            "Index action preview",
            "Change preview exceeds 128 KiB. Nothing will be staged or unstaged.",
        )?;
        if !exit.success() {
            return Err("Cannot inspect exact change preview.".into());
        }
        out
    };
    let text = String::from_utf8(bytes)
        .map_err(|_| "Change preview is not UTF-8. Index action refused.")?;
    if status != "??"
        && text
            .lines()
            .any(|line| line.starts_with("Binary files ") || line == "GIT binary patch")
    {
        return Err("Binary staging preview is unsupported. Index action refused.".into());
    }
    if text.lines().count() > 1024
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err("Change preview exceeds 1,024 lines or contains unsupported controls. Index action refused.".into());
    }
    Ok(text)
}
fn literal(cmd: &mut std::process::Command) {
    cmd.env("GIT_LITERAL_PATHSPECS", "1")
        .env_remove("GIT_GLOB_PATHSPECS")
        .env_remove("GIT_NOGLOB_PATHSPECS")
        .env_remove("GIT_ICASE_PATHSPECS");
}
struct OwnedPath(PathBuf);
impl Drop for OwnedPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
pub(crate) fn apply(snapshot: Snapshot, cancel: &AtomicBool) -> Result<(), String> {
    let deadline = Instant::now() + DEADLINE;
    let mut fresh = prepare_at(
        &snapshot.root,
        &snapshot.path,
        &snapshot.status,
        snapshot.action,
        cancel,
        deadline,
    )?;
    fresh.hunk = snapshot.hunk;
    fresh.lines = snapshot.lines.clone();
    if snapshot.lines.active && (snapshot.hunk.0.is_some() || !snapshot.lines.any()) {
        return Err("Select at least one valid changed line; empty line intent never stages the whole file.".into());
    }
    let hunk_patch = if snapshot.hunk.0.is_some() {
        Some(hunks::extract_selection(&snapshot).ok_or("Select at least one supported hunk.")?)
    } else if snapshot.lines.active {
        Some(
            hunks::lines::synthesize(&snapshot)
                .ok_or("Selected lines do not form a valid patch; adjust selection.")?,
        )
    } else {
        None
    };
    if fresh != snapshot {
        return Err(
            "File, HEAD or index changed since confirmation. Nothing staged; inspect again.".into(),
        );
    }
    if snapshot.action == Action::Discard {
        return discard::apply(snapshot, cancel, deadline);
    }
    // Cooperate with Git's lock protocol; never overwrite another writer's lock.
    let lock_path = snapshot.index_path.with_file_name("index.lock");
    let mut lock = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|_| "Git index is locked. No changes made; retry explicitly.")?;
    let lock_guard = OwnedPath(lock_path.clone());
    if inspect_index(&snapshot.index_path, snapshot.index.is_empty())? != snapshot.index {
        return Err("Index changed before lock acquisition.".into());
    }
    let temp = snapshot
        .index_path
        .with_file_name(format!("pui-index-{}", uuid::Uuid::new_v4()));
    let temp_guard = OwnedPath(temp.clone());
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut private = options
        .open(&temp)
        .map_err(|_| "Cannot create private index.")?;
    private
        .write_all(&snapshot.index)
        .map_err(|_| "Cannot copy private index.")?;
    drop(private);
    if snapshot.index.is_empty() {
        fs::remove_file(&temp).map_err(|_| "Cannot initialize private empty index")?;
    }
    let mut cmd = read_only::command(&snapshot.root);
    literal(&mut cmd);
    cmd.args([
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.splitIndex=false",
    ]);
    cmd.env("GIT_INDEX_FILE", &temp)
        .env("GIT_NO_REPLACE_OBJECTS", "1");
    let patch_path = temp.with_file_name(format!("pui-patch-{}", uuid::Uuid::new_v4()));
    let mut patch_guard = None;
    if let Some(patch) = hunk_patch {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&patch_path)
            .map_err(|_| "Cannot prepare selected hunk.")?;
        patch_guard = Some(OwnedPath(patch_path.clone()));
        file.write_all(patch.as_bytes())
            .map_err(|_| "Cannot write selected hunk.")?;
        drop(file);
        if snapshot.lines.any() {
            // Dry-run against the private index: refuse if the selection cannot
            // apply cleanly. Never risk corrupting the real index.
            let mut check = read_only::command(&snapshot.root);
            literal(&mut check);
            check.args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.splitIndex=false",
            ]);
            check
                .env("GIT_INDEX_FILE", &temp)
                .env("GIT_NO_REPLACE_OBJECTS", "1");
            if snapshot.action == Action::Unstage {
                check.arg("apply").arg("--reverse");
            } else {
                check.arg("apply");
            }
            check
                .args([
                    "--cached",
                    "--recount",
                    "--whitespace=nowarn",
                    "--check",
                    "--",
                ])
                .arg(&patch_path);
            let (_, check_status) = read_only::run(
                &mut check,
                cancel,
                deadline,
                1024,
                "Line selection check",
                "Unexpected line check output.",
            )?;
            if !check_status.success() {
                return Err(
                    "Selected lines do not apply cleanly to the current index. Nothing staged; adjust the selection.".into(),
                );
            }
        }
        if snapshot.action == Action::Unstage {
            cmd.arg("apply").arg("--reverse");
        } else {
            cmd.arg("apply");
        }
        cmd.args(["--cached", "--recount", "--whitespace=nowarn", "--"])
            .arg(&patch_path);
    } else {
        match snapshot.action {
            Action::Stage => {
                cmd.args(["add", "--", &snapshot.path]);
            }
            Action::Unstage => {
                if snapshot.head.bytes().all(|b| b == b'0') {
                    cmd.args(["update-index", "--force-remove", "--", &snapshot.path]);
                } else {
                    cmd.args(["reset", "--quiet", &snapshot.head, "--", &snapshot.path]);
                }
            }
            Action::Discard => unreachable!(),
        }
    }
    let result = read_only::run(
        &mut cmd,
        cancel,
        deadline,
        1024,
        "Private index update",
        "Unexpected index action output.",
    );
    let _private_lock = OwnedPath(temp.with_file_name(format!(
        "{}.lock",
        temp.file_name().unwrap().to_string_lossy()
    )));
    let (_, exit) = result?;
    if !exit.success() {
        return Err("Private index update failed. Original index unchanged.".into());
    }
    drop(patch_guard);
    let mut after = prepare_at(
        &snapshot.root,
        &snapshot.path,
        &snapshot.status,
        snapshot.action,
        cancel,
        deadline,
    )?;
    after.hunk = snapshot.hunk;
    after.lines = snapshot.lines.clone();
    if after != snapshot {
        return Err(
            "File, HEAD or index changed during preparation. Original index unchanged.".into(),
        );
    }
    let updated = bytes(&temp, MAX_INDEX)?;
    lock.write_all(&updated)
        .map_err(|_| "Cannot write index lock.")?;
    lock.sync_all().map_err(|_| "Cannot sync index lock.")?;
    drop(lock);
    if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Err("Index action cancelled before publication.".into());
    }
    fs::set_permissions(
        &lock_path,
        if snapshot.index.is_empty() {
            fs::metadata(&temp)
                .map_err(|_| "Cannot inspect new index permissions")?
                .permissions()
        } else {
            fs::metadata(&snapshot.index_path)
                .map_err(|_| "Cannot inspect index permissions.")?
                .permissions()
        },
    )
    .map_err(|_| "Cannot preserve index permissions.")?;
    fs::rename(&lock_path, &snapshot.index_path).map_err(|_| "Cannot publish prepared index.")?;
    drop(lock_guard);
    drop(temp_guard);
    Ok(())
}
/// Whether a retained recovery copy exists for a file snapshot.
#[cfg(test)]
pub(crate) fn recovery_available(snapshot: &Snapshot) -> bool {
    snapshot.recovery_path().try_exists().unwrap_or(false)
}
/// Compatibility helper for disposable backend tests; UI uses a frozen review.
#[cfg(test)]
pub(crate) fn restore(
    root: &Path,
    path: &str,
    recovery: &Path,
    cancel: &AtomicBool,
) -> Result<(), String> {
    recovery::apply(recovery::inspect(root, path, recovery, cancel)?, cancel)
}
pub(crate) mod conflict_resolution;
mod discard;
pub(crate) mod hunks;
pub(crate) mod recovery;
#[cfg(test)]
mod tests;
