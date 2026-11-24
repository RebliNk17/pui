//! Ordinary text conflict editing/staging with separate frozen reviews. No continue/commit.
use super::*;
const MAX_TEXT: usize = 64 * 1024;
type Operation = Vec<(String, Option<Vec<u8>>)>;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Review {
    pub root: PathBuf,
    pub path: String,
    pub text: String,
    pub mode: String,
    index_path: PathBuf,
    index: Vec<u8>,
    branch: String,
    head: String,
    operation: Operation,
    permissions: u32,
}
fn check_text(text: &str) -> Result<(), String> {
    if text.len() > MAX_TEXT
        || text.lines().count() > 2048
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(
            "Resolution exceeds 64 KiB/2048 lines or contains unsupported controls.".into(),
        );
    }
    Ok(())
}
pub(crate) fn valid_resolution(text: &str) -> bool {
    check_text(text).is_ok()
        && !text.lines().any(|line| {
            let line = line.trim_start();
            ["<<<<<<<", "|||||||", "=======", ">>>>>>>"]
                .iter()
                .any(|marker| line.starts_with(marker))
        })
}
fn input_path(root: &Path, path: &str) -> Result<PathBuf, String> {
    if !valid_diff_path(path)
        || path
            .split('/')
            .any(|s| s.is_empty() || matches!(s, "." | ".."))
        || !root.is_absolute()
        || fs::canonicalize(root).map_err(|_| "Root unavailable")? != root
    {
        return Err("Exact canonical root and literal resolution path required.".into());
    }
    let full = root.join(path);
    let parent = full.parent().ok_or("File parent unavailable")?;
    if fs::canonicalize(parent).map_err(|_| "File parent unavailable")? != parent {
        return Err("Symlinked parents refuse conflict resolution.".into());
    }
    Ok(full)
}
fn mode(path: &Path) -> Result<String, String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "Working file unavailable")?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(
            "Existing ordinary working file required; deletion/type resolution unsupported.".into(),
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(if meta.permissions().mode() & 0o111 != 0 {
            "100755"
        } else {
            "100644"
        }
        .into())
    }
    #[cfg(not(unix))]
    {
        Err("Conflict writes require Unix file modes.".into())
    }
}
fn operation(dir: &Path) -> Result<Operation, String> {
    // Freeze the Git operation control files, including detached rebase progress.
    let mut files = Vec::new();
    for name in [
        "MERGE_HEAD",
        "MERGE_MSG",
        "AUTO_MERGE",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "REBASE_HEAD",
        "ORIG_HEAD",
        "rebase-merge/head-name",
        "rebase-merge/onto",
        "rebase-merge/orig-head",
        "rebase-merge/msgnum",
        "rebase-merge/end",
        "rebase-merge/stopped-sha",
        "rebase-apply/head-name",
        "rebase-apply/onto",
        "rebase-apply/orig-head",
        "rebase-apply/next",
        "rebase-apply/last",
        "rebase-apply/original-commit",
        "sequencer/head",
        "sequencer/todo",
        "sequencer/abort-safety",
    ] {
        let path = dir.join(name);
        if let Some(parent) = path.parent()
            && parent != dir
            && let Ok(meta) = fs::symlink_metadata(parent)
            && (!meta.is_dir() || meta.file_type().is_symlink())
        {
            return Err("Unsupported operation metadata directory.".into());
        }
        let data = match fs::symlink_metadata(&path) {
            Ok(_) => Some(bytes(&path, 16 * 1024)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err("Operation state unavailable.".into()),
        };
        files.push((name.into(), data));
    }
    Ok(files)
}
pub(crate) fn inspect(root: &Path, path: &str, cancel: &AtomicBool) -> Result<Review, String> {
    inspect_at(root, path, cancel, Instant::now() + DEADLINE)
}
fn inspect_at(
    root: &Path,
    path: &str,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Review, String> {
    let full = input_path(root, path)?;
    let paths = String::from_utf8(run(
        root,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-path",
            "index",
            "--absolute-git-dir",
            "--shared-index-path",
        ],
        cancel,
        deadline,
    )?)
    .map_err(|_| "Invalid Git paths.")?;
    let mut path_lines = paths.lines();
    let actual_root = path_lines.next().ok_or("Missing repository root.")?;
    let index_raw = path_lines.next().ok_or("Missing index path.")?;
    let git_dir_raw = path_lines.next().ok_or("Missing git directory.")?;
    let shared_raw = path_lines.next();
    if path_lines.next().is_some()
        || actual_root.is_empty()
        || actual_root.len() > 8192
        || actual_root.chars().any(char::is_control)
        || index_raw.is_empty()
        || index_raw.len() > 8192
        || index_raw.chars().any(char::is_control)
        || git_dir_raw.is_empty()
        || git_dir_raw.len() > 8192
        || git_dir_raw.chars().any(char::is_control)
    {
        return Err("Invalid Git path inventory.".into());
    }
    if Path::new(actual_root) != root {
        return Err("Exact repository root required.".into());
    }
    let index_path = PathBuf::from(index_raw);
    let git_dir = PathBuf::from(git_dir_raw);
    let index = bytes(&index_path, MAX_INDEX)?;
    let conflict = super::super::conflict::inspect_at(root, path, cancel, deadline, true)?;
    let mode = mode(&full)?;
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        fs::symlink_metadata(&full)
            .map_err(|_| "Working file unavailable")?
            .permissions()
            .mode()
    };
    #[cfg(not(unix))]
    let permissions = 0;
    if conflict.sides[1].is_none()
        || conflict.sides[2].is_none()
        || conflict.sides.iter().flatten().any(|s| s.mode != mode)
    {
        return Err("Only both-present ordinary text conflicts with unchanged file mode are supported; deletion/type/mode conflicts refuse.".into());
    }
    // No implicit encoding/filter/EOL conversion: stage exactly the displayed bytes.
    if !run(
        root,
        &[
            "--literal-pathspecs",
            "check-attr",
            "-z",
            "--all",
            "--",
            path,
        ],
        cancel,
        deadline,
    )?
    .is_empty()
    {
        return Err("File attributes refuse raw resolution staging.".into());
    }
    let (out, status) = read_only::run(
        read_only::command(root).args([
            "config",
            "--includes",
            "--null",
            "--name-only",
            "--get-regexp",
            "^(filter\\..*\\.(clean|process)|core\\.(autocrlf|eol|sparsecheckout|sparsecheckoutcone))$",
        ]),
        cancel,
        deadline,
        8192,
        "Resolution configuration",
        "Resolution configuration exceeds limit",
    )?;
    if status.code() != Some(1) || !out.is_empty() {
        return Err(
            "Filter or line-ending/sparse-checkout configuration refuses resolution writes.".into(),
        );
    }
    if shared_raw.is_some_and(|s| !s.is_empty()) {
        return Err("Split index resolution unsupported.".into());
    }
    let (branch, head) = super::super::commit::branch_head(root, cancel, deadline)?;
    let text = String::from_utf8(bytes(&full, MAX_TEXT)?)
        .map_err(|_| "Resolution requires UTF-8 text.")?;
    check_text(&text)?;
    let operation = operation(&git_dir)?;
    if bytes(&index_path, MAX_INDEX)? != index {
        return Err("Index changed during resolution inspection; review again.".into());
    }
    if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Err("Resolution inspection cancelled/timed out.".into());
    }
    Ok(Review {
        root: root.into(),
        path: path.into(),
        text,
        mode,
        index_path,
        index,
        branch,
        head,
        operation,
        permissions,
    })
}
fn validate(review: &Review, cancel: &AtomicBool, deadline: Instant) -> Result<(), String> {
    if inspect_at(&review.root, &review.path, cancel, deadline)? != *review {
        return Err(
            "Conflict, working file, index, HEAD or operation changed; review again.".into(),
        );
    }
    Ok(())
}
fn private_file(path: &Path, content: &[u8]) -> Result<(), String> {
    let mut opts = OpenOptions::new();
    opts.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(path)
        .map_err(|_| "Cannot create private resolution file")?;
    f.write_all(content)
        .and_then(|_| f.sync_all())
        .map_err(|_| "Cannot sync private resolution file".into())
}
fn lock(review: &Review) -> Result<(fs::File, OwnedPath), String> {
    let path = review.index_path.with_file_name("index.lock");
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let f = opts
        .open(&path)
        .map_err(|_| "Git index locked; nothing changed")?;
    Ok((f, OwnedPath(path)))
}
/// Save working text only. Preserve old content beside the actual worktree index.
pub(crate) fn save(review: Review, draft: String, cancel: &AtomicBool) -> Result<PathBuf, String> {
    if !valid_resolution(&draft) {
        return Err("Remove all conflict-marker lines and keep resolution within text bounds before saving.".into());
    }
    let deadline = Instant::now() + DEADLINE;
    let (_lock, _guard) = lock(&review)?;
    validate(&review, cancel, deadline)?;
    let mut digest = Sha256::new();
    digest.update(review.path.as_bytes());
    digest.update(Sha256::digest(review.text.as_bytes()));
    let backup = review.index_path.with_file_name(format!(
        "pui-discard-recovery-{}",
        crate::hex::lower(digest.finalize())
    ));
    if backup.try_exists().map_err(|_| "Recovery unavailable")? {
        if bytes(&backup, MAX_TEXT)? != review.text.as_bytes() {
            return Err("Recovery collision; nothing saved.".into());
        }
    } else {
        private_file(&backup, review.text.as_bytes())?;
    }
    let full = input_path(&review.root, &review.path)?;
    let temp = full.with_file_name(format!(".pui-resolution-{}", uuid::Uuid::new_v4()));
    private_file(&temp, draft.as_bytes())?;
    let _temp = OwnedPath(temp.clone());
    fs::set_permissions(
        &temp,
        fs::metadata(&full)
            .map_err(|_| "Working file unavailable")?
            .permissions(),
    )
    .map_err(|_| "Cannot preserve file permissions")?;
    validate(&review, cancel, deadline)?;
    fs::rename(&temp, &full).map_err(|_| "Cannot save resolution; backup retained")?;
    Ok(backup)
}
/// Mark this exact working text resolved by private-index publication only.
pub(crate) fn stage(review: Review, cancel: &AtomicBool) -> Result<(), String> {
    if !valid_resolution(&review.text) {
        return Err(
            "Conflict-marker lines remain; edit the working file before marking resolved.".into(),
        );
    }
    let deadline = Instant::now() + DEADLINE;
    let (mut lock, guard) = lock(&review)?;
    validate(&review, cancel, deadline)?;
    let temp = review
        .index_path
        .with_file_name(format!("pui-resolution-index-{}", uuid::Uuid::new_v4()));
    private_file(&temp, &review.index)?;
    let _temp = OwnedPath(temp.clone());
    let _temp_lock = OwnedPath(temp.with_file_name(format!(
        "{}.lock",
        temp.file_name().unwrap().to_string_lossy()
    )));
    let blob = temp.with_file_name(format!("pui-resolution-blob-{}", uuid::Uuid::new_v4()));
    private_file(&blob, review.text.as_bytes())?;
    let _blob = OwnedPath(blob.clone());
    let mut cmd = read_only::command(&review.root);
    cmd.args(["hash-object", "-w", "--no-filters", "--"])
        .arg(&blob);
    let (out, status) = read_only::run(
        &mut cmd,
        cancel,
        deadline,
        128,
        "Resolution object",
        "Invalid resolution object",
    )?;
    let oid = line(&out, 128, "object")?;
    if !status.success() || !super::super::history::oid(&oid) {
        return Err("Cannot retain resolution object; index unchanged.".into());
    }
    let (_, status) = read_only::run(
        read_only::command(&review.root)
            .env("GIT_INDEX_FILE", &temp)
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.splitIndex=false",
                "update-index",
                "--add",
                "--cacheinfo",
                &review.mode,
                &oid,
                &review.path,
            ]),
        cancel,
        deadline,
        1024,
        "Resolve private index",
        "Unexpected private index output",
    )?;
    if !status.success() {
        return Err("Cannot resolve private index; original index unchanged.".into());
    }
    let mut cmd = read_only::command(&review.root);
    literal(&mut cmd);
    let (out, status) = read_only::run(
        cmd.env("GIT_INDEX_FILE", &temp)
            .args(["ls-files", "--stage", "-z", "--", &review.path]),
        cancel,
        deadline,
        8192,
        "Resolved entry check",
        "Invalid resolved entry",
    )?;
    if !status.success()
        || out != format!("{} {} 0\t{}\0", review.mode, oid, review.path).as_bytes()
    {
        return Err("Resolved index entry mismatch; original index unchanged.".into());
    }
    validate(&review, cancel, deadline)?;
    let updated = bytes(&temp, MAX_INDEX)?;
    lock.write_all(&updated)
        .and_then(|_| lock.sync_all())
        .map_err(|_| "Cannot sync resolution index lock")?;
    fs::set_permissions(
        &guard.0,
        fs::metadata(&review.index_path)
            .map_err(|_| "Index unavailable")?
            .permissions(),
    )
    .map_err(|_| "Cannot preserve index permissions")?;
    drop(lock);
    if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Err("Resolution cancelled before index publication.".into());
    }
    fs::rename(&guard.0, &review.index_path).map_err(|_| "Cannot publish resolved index")?;
    Ok(())
}
#[cfg(test)]
mod tests;
