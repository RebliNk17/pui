//! Read-only, conflict-aware apply-back preparation. This is NOT an apply engine.
//!
//! A multi-file working tree cannot be replaced transactionally with ordinary
//! rename/copy operations, and advisory locks do not exclude editors or Git.
//! Until a recoverable transaction/rollback protocol exists, every plan is
//! explicitly blocked from mutation. No commit, merge, stash or index writes.
use super::{Phase, Task, workspace};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};
// The same descriptor-relative, no-follow reader used by source snapshots.
use super::workspace::snapshot::source;

const MAX_FILES: usize = 10_000;
const MAX_BYTES: u64 = 32 * 1024 * 1024;
pub const TRANSACTIONAL_BLOCKER: &str = "Apply-back is unavailable: an all-file recoverable transaction with conflict-safe rollback has not been implemented. Preview/check never changes the original checkout. Use separately reviewed regular development; workflow approval is not permission to apply.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub original: PathBuf,
    pub candidate: PathBuf,
    pub original_git_dir: PathBuf,
    pub candidate_git_dir: PathBuf,
    pub common_dir: PathBuf,
    pub head: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Add,
    Modify,
    Delete,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
    pub conflict: bool,
    /// Full-file text preview, not an executable shell command or Git patch.
    pub before: Option<String>,
    pub after: Option<String>,
    pub unsupported: Option<String>,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    pub before_mode: Option<u32>,
    pub after_mode: Option<u32>,
}
#[derive(Clone, Debug)]
pub struct Plan {
    pub task_id: String,
    pub identity: Identity,
    pub baseline_fingerprint: String,
    pub original_fingerprint: String,
    pub candidate_fingerprint: String,
    pub checked_candidate_fingerprint: String,
    inventory: String,
    /// SHA-256 of the exact versioned, length-framed net-change payload.
    pub patch_hash: String,
    pub changes: Vec<Change>,
    /// Always includes TRANSACTIONAL_BLOCKER; never present this as actionable.
    pub blockers: Vec<String>,
}
impl Plan {
    /// Machine-readable, exact content addressed by patch_hash. Not `git apply`
    /// input: full-file replacements avoid diff ambiguity/newline confusion.
    #[cfg(test)]
    pub fn patch_payload(&self) -> String {
        payload(&self.changes)
    }
    pub fn summary(&self) -> String {
        format!(
            "{} HEAD-relative candidate changes · {} conflicts\nPatch SHA-256: {}\nOriginal: {}\nCandidate: {}\nWorkflow: {}\nHEAD baseline SHA-256: {}\nOriginal SHA-256: {}\nCandidate SHA-256: {}\n{}",
            self.changes.len(),
            self.changes.iter().filter(|c| c.conflict).count(),
            self.patch_hash,
            self.identity.original.display(),
            self.identity.candidate.display(),
            self.task_id,
            self.baseline_fingerprint,
            self.original_fingerprint,
            self.candidate_fingerprint,
            self.blockers.join("\n")
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
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
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or("Git output pipe unavailable.")?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() as u64 > MAX_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Cannot read apply preview within its 32 MiB budget.".into());
    }
    if !child.wait().map_err(|e| e.to_string())?.success() {
        return Err("Cannot inspect Git for apply-back. Nothing was changed.".into());
    }
    Ok(bytes)
}
fn text(bytes: Vec<u8>) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|_| "Non-UTF-8 Git metadata is unsupported.".into())
}
fn name(raw: &[u8]) -> Result<String, String> {
    let name = std::str::from_utf8(raw).map_err(|_| "Non-UTF-8 apply path is unsupported.")?;
    if !name.is_ascii() {
        return Err(
            "Unicode-normalization ambiguity is unsupported for apply preview paths.".into(),
        );
    }
    if name.is_empty()
        || name.contains(['\\', '\0'])
        || name.chars().any(char::is_control)
        || Path::new(name)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Ambiguous or external apply path is unsupported.".into());
    }
    for part in name.split('/') {
        let lower = part.to_ascii_lowercase();
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
            .ok_or("Invalid Git entry.")?;
        let header = std::str::from_utf8(&row[..tab]).map_err(|_| "Invalid Git entry.")?;
        let parts: Vec<_> = header.split(' ').collect();
        if parts.len() != 3 || (index && parts[2] != "0") {
            return Err("Unmerged or ambiguous index is unsupported for apply preview.".into());
        }
        let oid = if index { parts[1] } else { parts[2] };
        if !matches!(parts[0], "100644" | "100755") {
            return Err("Symlinks, submodules, sparse entries and special modes are unsupported for apply preview.".into());
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
            return Err("Too many apply preview files.".into());
        }
    }
    Ok(result)
}
fn canonical_output(root: &Path, args: &[&str]) -> Result<PathBuf, String> {
    let value = text(git(root, args)?)?;
    fs::canonicalize(root.join(value.trim_end_matches('\n'))).map_err(|e| e.to_string())
}
fn identity(task: &Task) -> Result<Identity, String> {
    if !task.isolated || task.dirty_source {
        return Err("Apply preview requires an isolated, clean-at-start Git workflow. Dirty-at-start workflows lack a durable original snapshot receipt; HEAD is not their baseline. Nothing was changed.".into());
    }
    let original = fs::canonicalize(&task.source).map_err(|e| e.to_string())?;
    let candidate = fs::canonicalize(task.workspace.as_ref().ok_or("No candidate workspace.")?)
        .map_err(|e| e.to_string())?;
    if original == candidate || original.starts_with(&candidate) || candidate.starts_with(&original)
    {
        return Err("Apply requires distinct non-nested original and candidate checkouts.".into());
    }
    for root in [&original, &candidate] {
        if canonical_output(root, &["rev-parse", "--show-toplevel"])? != *root {
            return Err(
                "Apply preview currently requires full repository roots, not subfolders.".into(),
            );
        }
        if text(git(root, &["rev-parse", "--verify", "HEAD"])?)?.trim() != task.base_head {
            return Err("Original or candidate HEAD changed. Start a new review.".into());
        }
    }
    if !matches!(task.base_head.len(), 40 | 64)
        || !task.base_head.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Missing baseline commit identity.".into());
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
        original_git_dir,
        candidate_git_dir,
        common_dir,
        head: task.base_head.clone(),
    })
}
fn read(root: &Path, relative: &str, total: &mut u64) -> Result<Option<FileState>, String> {
    match source::entry(root, Path::new(relative))? {
        source::Entry::Missing => {
            if fs::symlink_metadata(root.join(relative)).is_ok() {
                return Err("File/directory typechanges are unsupported for apply preview.".into());
            }
            Ok(None)
        }
        source::Entry::Link(link) => {
            let _ = link;
            Err("Symlinks are unsupported for apply preview; no link was followed.".into())
        }
        source::Entry::File(mut file) => {
            let meta = file.metadata().map_err(|e| e.to_string())?;
            if meta.len() > MAX_BYTES - *total {
                return Err("Apply preview exceeds its 32 MiB budget.".into());
            }
            let mut bytes = Vec::new();
            (&mut file)
                .take(meta.len() + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            let after = file.metadata().map_err(|e| e.to_string())?;
            if bytes.len() as u64 != meta.len() || meta.modified().ok() != after.modified().ok() {
                return Err("Files changed during apply preview.".into());
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
    // Hash the raw index as well as working bytes, preserving all staged edits and flags.
    let index = read(git_dir, "index", &mut total)?;
    for path in paths {
        tree.insert(path.clone(), read(root, path, &mut total)?);
    }
    let mut hash = Sha256::new();
    frame(&mut hash, b"apply-state-v1");
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
    let rows: Vec<_> = changes.iter().map(|c| serde_json::json!({"path":c.path,"kind":format!("{:?}", c.kind),"before":c.before,"after":c.after,"before_sha256":c.before_hash,"after_sha256":c.after_hash,"before_mode":c.before_mode,"after_mode":c.after_mode,"unsupported":c.unsupported})).collect();
    serde_json::json!({"format":"pui-full-file-preview-v1","changes":rows}).to_string()
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Prepare only: no writer lock, filesystem writes, shell commands or provider calls.
/// HEAD-relative changes are for inspection, NOT proof of which edits a builder
/// introduced: even initially clean tasks lack a durable preparation receipt.
/// Unrelated original paths are bound and never mutated. A changed target is a
/// conflict even if its text would merge.
pub fn prepare(task: &Task) -> Result<Plan, String> {
    let attempt = task.latest().ok_or("No checked candidate.")?;
    if task.phase != Phase::Ready || !task.approved.as_ref().is_some_and(|c| attempt.passed(c)) {
        return Err("Apply preview needs current passing checks and independent review.".into());
    }
    let checked = attempt
        .fingerprint
        .as_ref()
        .ok_or("Missing candidate fingerprint.")?;
    let identity = identity(task)?;
    let inventory = inventory(&identity)?;
    if workspace::fingerprint(&identity.candidate)? != *checked {
        return Err("Candidate changed after review; evidence is stale.".into());
    }
    let baseline_entries = entries(
        &git(
            &identity.candidate,
            &["ls-tree", "-r", "-z", &identity.head],
        )?,
        false,
    )?;
    let mut paths: BTreeSet<String> = baseline_entries.keys().cloned().collect();
    let mut indexes = Vec::new();
    for root in [&identity.original, &identity.candidate] {
        let index = entries(
            &git(root, &["ls-files", "--stage", "--sparse", "-z"])?,
            true,
        )?;
        paths.extend(index.keys().cloned());
        indexes.push(index);
        for raw in git(root, &["ls-files", "--others", "--exclude-standard", "-z"])?
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
        {
            paths.insert(name(raw)?);
        }
    }
    if paths.len() > MAX_FILES {
        return Err("Too many apply preview files.".into());
    }
    // Portable/case-folding ambiguity: stricter than Git, intentionally fail closed.
    let mut folded = BTreeSet::new();
    if paths.iter().any(|p| !folded.insert(p.to_lowercase())) {
        return Err("Case-ambiguous apply paths are unsupported.".into());
    }
    let mut baseline = Tree::new();
    let mut total = 0;
    for path in &paths {
        let file = baseline_entries
            .get(path)
            .map(|entry| -> Result<FileState, String> {
                let bytes = git(&identity.candidate, &["cat-file", "blob", &entry.oid])?;
                total += bytes.len() as u64;
                if total > MAX_BYTES {
                    return Err("Baseline exceeds the apply preview budget.".into());
                }
                Ok(FileState {
                    bytes,
                    mode: if entry.mode == "100755" { 0o755 } else { 0o644 },
                })
            })
            .transpose()?;
        baseline.insert(path.clone(), file);
    }
    let (original, original_fingerprint) =
        capture(&identity.original, &identity.original_git_dir, &paths)?;
    let (candidate, candidate_fingerprint) =
        capture(&identity.candidate, &identity.candidate_git_dir, &paths)?;
    let mut baseline_hash = Sha256::new();
    frame(&mut baseline_hash, identity.head.as_bytes());
    let mut changes = Vec::new();
    let mut blockers = vec![TRANSACTIONAL_BLOCKER.into(), "No durable preparation baseline receipt: this is a HEAD-relative preview, not a proven builder-only patch. Initial clean status alone cannot prove that source edits were not copied later.".into()];
    if indexes[1] != baseline_entries {
        blockers.push("Candidate index differs from the clean baseline; staged-only edits/intent-to-add are not an apply-back payload.".into());
    }
    for path in &paths {
        let before = baseline[path].as_ref();
        frame(&mut baseline_hash, path.as_bytes());
        hash_file(&mut baseline_hash, before);
        let after = candidate[path].as_ref();
        if before == after {
            continue;
        }
        let conflict = original[path].as_ref() != before;
        let kind = match (before, after) {
            (None, _) => ChangeKind::Add,
            (_, None) => ChangeKind::Delete,
            _ => ChangeKind::Modify,
        };
        let mut unsupported = Vec::new();
        if matches!(kind, ChangeKind::Delete) {
            unsupported.push("Deletion requires recoverable rollback.");
        }
        if before.zip(after).is_some_and(|(a, b)| a.mode != b.mode)
            || after.is_some_and(|f| !matches!(f.mode, 0o644 | 0o755))
        {
            unsupported.push("Mode changes are not supported for apply-back.");
        }
        let as_text = |file: Option<&FileState>| {
            file.and_then(|f| {
                String::from_utf8(f.bytes.clone())
                    .ok()
                    .filter(|_| !f.bytes.contains(&0))
            })
        };
        let old = as_text(before);
        let new = as_text(after);
        if before.is_some() && old.is_none() || after.is_some() && new.is_none() {
            unsupported.push(
                "Binary/non-UTF-8 content is previewed by path only; applying is unsupported.",
            );
        }
        let unsupported = (!unsupported.is_empty()).then(|| unsupported.join(" "));
        if conflict {
            blockers.push(format!(
                "Conflict: {path} differs from the workflow baseline in the original checkout."
            ));
        }
        if let Some(reason) = &unsupported {
            blockers.push(format!("{path}: {reason}"));
        }
        changes.push(Change {
            path: path.clone(),
            kind,
            conflict,
            before: old,
            after: new,
            unsupported,
            before_hash: before.map(|f| digest(&f.bytes)),
            after_hash: after.map(|f| digest(&f.bytes)),
            before_mode: before.map(|f| f.mode),
            after_mode: after.map(|f| f.mode),
        });
    }
    if changes.is_empty() {
        blockers.push("No candidate net changes.".into());
    }
    // Repeat the inventories as well as bytes: adding/deleting a path must stale the plan.
    let plan = Plan {
        task_id: task.id.clone(),
        identity,
        inventory,
        baseline_fingerprint: format!("{:x}", baseline_hash.finalize()),
        original_fingerprint,
        candidate_fingerprint,
        checked_candidate_fingerprint: checked.clone(),
        patch_hash: digest(payload(&changes).as_bytes()),
        changes,
        blockers,
    };
    verify_state(task, &plan, &paths)?;
    Ok(plan)
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
fn verify_state(task: &Task, plan: &Plan, paths: &BTreeSet<String>) -> Result<(), String> {
    if identity(task)? != plan.identity
        || inventory(&plan.identity)? != plan.inventory
        || workspace::fingerprint(&plan.identity.candidate)? != plan.checked_candidate_fingerprint
        || capture(
            &plan.identity.original,
            &plan.identity.original_git_dir,
            paths,
        )?
        .1 != plan.original_fingerprint
        || capture(
            &plan.identity.candidate,
            &plan.identity.candidate_git_dir,
            paths,
        )?
        .1 != plan.candidate_fingerprint
    {
        return Err(
            "Original or candidate changed during apply preview. Nothing was changed.".into(),
        );
    }
    Ok(())
}
/// Recompute from authoritative task state. A preview is not durable approval;
/// no eventual writer may skip these checks or reuse a confirmation after drift.
#[cfg(test)]
pub fn check(task: &Task, plan: &Plan) -> Result<(), String> {
    let current = prepare(task)?;
    if current.task_id != plan.task_id
        || current.identity != plan.identity
        || current.baseline_fingerprint != plan.baseline_fingerprint
        || current.original_fingerprint != plan.original_fingerprint
        || current.candidate_fingerprint != plan.candidate_fingerprint
        || current.checked_candidate_fingerprint != plan.checked_candidate_fingerprint
        || current.inventory != plan.inventory
        || current.patch_hash != plan.patch_hash
        || digest(plan.patch_payload().as_bytes()) != plan.patch_hash
    {
        return Err(
            "Apply preview is stale or altered. Review a fresh plan; nothing was changed.".into(),
        );
    }
    Ok(())
}
/// Test-only assertion of the blocked apply boundary; no production apply action exists.
#[cfg(test)]
pub fn apply(task: &Task, plan: &Plan) -> Result<(), String> {
    check(task, plan)?;
    Err(TRANSACTIONAL_BLOCKER.into())
}

#[cfg(test)]
#[path = "apply_tests.rs"]
mod tests;
