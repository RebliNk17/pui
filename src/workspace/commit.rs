//! Explicit local staged commit, frozen index and compare-and-swap branch publication.
use super::*;
mod diagnostics;
use std::{fs, io::Read};
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub root: PathBuf,
    pub head: String,
    pub branch: String,
    pub preview: String,
    pub files: Vec<history::files::Entry>,
    pub author: Identity,
    pub committer: Identity,
    signing_config: Vec<u8>,
    index_path: PathBuf,
    index: Vec<u8>,
}
fn run(
    root: &Path,
    args: &[&str],
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<u8>, String> {
    let (out, status) = read_only::run(
        read_only::command(root)
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .args(args),
        cancel,
        deadline,
        128 * 1024,
        "Staged commit",
        "Staged commit output exceeds 128 KiB.",
    )?;
    if !status.success() {
        return Err(
            "Git commit inspection failed; inspect repository and identity configuration.".into(),
        );
    }
    Ok(out)
}
fn line(bytes: Vec<u8>) -> Result<String, String> {
    let s = String::from_utf8(bytes).map_err(|_| "Invalid Git response")?;
    let s = s.strip_suffix('\n').unwrap_or(&s);
    if s.is_empty() || s.len() > 8192 || s.chars().any(char::is_control) {
        return Err("Invalid Git response".into());
    }
    Ok(s.into())
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Identity {
    pub name: String,
    pub email: String,
}
fn identity(bytes: Vec<u8>) -> Result<Identity, String> {
    let value = line(bytes)?;
    let (prefix, _date) = value.rsplit_once("> ").ok_or("Invalid Git identity")?;
    let (name, email) = prefix.rsplit_once(" <").ok_or("Invalid Git identity")?;
    if name.is_empty()
        || email.is_empty()
        || name.len() > 512
        || email.len() > 512
        || name.contains(['<', '>'])
        || email.contains(['<', '>'])
    {
        return Err("Git identity must have bounded name and email.".into());
    }
    Ok(Identity {
        name: name.into(),
        email: email.into(),
    })
}
fn index_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let m = fs::symlink_metadata(path).map_err(|_| "Index unavailable")?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() > 16 * 1024 * 1024 {
        return Err("Requires ordinary index <=16 MiB.".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "Index unavailable")?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Index read failed")?;
    if bytes.len() > 16 * 1024 * 1024 || !bytes.starts_with(b"DIRC") {
        return Err("Invalid or oversized index.".into());
    }
    Ok(bytes)
}
pub(super) fn branch_head(
    root: &Path,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<(String, String), String> {
    let (out, status) = read_only::run(
        read_only::command(root).args(["symbolic-ref", "-q", "HEAD"]),
        cancel,
        deadline,
        1024,
        "Current branch",
        "Branch exceeds limit",
    )?;
    let branch = if status.success() {
        let branch = line(out)?;
        if !branch.starts_with("refs/heads/") || branch.len() > 1024 {
            return Err("Invalid local branch".into());
        }
        branch
    } else if status.code() == Some(1) && out.is_empty() {
        String::new()
    } else {
        return Err("Current branch unavailable".into());
    };
    let (out, status) = read_only::run(
        read_only::command(root)
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .args(["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]),
        cancel,
        deadline,
        1024,
        "Commit HEAD",
        "Invalid HEAD response",
    )?;
    let head = if status.success() {
        let head = line(out)?;
        if !history::oid(&head) {
            return Err("Invalid HEAD".into());
        }
        head
    } else {
        if branch.is_empty() {
            return Err("Missing HEAD without a local unborn branch".into());
        }
        let (_, exists) = read_only::run(
            read_only::command(root).args(["show-ref", "--verify", "--quiet", &branch]),
            cancel,
            deadline,
            1024,
            "Unborn branch inspection",
            "Invalid branch response",
        )?;
        if status.code() != Some(1) || exists.code() != Some(1) {
            return Err("HEAD is unavailable or invalid, not an unborn local branch.".into());
        }
        match line(run(
            root,
            &["rev-parse", "--show-object-format"],
            cancel,
            deadline,
        )?)?
        .as_str()
        {
            "sha1" => "0".repeat(40),
            "sha256" => "0".repeat(64),
            _ => return Err("Unsupported object format".into()),
        }
    };
    Ok((branch, head))
}
pub(crate) fn prepare(root: &Path, cancel: &AtomicBool) -> Result<Snapshot, String> {
    let deadline = Instant::now() + DEADLINE;
    read_only::filters_disabled(root, cancel, deadline)?;
    if !root.is_absolute()
        || Path::new(&line(run(
            root,
            &["rev-parse", "--show-toplevel"],
            cancel,
            deadline,
        )?)?)
            != root
    {
        return Err("Exact repository root required.".into());
    }
    let (branch, head) = branch_head(root, cancel, deadline)?;
    if branch.is_empty() {
        return Err("Commit requires a local branch, not detached HEAD.".into());
    }
    for name in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
    ] {
        let path = PathBuf::from(line(run(
            root,
            &["rev-parse", "--path-format=absolute", "--git-path", name],
            cancel,
            deadline,
        )?)?);
        if path.exists() {
            return Err("Finish the merge/rebase/cherry-pick/revert first.".into());
        }
    }
    let index_path = PathBuf::from(line(run(
        root,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        cancel,
        deadline,
    )?)?);
    let index = index_bytes(&index_path)?;
    if !run(
        root,
        &["rev-parse", "--shared-index-path"],
        cancel,
        deadline,
    )?
    .is_empty()
    {
        return Err("Split index unsupported.".into());
    }
    if !run(root, &["ls-files", "--unmerged", "-z"], cancel, deadline)?.is_empty() {
        return Err("Resolve index conflicts first.".into());
    }
    let initial = head.bytes().all(|b| b == b'0');
    let mut file_args = vec![
        "diff",
        "--cached",
        "--name-status",
        "-z",
        "--no-renames",
        "--no-ext-diff",
        "--no-textconv",
    ];
    let mut patch_args = vec![
        "diff",
        "--cached",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "--no-renames",
        "--src-prefix=a/",
        "--dst-prefix=b/",
    ];
    if !initial {
        file_args.push(&head);
        patch_args.push(&head);
    }
    file_args.push("--");
    patch_args.push("--");
    let files = history::files::parse(&run(root, &file_args, cancel, deadline)?)?;
    let bytes = run(root, &patch_args, cancel, deadline)?;
    let preview = String::from_utf8(bytes).map_err(|_| "Staged patch is not UTF-8")?;
    if preview.is_empty() {
        return Err("No staged changes to commit.".into());
    }
    if preview.lines().count() > 1024
        || preview
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err("Staged preview exceeds 1024 lines or contains unsupported controls.".into());
    }
    if index_bytes(&index_path)? != index {
        return Err("Index changed during staged review; inspect again.".into());
    }
    let author = identity(run(root, &["var", "GIT_AUTHOR_IDENT"], cancel, deadline)?)?;
    let committer = identity(run(
        root,
        &["var", "GIT_COMMITTER_IDENT"],
        cancel,
        deadline,
    )?)?;
    let (signing_config, signing_status) = read_only::run(
        read_only::command(root).args([
            "config",
            "--includes",
            "--null",
            "--get-regexp",
            "^(gpg\\.|user\\.signingkey$)",
        ]),
        cancel,
        deadline,
        16384,
        "Signing configuration",
        "Signing configuration exceeds 16 KiB",
    )?;
    if !(signing_status.success() || signing_status.code() == Some(1) && signing_config.is_empty())
    {
        return Err("Cannot inspect signing configuration".into());
    }
    Ok(Snapshot {
        signing_config,
        author,
        committer,
        files,
        root: root.into(),
        branch,
        head,
        preview,
        index_path,
        index,
    })
}
pub(crate) fn valid_message(message: &str) -> bool {
    !message.trim().is_empty()
        && message.len() <= 4096
        && !message
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
}
pub(crate) fn apply(
    snapshot: Snapshot,
    message: &str,
    cancel: &AtomicBool,
) -> Result<String, String> {
    apply_with_signing(snapshot, message, false, cancel)
}
pub(crate) fn apply_with_signing(
    snapshot: Snapshot,
    message: &str,
    sign: bool,
    cancel: &AtomicBool,
) -> Result<String, String> {
    apply_options(snapshot, message, sign, false, cancel)
}
pub(crate) fn apply_options(
    snapshot: Snapshot,
    message: &str,
    sign: bool,
    pre_commit: bool,
    cancel: &AtomicBool,
) -> Result<String, String> {
    apply_checks(snapshot, message, sign, pre_commit, false, cancel)
}
pub(crate) fn apply_checks(
    snapshot: Snapshot,
    message: &str,
    sign: bool,
    pre_commit: bool,
    commit_msg: bool,
    cancel: &AtomicBool,
) -> Result<String, String> {
    apply_timed_checks(snapshot, message, sign, pre_commit, commit_msg, 30, cancel)
}
pub(crate) fn apply_timed_checks(
    snapshot: Snapshot,
    message: &str,
    sign: bool,
    pre_commit: bool,
    commit_msg: bool,
    hook_timeout: u64,
    cancel: &AtomicBool,
) -> Result<String, String> {
    if !(1..=300).contains(&hook_timeout) {
        return Err("Hook timeout must be 1–300 seconds.".into());
    }
    if !valid_message(message) {
        return Err(
            "Commit message requires 1–4096 UTF-8 bytes without unsupported controls.".into(),
        );
    }
    let fresh = prepare(&snapshot.root, cancel)?;
    if fresh != snapshot {
        return Err(
            "Staged snapshot, branch or author/committer identity changed; inspect again.".into(),
        );
    }
    let lock_path = snapshot.index_path.with_file_name("index.lock");
    let _lock = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|_| "Index is locked; nothing committed.")?;
    struct Lock(PathBuf);
    impl Drop for Lock {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let _cleanup = Lock(lock_path);
    if index_bytes(&snapshot.index_path)? != snapshot.index {
        return Err("Index changed; inspect again.".into());
    }
    // Hook waits do not consume the later short Git-publication budget.
    // write-tree uses a private exact index, never updates the original index.
    let private = snapshot
        .index_path
        .with_file_name(format!("pui-commit-index-{}", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    let mut file = options
        .open(&private)
        .map_err(|_| "Cannot create private index")?;
    let _private_cleanup = Lock(private.clone());
    file.write_all(&snapshot.index)
        .map_err(|_| "Cannot write private index")?;
    drop(file);
    if pre_commit {
        let mut hook = read_only::command(&snapshot.root);
        hook.env("GIT_INDEX_FILE", &private)
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .args(["hook", "run", "--ignore-missing", "pre-commit"]);
        diagnostics::run(
            &mut hook,
            cancel,
            Instant::now() + std::time::Duration::from_secs(hook_timeout),
            "Pre-commit check",
        )?;
        if index_bytes(&private)? != snapshot.index || prepare(&snapshot.root, cancel)? != snapshot
        {
            return Err("Pre-commit check changed the reviewed index or source state; nothing committed. Inspect changes and review again.".into());
        }
    }
    if commit_msg {
        let message_path = snapshot
            .index_path
            .with_file_name(format!("pui-commit-message-{}", uuid::Uuid::new_v4()));
        let mut message_file = options
            .open(&message_path)
            .map_err(|_| "Cannot create private commit message")?;
        let _message_cleanup = Lock(message_path.clone());
        let confirmed = message.trim().as_bytes();
        message_file
            .write_all(confirmed)
            .map_err(|_| "Cannot write private commit message")?;
        drop(message_file);
        let mut hook = read_only::command(&snapshot.root);
        hook.env("GIT_INDEX_FILE", &private)
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .args(["hook", "run", "--ignore-missing", "commit-msg", "--"])
            .arg(&message_path);
        diagnostics::run(
            &mut hook,
            cancel,
            Instant::now() + std::time::Duration::from_secs(hook_timeout),
            "Commit-message check",
        )?;
        let meta = fs::symlink_metadata(&message_path)
            .map_err(|_| "Commit-message hook removed message; nothing committed")?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() != confirmed.len() as u64
        {
            return Err("Commit-message hook rewrote the confirmed message; nothing committed. Review the message again.".into());
        }
        let mut actual = Vec::new();
        fs::File::open(&message_path)
            .map_err(|_| "Cannot read checked message")?
            .take(4097)
            .read_to_end(&mut actual)
            .map_err(|_| "Cannot read checked message")?;
        if actual != confirmed
            || index_bytes(&private)? != snapshot.index
            || prepare(&snapshot.root, cancel)? != snapshot
        {
            return Err("Commit-message check changed the reviewed message or snapshot; nothing committed. Review again.".into());
        }
    }
    let deadline = Instant::now() + DEADLINE;
    let mut cmd = read_only::command(&snapshot.root);
    cmd.env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_INDEX_FILE", &private)
        .args(["-c", "core.splitIndex=false", "write-tree"]);
    let (out, status) = read_only::run(
        &mut cmd,
        cancel,
        deadline,
        1024,
        "Commit tree",
        "Invalid commit tree",
    )?;
    if !status.success() {
        return Err("Cannot create staged tree.".into());
    }
    let tree = line(out)?;
    // Signing explicitly requested; failure never falls back to unsigned. Hooks remain disabled.
    let mut command = read_only::command(&snapshot.root);
    command
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_AUTHOR_NAME", &snapshot.author.name)
        .env("GIT_AUTHOR_EMAIL", &snapshot.author.email)
        .env("GIT_COMMITTER_NAME", &snapshot.committer.name)
        .env("GIT_COMMITTER_EMAIL", &snapshot.committer.email)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgSign=false",
            "commit-tree",
            &tree,
        ]);
    if !snapshot.head.bytes().all(|b| b == b'0') {
        command.args(["-p", &snapshot.head]);
    }
    if sign {
        command.arg("-S");
    }
    command.args(["-m", message.trim()]);
    let (out, status) = read_only::run(
        &mut command,
        cancel,
        deadline,
        1024,
        "Create commit object",
        "Commit response exceeds limit",
    )?;
    if !status.success() {
        return Err(if sign{"Signing failed or signer unavailable; no unsigned fallback or branch publication. Configure an available Git signer/key."}else{"Cannot create commit object with reviewed identity."}.into());
    }
    let oid = line(out)?;
    if !history::oid(&oid) {
        return Err("Invalid commit object".into());
    }
    if index_bytes(&snapshot.index_path)? != snapshot.index
        || line(run(
            &snapshot.root,
            &["symbolic-ref", "-q", "HEAD"],
            cancel,
            deadline,
        )?)? != snapshot.branch
    {
        return Err("Index or branch changed; unattached commit object may remain.".into());
    }
    run(&snapshot.root,&["-c","core.hooksPath=/dev/null","update-ref","-m","Pui: commit staged snapshot",&snapshot.branch,&oid,&snapshot.head],cancel,deadline).map_err(|_|"Branch update failed or completion uncertain; inspect History before retrying. An unattached object may remain.")?;
    Ok(oid)
}
#[cfg(test)]
mod tests;
