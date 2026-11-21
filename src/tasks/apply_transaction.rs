//! Recoverable, non-atomic file transaction. Each target is moved aside (never
//! overwritten), checked again, then new content is published exclusively. Pinned
//! no-follow parent descriptors are used for every mutation. Editors/Git are not
//! locked out; races may require manual recovery and are never reported as success.
use super::*;
#[path = "apply_directories.rs"]
mod directories;
pub(super) use directories::missing as missing_directories;
use std::io::Write;
#[cfg(unix)]
use std::{
    ffi::CString,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionStatus {
    Applied,
    RolledBack,
    RecoveryRequired,
    Interrupted,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApplyOutcome {
    pub transaction_id: String,
    pub status: TransactionStatus,
    pub detail: String,
    pub retained_at: PathBuf,
}
pub type RecoveryStatus = ApplyOutcome;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryAction {
    RollBack,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Operation {
    path: String,
    before: Option<FileState>,
    after: Option<FileState>,
    stage: String,
    backup: String,
    parent_dev: u64,
    parent_ino: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Journal {
    version: u32,
    task_id: String,
    transaction_id: String,
    identity: Identity,
    patch_hash: String,
    operations: Vec<Operation>,
    #[serde(default)]
    directories: Vec<String>,
}
struct Parent {
    directory: fs::File,
    recovery: Option<fs::File>,
    leaf: CString,
}
impl Parent {
    fn fd(&self, leaf: &CString) -> i32 {
        if leaf.as_bytes().starts_with(b".pui-apply-") {
            self.recovery
                .as_ref()
                .expect("Pinned recovery directory")
                .as_raw_fd()
        } else {
            self.directory.as_raw_fd()
        }
    }
    fn sync(&self) -> Result<(), String> {
        self.directory
            .sync_all()
            .and_then(|_| self.recovery.as_ref().map_or(Ok(()), fs::File::sync_all))
            .map_err(|e| e.to_string())
    }
}

#[cfg(unix)]
fn parent(root: &Path, path: &str) -> Result<Parent, String> {
    name(path.as_bytes())?;
    let mut directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)
        .map_err(|e| e.to_string())?;
    let components: Vec<_> = Path::new(path).iter().collect();
    for component in &components[..components.len() - 1] {
        let c = CString::new(component.as_bytes()).map_err(|_| "Invalid target path")?;
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                c.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err("Target parent is missing, symlinked, or inaccessible.".into());
        }
        directory = unsafe { fs::File::from_raw_fd(fd) };
    }
    Ok(Parent {
        directory,
        recovery: None,
        leaf: CString::new(components.last().unwrap().as_bytes())
            .map_err(|_| "Invalid target leaf")?,
    })
}
#[cfg(not(unix))]
fn parent(_root: &Path, _path: &str) -> Result<Parent, String> {
    Err("Recoverable apply is currently supported only on Unix.".into())
}
#[cfg(test)]
pub(super) fn parent_available(root: &Path, path: &str) -> bool {
    parent(root, path).is_ok()
}
#[cfg(unix)]
fn at_state(parent: &Parent, leaf: &CString) -> Result<Option<FileState>, String> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent.fd(leaf),
            leaf.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } < 0
    {
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error.to_string())
        };
    }
    if unsafe { stat.assume_init() }.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err("Target changed to a link, directory or special file.".into());
    }
    let fd = unsafe {
        libc::openat(
            parent.fd(leaf),
            leaf.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut file = unsafe { fs::File::from_raw_fd(fd) };
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Err("Invalid or oversized transaction file.".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(meta.len() + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let after = file.metadata().map_err(|e| e.to_string())?;
    if bytes.len() as u64 != meta.len() || meta.modified().ok() != after.modified().ok() {
        return Err("Concurrent file modification during transaction.".into());
    }
    Ok(Some(FileState {
        bytes,
        mode: meta.mode() & 0o777,
    }))
}
#[cfg(unix)]
fn token(parent: &Parent, leaf: &CString) -> Result<Option<(u64, u64)>, String> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent.fd(leaf),
            leaf.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } < 0
    {
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error.to_string())
        };
    }
    let s = unsafe { stat.assume_init() };
    if s.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err("Transaction path is no longer a regular file.".into());
    }
    Ok(Some((s.st_dev as u64, s.st_ino)))
}
fn c(value: &str) -> CString {
    CString::new(value).expect("Generated transaction path")
}
#[cfg(unix)]
fn parent_matches(root: &Path, op: &Operation, pinned: &Parent) -> Result<(), String> {
    let current = parent(root, &op.path)?;
    let a = current.directory.metadata().map_err(|e| e.to_string())?;
    let b = pinned.directory.metadata().map_err(|e| e.to_string())?;
    if a.dev() != op.parent_dev
        || a.ino() != op.parent_ino
        || a.dev() != b.dev()
        || a.ino() != b.ino()
    {
        return Err(
            "A target parent directory moved or was replaced. Recovery requires inspection.".into(),
        );
    }
    Ok(())
}
#[cfg(unix)]
fn move_exclusive(parent: &Parent, from: &CString, to: &CString) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let result = unsafe {
        libc::renameatx_np(
            parent.fd(from),
            from.as_ptr(),
            parent.fd(to),
            to.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            parent.fd(from),
            from.as_ptr(),
            parent.fd(to),
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let result = -1;
    if result < 0 {
        return Err(format!(
            "Exclusive target move failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    parent.sync()
}
#[cfg(unix)]
fn link_exclusive(parent: &Parent, from: &CString, to: &CString) -> Result<(), String> {
    if unsafe {
        libc::linkat(
            parent.fd(from),
            from.as_ptr(),
            parent.fd(to),
            to.as_ptr(),
            0,
        )
    } < 0
    {
        return Err(format!(
            "Exclusive publication failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    parent.sync()
}
#[cfg(unix)]
fn stage(parent: &Parent, name: &str, state: &FileState) -> Result<(), String> {
    let fd = unsafe {
        libc::openat(
            parent.fd(&c(name)),
            c(name).as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut file = unsafe { fs::File::from_raw_fd(fd) };
    file.write_all(&state.bytes)
        .and_then(|_| {
            file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(state.mode))
        })
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    parent.sync()
}
fn recovery_directory(identity: &Identity, id: &str, create: bool) -> Result<fs::File, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid transaction identity")?;
    let path = identity.original_git_dir.join("pui-apply-v1").join(id);
    if create {
        baseline::private_dir(&path)?;
        baseline::sync_dir(path.parent().unwrap())?;
    }
    baseline::open_directory(&path)
}
fn marker(directory: &Path, name: &str) -> Result<(), String> {
    baseline::durable_file(&directory.join(name), b"1")
}
fn outcome(directory: &Path, id: &str, status: TransactionStatus, detail: String) -> ApplyOutcome {
    ApplyOutcome {
        transaction_id: id.into(),
        status,
        detail,
        retained_at: directory.into(),
    }
}
fn events(directory: &Path) -> Result<Vec<(u64, String)>, String> {
    let mut found = Vec::new();
    for (count, entry) in fs::read_dir(directory)
        .map_err(|e| e.to_string())?
        .enumerate()
    {
        if count > 3 * MAX_FILES + 64 {
            return Err("Apply event directory exceeds its scan budget; inspect retained evidence manually.".into());
        }
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some(number) = name
            .strip_prefix("event-")
            .and_then(|s| s.strip_suffix(".json"))
        {
            let sequence = number
                .parse::<u64>()
                .map_err(|_| "Invalid apply event sequence")?;
            if format!("{sequence:020}") != number {
                return Err("Invalid apply event name".into());
            }
            found.push((sequence, name.into()));
        }
    }
    found.sort_by_key(|e| e.0);
    Ok(found)
}
fn append_event(result: &ApplyOutcome) -> Result<(), String> {
    let sequence = events(&result.retained_at)?
        .last()
        .map_or(Ok(0), |(n, _)| {
            n.checked_add(1).ok_or("Apply event sequence overflow")
        })?;
    let bytes = serde_json::to_vec(result).map_err(|e| e.to_string())?;
    baseline::durable_file(
        &result
            .retained_at
            .join(format!("event-{sequence:020}.json")),
        &bytes,
    )
}
#[cfg(test)]
pub(super) fn append_event_for_test(result: &ApplyOutcome) {
    append_event(result).unwrap();
}
fn finish(directory: &Path, id: &str, status: TransactionStatus, detail: String) -> ApplyOutcome {
    let result = outcome(directory, id, status, detail);
    match append_event(&result) {
        Ok(()) => result,
        Err(error) => outcome(
            directory,
            id,
            TransactionStatus::RecoveryRequired,
            format!(
                "{} Final receipt could not be synced: {error}. Treat outcome as uncertain; inspect retained evidence.",
                result.detail
            ),
        ),
    }
}
/// Validate the real retained task-lock file, not a boolean UI permission.
/// The owner must keep its exclusive fs2 lock for the entire operation.
#[cfg(unix)]
fn validate_claim(home: &Path, claim: &fs::File, task: &Task) -> Result<(), String> {
    uuid::Uuid::parse_str(&task.id).map_err(|_| "Invalid workflow ID")?;
    let expected = source::entry(home, Path::new(&format!("{}.lock", task.id)))?;
    let source::Entry::File(expected) = expected else {
        return Err("Missing workflow writer claim.".into());
    };
    let a = claim.metadata().map_err(|e| e.to_string())?;
    let b = expected.metadata().map_err(|e| e.to_string())?;
    if a.dev() != b.dev() || a.ino() != b.ino() {
        return Err("Writer claim does not belong to this workflow.".into());
    }
    // A second independent descriptor must conflict; otherwise this handle is not
    // actually locked. Merely pointing to the lock file is not writer authority.
    use fs2::FileExt;
    match expected.try_lock_exclusive() {
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            claim.try_lock_exclusive().map_err(|_| {
                "The supplied descriptor is not the owner of the workflow writer claim.".into()
            })
        }
        Ok(()) => {
            let _ = fs2::FileExt::unlock(&expected);
            Err("Workflow writer claim is not held exclusively.".into())
        }
        Err(e) => Err(format!("Cannot verify writer claim: {e}")),
    }
}
#[cfg(test)]
pub fn apply(home: &Path, expected: &Task, reviewed: &Plan) -> Result<ApplyOutcome, String> {
    let claim = super::super::store::lock(home, &expected.id)?;
    apply_owned(home, &claim, expected, reviewed)
}
/// Called ONLY after separate explicit Quiet Mac confirmation of reviewed.
/// No model calls, setup, command replay, commits, merges, stashes or index writes.
pub fn apply_owned(
    home: &Path,
    claim: &fs::File,
    expected: &Task,
    reviewed: &Plan,
) -> Result<ApplyOutcome, String> {
    validate_claim(home, claim, expected)?;
    let task = super::super::store::current_record(home, expected)?;
    check(&task, reviewed)?;
    if inspect_recovery(home, &task)?.iter().any(|r| {
        matches!(
            r.status,
            TransactionStatus::Interrupted | TransactionStatus::RecoveryRequired
        )
    }) {
        return Err(
            "An earlier apply transaction needs explicit recovery before another apply.".into(),
        );
    }
    transact(home, &task, reviewed, Fault::None)
}
#[derive(Clone, Copy)]
pub(super) enum Fault {
    None,
    #[cfg(test)]
    FailAfter(usize),
    #[cfg(test)]
    InterruptAfter(usize),
    #[cfg(test)]
    ExternalAfter(usize),
}
pub(super) fn transact(
    home: &Path,
    task: &Task,
    plan: &Plan,
    fault: Fault,
) -> Result<ApplyOutcome, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let directory = fs::canonicalize(home)
        .map_err(|e| e.to_string())?
        .join("apply-v1")
        .join(format!("transaction-{}-{id}", task.id));
    let missing = plan
        .changes
        .iter()
        .map(|change| directories::missing(&plan.identity.original, &change.path))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>();
    let mut missing = missing.into_iter().collect::<Vec<_>>();
    missing.sort_by_key(|path| (path.matches('/').count(), path.clone()));
    if missing.len() > MAX_FILES {
        return Err("Apply requires too many new directories; nothing changed.".into());
    }
    check(task, plan)?;
    baseline::private_dir(&directory)?;
    // Directory intent is durable before mkdir. A partial creation without a
    // matching receipt is explicitly uncertain and is never automatically removed.
    baseline::durable_file(
        &directory.join("directory-plan.json"),
        &serde_json::to_vec(&missing).map_err(|e| e.to_string())?,
    )?;
    baseline::sync_dir(directory.parent().unwrap())?;
    let created = directories::create(&plan.identity.original, &missing, &directory)?;
    let mut operations = Vec::new();
    let mut parents = Vec::new();
    // All paths/content remain checked before file publication.
    for (i, change) in plan.changes.iter().enumerate() {
        name(change.path.as_bytes())?;
        let p = parent(&plan.identity.original, &change.path)?;
        let before = change.before.as_ref().map(|s| FileState {
            bytes: s.as_bytes().into(),
            mode: change.before_mode.unwrap(),
        });
        let after = change.after.as_ref().map(|s| FileState {
            bytes: s.as_bytes().into(),
            mode: change.after_mode.unwrap(),
        });
        if at_state(&p, &p.leaf)? != before {
            return Err(format!("Target changed before apply: {}", change.path));
        }
        let m = p.directory.metadata().map_err(|e| e.to_string())?;
        operations.push(Operation {
            path: change.path.clone(),
            before,
            after,
            stage: format!(".pui-apply-{id}-{i}-new"),
            backup: format!(".pui-apply-{id}-{i}-old"),
            parent_dev: m.dev(),
            parent_ino: m.ino(),
        });
        parents.push(p);
    }
    check(task, plan)?;
    baseline::private_dir(&directory)?;
    let journal = Journal {
        version: 1,
        task_id: task.id.clone(),
        transaction_id: id.clone(),
        identity: plan.identity.clone(),
        patch_hash: plan.patch_hash.clone(),
        operations,
        directories: missing,
    };
    let retained = recovery_directory(&journal.identity, &id, true)?;
    for p in &mut parents {
        if p.directory.metadata().map_err(|e| e.to_string())?.dev()
            != retained.metadata().map_err(|e| e.to_string())?.dev()
        {
            return Err("Apply requires working files and Git recovery directory on the same filesystem. No source files changed.".into());
        }
        p.recovery = Some(retained.try_clone().map_err(|e| e.to_string())?);
    }
    // Entire before/after backups and intent are synced before source mutations.
    baseline::durable_file(
        &directory.join("journal.json"),
        &serde_json::to_vec(&journal).map_err(|e| e.to_string())?,
    )?;
    baseline::sync_dir(directory.parent().unwrap())?;
    let result = (|| {
        for (op, p) in journal.operations.iter().zip(&parents) {
            parent_matches(&journal.identity.original, op, p)?;
            if let Some(after) = &op.after {
                stage(p, &op.stage, after)?;
            }
        }
        // Ignored local retained artifacts are not payload. The original inventory
        // check was already complete before staging; verify raw indexes/HEAD and
        // candidate again, plus EVERY target, before beginning target writes.
        if identity(task)? != plan.identity
            || inventory(&plan.identity)? != plan.inventory
            || workspace::fingerprint(&plan.identity.candidate)?
                != plan.checked_candidate_fingerprint
            || capture(
                &plan.identity.candidate,
                &plan.identity.candidate_git_dir,
                &plan.paths,
            )?
            .1 != plan.candidate_fingerprint
            || capture(
                &plan.identity.original,
                &plan.identity.original_git_dir,
                &plan.paths,
            )?
            .1 != plan.original_fingerprint
        {
            return Err("Source/candidate/index changed before writes.".into());
        }
        for (op, p) in journal.operations.iter().zip(&parents) {
            parent_matches(&journal.identity.original, op, p)?;
            if at_state(p, &p.leaf)? != op.before {
                return Err(format!("Target changed before writes: {}", op.path));
            }
        }
        for (i, (op, p)) in journal.operations.iter().zip(&parents).enumerate() {
            parent_matches(&journal.identity.original, op, p)?;
            marker(&directory, &format!("intent-{i}"))?;
            if at_state(p, &p.leaf)? != op.before {
                return Err(format!("Target changed: {}", op.path));
            }
            if op.before.is_some() {
                move_exclusive(p, &p.leaf, &c(&op.backup))?;
                if at_state(p, &c(&op.backup))? != op.before {
                    return Err(format!("Target raced while moved aside: {}", op.path));
                }
            }
            if op.after.is_some() {
                link_exclusive(p, &c(&op.stage), &p.leaf)?;
            }
            marker(&directory, &format!("installed-{i}"))?;
            match fault {
                #[cfg(test)]
                Fault::FailAfter(n) if n == i + 1 => return Err("Injected write failure".into()),
                #[cfg(test)]
                Fault::InterruptAfter(n) if n == i + 1 => return Ok(false),
                #[cfg(test)]
                Fault::ExternalAfter(n) if n == i + 1 => {
                    fs::write(
                        journal.identity.original.join(&op.path),
                        "external concurrent edit\n",
                    )
                    .map_err(|e| e.to_string())?;
                    return Err("Injected failure with external edit".into());
                }
                _ => {}
            }
        }
        for (op, p) in journal.operations.iter().zip(&parents) {
            parent_matches(&journal.identity.original, op, p)?;
            if at_state(p, &p.leaf)? != op.after
                || (op.before.is_some() && at_state(p, &c(&op.backup))? != op.before)
            {
                return Err("Concurrent edit detected after publication.".into());
            }
        }
        if identity(task)? != plan.identity
            || workspace::fingerprint(&plan.identity.candidate)?
                != plan.checked_candidate_fingerprint
            || capture(
                &plan.identity.candidate,
                &plan.identity.candidate_git_dir,
                &plan.paths,
            )?
            .1 != plan.candidate_fingerprint
        {
            return Err("Candidate or repository identity changed during publication.".into());
        }
        Ok(true)
    })();
    match result {
        Ok(false)=>Ok(outcome(&directory,&id,TransactionStatus::Interrupted,"Injected interruption: transaction intentionally has no completion receipt; inspect and explicitly roll back, never replay.".into())),
        Ok(true)=>Ok(finish(&directory,&id,TransactionStatus::Applied,"Exact reviewed builder changes applied to working files. Git index and unrelated files were not written. Private Git-directory backups and durable journal permit explicit conditional rollback; no recovery artifacts were added to the source working tree. Filesystem multi-file writes were not atomic.".into())),
        Err(error)=>{let(complete,detail)=rollback(&directory,&journal,&parents);let clean=if complete{directories::cleanup(&journal.identity.original,&created)}else{Err("Directory cleanup awaits file recovery.".into())};Ok(finish(&directory,&id,if complete&&clean.is_ok(){TransactionStatus::RolledBack}else{TransactionStatus::RecoveryRequired},format!("Apply failed: {error}. {detail} {}",clean.err().unwrap_or_default())))}
    }
}
/// Roll back only bytes/inodes we can still attribute to this transaction. A
/// delete/replacement is first moved aside and rechecked; raced data is retained,
/// never overwritten. No recovery action installs candidate content.
fn rollback(directory: &Path, journal: &Journal, parents: &[Parent]) -> (bool, String) {
    let mut failures = Vec::new();
    for (i, (op, p)) in journal.operations.iter().zip(parents).enumerate().rev() {
        let result = (|| {
            parent_matches(&journal.identity.original, op, p)?;
            // No target mutation was intended yet; leave user data untouched.
            if !directory
                .join(format!("intent-{i}"))
                .try_exists()
                .map_err(|e| e.to_string())?
            {
                return Ok(());
            }
            let current = at_state(p, &p.leaf)?;
            let backup = at_state(p, &c(&op.backup))?;
            if backup.is_none() && current == op.before {
                return Ok(());
            }
            if let Some(after) = &op.after {
                if current.as_ref() == Some(after) && token(p, &p.leaf)? == token(p, &c(&op.stage))?
                {
                    let removed = c(&format!("{}-rollback", op.stage));
                    move_exclusive(p, &p.leaf, &removed)?;
                    if at_state(p, &removed)?.as_ref() != Some(after)
                        || token(p, &removed)? != token(p, &c(&op.stage))?
                    {
                        let _ = move_exclusive(p, &removed, &p.leaf);
                        return Err(
                            "Concurrent edit while rolling back; raced data retained".into()
                        );
                    }
                } else if current.is_some() {
                    return Err(
                        "External edit/replacement preserved; target not rolled back".into(),
                    );
                } else if directory
                    .join(format!("installed-{i}"))
                    .try_exists()
                    .map_err(|e| e.to_string())?
                {
                    return Err(
                        "Published target was externally deleted; deletion preserved".into(),
                    );
                }
            } else if current.is_some() {
                return Err("External file at deleted path preserved".into());
            }
            if let Some(before) = &op.before {
                if backup.as_ref() != Some(before) {
                    return Err(
                        "Retained original changed or missing; manual recovery required".into(),
                    );
                }
                move_exclusive(p, &c(&op.backup), &p.leaf)?;
                if at_state(p, &p.leaf)?.as_ref() != Some(before) {
                    return Err("Restored file changed concurrently".into());
                }
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            failures.push(format!("{}: {error}", op.path));
        }
    }
    if failures.is_empty() {
        (true,"Conditionally rolled back transaction-owned paths. Index was never written. Journal, original backups and staged bytes remain retained.".into())
    } else {
        (
            false,
            format!(
                "Not fully rolled back; external edits were not overwritten. Explicit recovery/manual inspection required: {}",
                failures.join("; ")
            ),
        )
    }
}
fn journal_path(home: &Path, task: &Task, id: &str) -> Result<PathBuf, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid transaction identity")?;
    Ok(fs::canonicalize(home)
        .map_err(|e| e.to_string())?
        .join("apply-v1")
        .join(format!("transaction-{}-{id}", task.id)))
}
fn load_journal(directory: &Path, task: &Task, id: &str) -> Result<Journal, String> {
    let bytes = baseline::load_bytes(directory, "journal.json")?;
    let journal: Journal = serde_json::from_slice(&bytes).map_err(|_| "Invalid apply journal")?;
    if journal.version != 1
        || journal.task_id != task.id
        || journal.transaction_id != id
        || identity(task)? != journal.identity
        || journal.operations.len() > MAX_FILES
        || journal.directories.len() > MAX_FILES
    {
        return Err("Apply recovery identity mismatch; inspect retained evidence manually.".into());
    }
    let mut directory_names = BTreeSet::new();
    for directory in &journal.directories {
        name(directory.as_bytes())?;
        if !directory_names.insert(directory)
            || !journal
                .operations
                .iter()
                .any(|op| op.before.is_none() && op.path.starts_with(&format!("{directory}/")))
        {
            return Err("Invalid directory recovery plan; nothing changed.".into());
        }
    }
    let mut paths = BTreeSet::new();
    let mut total = 0u64;
    for (i, op) in journal.operations.iter().enumerate() {
        name(op.path.as_bytes())?;
        if !paths.insert(op.path.to_ascii_lowercase())
            || op.stage != format!(".pui-apply-{id}-{i}-new")
            || op.backup != format!(".pui-apply-{id}-{i}-old")
            || op.before == op.after
        {
            return Err("Malformed apply recovery operation; no files changed.".into());
        }
        for state in op.before.iter().chain(op.after.iter()) {
            total = total
                .checked_add(state.bytes.len() as u64)
                .ok_or("Recovery bytes overflow")?;
            if !matches!(state.mode, 0o644 | 0o755)
                || state.bytes.contains(&0)
                || std::str::from_utf8(&state.bytes).is_err()
                || total > 2 * MAX_BYTES
            {
                return Err("Unsupported or oversized apply recovery content.".into());
            }
        }
        if op
            .before
            .as_ref()
            .zip(op.after.as_ref())
            .is_some_and(|(a, b)| a.mode != b.mode)
        {
            return Err("Unsupported apply recovery mode change.".into());
        }
    }
    Ok(journal)
}
pub fn inspect_recovery(home: &Path, task: &Task) -> Result<Vec<RecoveryStatus>, String> {
    let root = home.join("apply-v1");
    let entries = match fs::read_dir(&root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let prefix = format!("transaction-{}-", task.id);
    let mut found = Vec::new();
    for (count, entry) in entries.enumerate() {
        if count >= 1000 {
            return Err(
                "Apply history exceeds its scan budget; no transactions were assumed complete."
                    .into(),
            );
        }
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(id) = name.strip_prefix(&prefix) else {
            continue;
        };
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid retained transaction directory")?;
        let directory = entry.path();
        if entry.file_type().map_err(|e| e.to_string())?.is_symlink() {
            return Err("Recovery directory cannot be a symlink.".into());
        }
        let latest = events(&directory)?.pop();
        let result = latest
            .and_then(|(_, name)| baseline::load_bytes(&directory, &name).ok())
            .and_then(|bytes| serde_json::from_slice::<ApplyOutcome>(&bytes).ok())
            .filter(|r| r.transaction_id == id);
        found.push(result.unwrap_or_else(||outcome(&directory,id,TransactionStatus::Interrupted,"No valid durable latest event. Source effects are uncertain; inspect retained journal and explicitly request conditional rollback. Never replay.".into())));
    }
    Ok(found)
}
pub fn recover_owned(
    home: &Path,
    claim: &fs::File,
    expected: &Task,
    id: &str,
    action: RecoveryAction,
) -> Result<ApplyOutcome, String> {
    validate_claim(home, claim, expected)?;
    let task = super::super::store::current_record(home, expected)?;
    let directory = journal_path(home, &task, id)?;
    let journal = load_journal(&directory, &task, id)?;
    let retained = recovery_directory(&journal.identity, &journal.transaction_id, false)?;
    let parents: Result<Vec<_>, _> = journal
        .operations
        .iter()
        .map(|op| {
            let mut p = parent(&journal.identity.original, &op.path)?;
            parent_matches(&journal.identity.original, op, &p)?;
            p.recovery = Some(retained.try_clone().map_err(|e| e.to_string())?);
            Ok::<_, String>(p)
        })
        .collect();
    let parents = parents?;
    let created =
        directories::receipts(&journal.identity.original, &journal.directories, &directory)?;
    // Append and fsync BEFORE any rollback mutation. A crash now supersedes an
    // earlier Applied event; inspection never replays or reports old success.
    append_event(&outcome(&directory,id,TransactionStatus::Interrupted,"Explicit conditional rollback started; completion is uncertain until a later durable event.".into()))?;
    match action {
        RecoveryAction::RollBack => {
            let (done, mut detail) = rollback(&directory, &journal, &parents);
            let cleanup = if done {
                directories::cleanup(&journal.identity.original, &created)
            } else {
                Err("Directory cleanup awaits file recovery.".into())
            };
            let done = done && cleanup.is_ok();
            if let Err(error) = cleanup {
                detail.push_str(&format!(" {error}"));
            }
            Ok(finish(
                &directory,
                id,
                if done {
                    TransactionStatus::RolledBack
                } else {
                    TransactionStatus::RecoveryRequired
                },
                detail,
            ))
        }
    }
}
