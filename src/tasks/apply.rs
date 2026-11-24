//! Explicit reviewed apply-back. Git's index is never written. Multi-file filesystem
//! replacement is NOT atomic: a durable journal precedes writes, failures roll back
//! conditionally, and uncertain/external edits retain evidence for explicit recovery.
use super::workspace::snapshot::source;
use super::{Phase, Task, workspace};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};
#[path = "apply_baseline.rs"]
mod baseline;
#[path = "apply_transaction.rs"]
mod transaction;
pub use baseline::{BaselineReceipt, capture_receipt};
pub use transaction::{
    ApplyOutcome, RecoveryAction, RecoveryStatus, TransactionStatus, apply_owned, inspect_recovery,
    recover_owned,
};

#[cfg(test)]
pub use transaction::apply;

const MAX_FILES: usize = 10_000;
const MAX_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_BINARY_FILE_BYTES: usize = 4 * 1024 * 1024;
// Kept for older read-only preview callers while their UI migrates.
pub const TRANSACTIONAL_BLOCKER: &str = "Preview is not apply permission. Applying requires a separate explicit confirmation of this exact reviewed plan. No commit, merge, stash or index update is performed.";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub original: PathBuf,
    pub candidate: PathBuf,
    pub original_scope: PathBuf,
    pub candidate_scope: PathBuf,
    pub original_git_dir: PathBuf,
    pub candidate_git_dir: PathBuf,
    pub common_dir: PathBuf,
    pub head: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeKind {
    Add,
    Modify,
    Delete,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
    pub conflict: bool,
    pub before: Option<String>,
    pub after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_bytes: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_bytes: Option<Vec<u8>>,
    pub unsupported: Option<String>,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    pub before_mode: Option<u32>,
    pub after_mode: Option<u32>,
}
impl Change {
    fn before_state(&self) -> Result<Option<FileState>, String> {
        state_from_parts(
            &self.path,
            &self.before,
            &self.before_bytes,
            self.before_mode,
        )
    }
    fn after_state(&self) -> Result<Option<FileState>, String> {
        state_from_parts(&self.path, &self.after, &self.after_bytes, self.after_mode)
    }
}
fn state_from_parts(
    path: &str,
    text: &Option<String>,
    raw: &Option<Vec<u8>>,
    mode: Option<u32>,
) -> Result<Option<FileState>, String> {
    match (text, raw, mode) {
        (Some(s), None, Some(mode)) => Ok(Some(FileState {
            bytes: s.as_bytes().into(),
            mode,
        })),
        (None, Some(bytes), Some(mode)) => Ok(Some(FileState {
            bytes: bytes.clone(),
            mode,
        })),
        (None, None, None) => Ok(None),
        _ => Err(format!("Malformed apply change state for {path}.")),
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub task_id: String,
    pub identity: Identity,
    pub baseline_fingerprint: String,
    pub original_fingerprint: String,
    pub candidate_fingerprint: String,
    pub checked_candidate_fingerprint: String,
    inventory: String,
    task_hash: String,
    paths: BTreeSet<String>,
    /// SHA-256 of exact versioned full-file payload; no shell/Git patch parsing.
    pub patch_hash: String,
    pub changes: Vec<Change>,
    pub blockers: Vec<String>,
}
impl Plan {
    pub fn patch_payload(&self) -> String {
        payload(&self.changes)
    }
    pub fn summary(&self) -> String {
        format!(
            "{} builder net changes · {} conflicts\nPatch SHA-256: {}\nOriginal: {}\nCandidate: {}\nWorkflow: {}\nPreparation baseline SHA-256: {}\nOriginal SHA-256: {}\nCandidate SHA-256: {}\n{}\n{}",
            self.changes.len(),
            self.changes.iter().filter(|c| c.conflict).count(),
            self.patch_hash,
            self.identity.original.display(),
            self.identity.candidate.display(),
            self.task_id,
            self.baseline_fingerprint,
            self.original_fingerprint,
            self.candidate_fingerprint,
            TRANSACTIONAL_BLOCKER,
            self.blockers.join("\n")
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct FileState {
    bytes: Vec<u8>,
    mode: u32,
}
type Tree = BTreeMap<String, Option<FileState>>;
#[derive(Clone, Debug, PartialEq, Eq)]
struct GitEntry {
    mode: String,
    oid: String,
}
type Entries = BTreeMap<String, GitEntry>;
fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new("git");
    cmd.arg("--no-replace-objects")
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "core.precomposeUnicode=true",
        ])
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1");
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
    ] {
        cmd.env_remove(var);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Cannot inspect apply-back: {e}"))?;
    // Read on a bounded worker so Git cannot hang an apply indefinitely (e.g. a
    // corrupt repository on a remote mount). No detached process survives timeout.
    let mut stdout = child.stdout.take().ok_or("Git output pipe unavailable")?;
    let (send, receive) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = (&mut stdout).take(MAX_BYTES + 1).read_to_end(&mut bytes);
        let _ = send.send((result, bytes));
    });
    let received = receive.recv_timeout(std::time::Duration::from_secs(15));
    if received.is_err() {
        let _ = child.kill();
        let _ = child.wait(); /* Reader owns only a pipe, never source state. */
        return Err("Git inspection timed out. No apply was started.".into());
    }
    let (result, bytes) = received.unwrap();
    let _ = reader.join();
    if result.is_err() || bytes.len() as u64 > MAX_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Git inspection exceeds its 32 MiB budget.".into());
    }
    // stdout EOF is not proof of process exit.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) if status.success() => return Ok(bytes),
            Some(_) => return Err("Cannot inspect Git for apply-back.".into()),
            None if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Git inspection timed out.".into());
            }
        }
    }
}
fn text(bytes: Vec<u8>) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|_| "Non-UTF-8 Git metadata is unsupported.".into())
}
fn is_ambiguous_unicode(ch: char) -> bool {
    matches!(
        ch,
        '\u{0300}'..='\u{036F}'
            | '\u{1100}'..='\u{11FF}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FE20}'..='\u{FE2F}'
            | '\u{FEFF}'
    )
}
fn name(raw: &[u8]) -> Result<String, String> {
    let name = std::str::from_utf8(raw).map_err(|_| "Non-UTF-8 apply path is unsupported.")?;
    if name.chars().any(is_ambiguous_unicode) {
        return Err("Unicode-normalization ambiguity is unsupported for apply paths.".into());
    }
    if name.is_empty()
        || name.len() > 4096
        || name.contains(['\\', '\0'])
        || name.chars().any(char::is_control)
        || Path::new(name)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Ambiguous or external apply path is unsupported.".into());
    }
    for part in name.split('/') {
        let lower = part.to_lowercase();
        if matches!(
            lower.as_str(),
            ".git"
                | ".hg"
                | ".svn"
                | ".ssh"
                | ".aws"
                | ".gnupg"
                | "node_modules"
                | "target"
                | "id_rsa"
                | "id_ed25519"
                | "credentials"
        ) || lower.starts_with(".env")
            || lower.starts_with(".pui-apply-")
            || lower.ends_with(".pem")
            || lower.ends_with(".key")
        {
            return Err("Protected or generated apply path is unsupported.".into());
        }
    }
    Ok(name.into())
}
fn entries(raw: &[u8], index: bool) -> Result<Entries, String> {
    let mut result = BTreeMap::new();
    for row in raw.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let tab = row
            .iter()
            .position(|b| *b == b'\t')
            .ok_or("Invalid Git entry")?;
        let header = std::str::from_utf8(&row[..tab]).map_err(|_| "Invalid Git entry")?;
        let parts: Vec<_> = header.split(' ').collect();
        if parts.len() != 3 || (index && parts[2] != "0") {
            return Err("Unmerged or ambiguous index is unsupported.".into());
        }
        let oid = if index { parts[1] } else { parts[2] };
        if !matches!(parts[0], "100644" | "100755") {
            return Err("Symlinks, submodules, sparse entries and special modes are unsupported for apply-back.".into());
        }
        if !matches!(oid.len(), 40 | 64) || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid Git object identity.".into());
        }
        if result
            .insert(
                name(&row[tab + 1..])?,
                GitEntry {
                    mode: parts[0].into(),
                    oid: oid.into(),
                },
            )
            .is_some()
        {
            return Err("Duplicate apply path.".into());
        }
        if result.len() > MAX_FILES {
            return Err("Too many apply files.".into());
        }
    }
    Ok(result)
}
fn canonical_output(root: &Path, args: &[&str]) -> Result<PathBuf, String> {
    let value = text(git(root, args)?)?;
    fs::canonicalize(root.join(value.trim_end_matches('\n'))).map_err(|e| e.to_string())
}
fn resolve_identity(source: &Path, candidate: &Path, head: &str) -> Result<Identity, String> {
    let original_scope = fs::canonicalize(source).map_err(|e| e.to_string())?;
    let candidate_scope = fs::canonicalize(candidate).map_err(|e| e.to_string())?;
    let original = workspace::fingerprint_root(&original_scope)?;
    let candidate = workspace::fingerprint_root(&candidate_scope)?;
    if original == candidate || original.starts_with(&candidate) || candidate.starts_with(&original)
    {
        return Err("Apply requires distinct non-nested original and candidate checkouts.".into());
    }
    if original_scope.strip_prefix(&original).ok() != candidate_scope.strip_prefix(&candidate).ok()
    {
        return Err("Original and candidate subfolder scopes differ.".into());
    }
    if !matches!(head.len(), 40 | 64) || !head.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Missing baseline commit identity.".into());
    }
    for root in [&original, &candidate] {
        if canonical_output(root, &["rev-parse", "--show-toplevel"])? != *root
            || text(git(root, &["rev-parse", "--verify", "HEAD"])?)?.trim() != head
        {
            return Err("Original or candidate HEAD/root changed. Start a new review.".into());
        }
        if !git(root, &["rev-parse", "--shared-index-path"])?.is_empty() {
            return Err("Split indexes are unsupported for apply-back.".into());
        }
    }
    let common_dir = canonical_output(&original, &["rev-parse", "--git-common-dir"])?;
    if canonical_output(&candidate, &["rev-parse", "--git-common-dir"])? != common_dir {
        return Err("Candidate is not a linked worktree of the original repository.".into());
    }
    let original_git_dir = canonical_output(&original, &["rev-parse", "--git-dir"])?;
    let candidate_git_dir = canonical_output(&candidate, &["rev-parse", "--git-dir"])?;
    if original_git_dir == candidate_git_dir {
        return Err("Candidate shares the original index.".into());
    }
    Ok(Identity {
        original,
        candidate,
        original_scope,
        candidate_scope,
        original_git_dir,
        candidate_git_dir,
        common_dir,
        head: head.into(),
    })
}
fn identity(task: &Task) -> Result<Identity, String> {
    if !task.isolated {
        return Err("Apply requires an isolated Git workflow.".into());
    }
    resolve_identity(
        &task.source,
        task.workspace.as_ref().ok_or("No candidate workspace")?,
        &task.base_head,
    )
}
fn read(root: &Path, relative: &str, total: &mut u64) -> Result<Option<FileState>, String> {
    match source::entry(root, Path::new(relative))? {
        source::Entry::Missing => {
            if fs::symlink_metadata(root.join(relative)).is_ok() {
                return Err("File/directory typechanges are unsupported for apply-back.".into());
            }
            Ok(None)
        }
        source::Entry::Link(_) => {
            Err("Symlinks are unsupported for apply-back; no link was followed.".into())
        }
        source::Entry::File(mut file) => {
            let meta = file.metadata().map_err(|e| e.to_string())?;
            if meta.len() > MAX_BYTES.saturating_sub(*total) {
                return Err("Apply exceeds its 32 MiB budget.".into());
            }
            let mut bytes = Vec::new();
            (&mut file)
                .take(meta.len() + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            let after = file.metadata().map_err(|e| e.to_string())?;
            if bytes.len() as u64 != meta.len() || meta.modified().ok() != after.modified().ok() {
                return Err("Files changed during apply capture.".into());
            }
            *total += meta.len();
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                meta.permissions().mode() & 0o777
            };
            #[cfg(not(unix))]
            let mode = 0;
            Ok(Some(FileState { bytes, mode }))
        }
    }
}
fn capture(
    root: &Path,
    git_dir: &Path,
    paths: &BTreeSet<String>,
) -> Result<(Tree, String), String> {
    let mut tree = Tree::new();
    let mut total = 0;
    let index = read(git_dir, "index", &mut total)?;
    for path in paths {
        tree.insert(path.clone(), read(root, path, &mut total)?);
    }
    let mut hash = Sha256::new();
    frame(&mut hash, b"apply-state-v2");
    hash_file(&mut hash, index.as_ref());
    for (path, file) in &tree {
        frame(&mut hash, path.as_bytes());
        hash_file(&mut hash, file.as_ref());
    }
    Ok((tree, format!("{:x}", hash.finalize())))
}
fn frame(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}
fn hash_file(hash: &mut Sha256, file: Option<&FileState>) {
    if let Some(f) = file {
        hash.update([1]);
        hash.update(f.mode.to_le_bytes());
        frame(hash, &f.bytes);
    } else {
        hash.update([0]);
    }
}
fn payload(changes: &[Change]) -> String {
    serde_json::json!({"format":"pui-full-file-apply-v2","changes":changes}).to_string()
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn all_paths(identity: &Identity) -> Result<BTreeSet<String>, String> {
    let mut paths: BTreeSet<_> = entries(
        &git(
            &identity.candidate,
            &["ls-tree", "-r", "-z", &identity.head],
        )?,
        false,
    )?
    .into_keys()
    .collect();
    for root in [&identity.original, &identity.candidate] {
        paths.extend(
            entries(
                &git(root, &["ls-files", "--stage", "--sparse", "-z"])?,
                true,
            )?
            .into_keys(),
        );
        for raw in git(root, &["ls-files", "--others", "--exclude-standard", "-z"])?
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
        {
            paths.insert(name(raw)?);
        }
    }
    if paths.len() > MAX_FILES {
        return Err("Too many apply files.".into());
    }
    let mut folded = BTreeSet::new();
    if paths.iter().any(|p| !folded.insert(p.to_lowercase())) {
        return Err("Case-ambiguous apply paths are unsupported.".into());
    }
    Ok(paths)
}
fn inventory(identity: &Identity) -> Result<String, String> {
    let mut hash = Sha256::new();
    for root in [&identity.original, &identity.candidate] {
        frame(
            &mut hash,
            &git(root, &["ls-files", "--stage", "--sparse", "-z"])?,
        );
        frame(
            &mut hash,
            &git(root, &["ls-files", "--others", "--exclude-standard", "-z"])?,
        );
    }
    Ok(format!("{:x}", hash.finalize()))
}
/// Read-only exact builder net-change plan against the durable pre-builder receipt.
/// Later unrelated source edits are preserved; changed source target paths conflict.
pub fn prepare(task: &Task) -> Result<Plan, String> {
    let attempt = task.current_evidence().ok_or("No checked candidate.")?;
    if task.phase != Phase::Ready || !task.approved.as_ref().is_some_and(|c| attempt.passed(c)) {
        return Err("Apply needs current passing checks and independent review.".into());
    }
    let checked = attempt
        .fingerprint
        .as_ref()
        .ok_or("Missing candidate fingerprint")?;
    let identity = identity(task)?;
    let baseline = baseline::load(task)?;
    if baseline.identity != identity {
        return Err("Preparation baseline identity differs from the current workflow.".into());
    }
    let mut paths = all_paths(&identity)?;
    paths.extend(baseline.original.tree.keys().cloned());
    let inventory = inventory(&identity)?;
    if workspace::fingerprint(&identity.candidate)? != *checked {
        return Err("Candidate changed after review; evidence is stale.".into());
    }
    let original = baseline::snapshot(&identity.original, &identity.original_git_dir, &paths)?;
    let candidate = baseline::snapshot(&identity.candidate, &identity.candidate_git_dir, &paths)?;
    let mut blockers = Vec::new();
    if candidate.index != baseline.candidate.index {
        blockers.push("Candidate index differs from the preparation baseline; staged edits/flags are unsupported. Keep builder changes unstaged.".into());
    }
    let mut changes = Vec::new();
    for path in &paths {
        let before = baseline.candidate.tree.get(path).and_then(Option::as_ref);
        let after = candidate.tree[path].as_ref();
        if before == after {
            continue;
        }
        let conflict = original.tree[path].as_ref()
            != baseline.original.tree.get(path).and_then(Option::as_ref);
        let kind = match (before, after) {
            (None, _) => ChangeKind::Add,
            (_, None) => ChangeKind::Delete,
            _ => ChangeKind::Modify,
        };
        let as_text = |f: Option<&FileState>| {
            f.and_then(|f| {
                String::from_utf8(f.bytes.clone())
                    .ok()
                    .filter(|_| !f.bytes.contains(&0))
            })
        };
        let old = as_text(before);
        let new = as_text(after);
        let old_binary = before.filter(|_| old.is_none()).map(|f| &f.bytes);
        let new_binary = after.filter(|_| new.is_none()).map(|f| &f.bytes);
        let mut unsupported = Vec::new();
        if before
            .into_iter()
            .chain(after)
            .any(|f| !matches!(f.mode, 0o644 | 0o755))
        {
            unsupported.push("Special permission modes outside 0644/0755 are unsupported.");
        }
        let binary_oversize = old_binary
            .into_iter()
            .chain(new_binary)
            .any(|b| b.len() > MAX_BINARY_FILE_BYTES);
        if binary_oversize {
            unsupported.push("Binary file exceeds the 4 MiB per-file apply limit.");
        }
        if transaction::missing_directories(&identity.original, path).is_err() {
            unsupported.push("Target parents must be ordinary directories or safely creatable missing directories.");
        }
        if conflict {
            blockers.push(format!(
                "Conflict: {path} differs from the preparation snapshot in the original checkout."
            ));
        }
        let unsupported = (!unsupported.is_empty()).then(|| unsupported.join(" "));
        if let Some(reason) = &unsupported {
            blockers.push(format!("{path}: {reason}"));
        }
        changes.push(Change {
            path: path.clone(),
            kind,
            conflict,
            before: old,
            after: new,
            before_bytes: (!binary_oversize).then(|| old_binary.cloned()).flatten(),
            after_bytes: (!binary_oversize).then(|| new_binary.cloned()).flatten(),
            unsupported,
            before_hash: before.map(|f| digest(&f.bytes)),
            after_hash: after.map(|f| digest(&f.bytes)),
            before_mode: before.map(|f| f.mode),
            after_mode: after.map(|f| f.mode),
        });
    }
    if changes.is_empty() {
        blockers.push("No builder net changes.".into());
    }
    let plan = Plan {
        task_id: task.id.clone(),
        identity,
        baseline_fingerprint: task
            .preparation_receipt
            .as_ref()
            .unwrap()
            .manifest_hash
            .clone(),
        original_fingerprint: original.fingerprint,
        candidate_fingerprint: candidate.fingerprint,
        checked_candidate_fingerprint: checked.clone(),
        inventory,
        task_hash: digest(&serde_json::to_vec(task).map_err(|e| e.to_string())?),
        paths,
        patch_hash: digest(payload(&changes).as_bytes()),
        changes,
        blockers,
    };
    verify_state(task, &plan)?;
    Ok(plan)
}
fn verify_state(task: &Task, plan: &Plan) -> Result<(), String> {
    if identity(task)? != plan.identity
        || inventory(&plan.identity)? != plan.inventory
        || workspace::fingerprint(&plan.identity.candidate)? != plan.checked_candidate_fingerprint
        || capture(
            &plan.identity.original,
            &plan.identity.original_git_dir,
            &plan.paths,
        )?
        .1 != plan.original_fingerprint
        || capture(
            &plan.identity.candidate,
            &plan.identity.candidate_git_dir,
            &plan.paths,
        )?
        .1 != plan.candidate_fingerprint
    {
        return Err(
            "Original or candidate changed during apply preview. Nothing was changed.".into(),
        );
    }
    Ok(())
}
/// Recompute authoritative evidence, task, indexes, paths, bytes and exact payload.
pub fn check(task: &Task, plan: &Plan) -> Result<(), String> {
    let current = prepare(task)?;
    if current != *plan || digest(plan.patch_payload().as_bytes()) != plan.patch_hash {
        return Err(
            "Apply plan is stale or altered. Review a fresh plan; nothing was changed.".into(),
        );
    }
    if !current.blockers.is_empty() {
        return Err(current.blockers.join("\n"));
    }
    Ok(())
}
#[cfg(test)]
#[path = "apply_tests.rs"]
mod tests;
