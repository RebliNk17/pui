//! Portable fail-closed apply facade. Keeping this API available lets retained
//! workflows and text views load on platforms without our verified transaction
//! primitives. No capture, preview, apply or recovery operation performs I/O here.
use super::Task;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::File,
    path::{Path, PathBuf},
};

pub const TRANSACTIONAL_BLOCKER: &str = "Reviewed apply-back is not supported on this platform. No files, index, evidence, or transaction state were changed. Use a supported macOS/Linux checkout for apply-back; viewing a retained workflow does not resume it.";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineReceipt {
    pub version: u32,
    pub task_id: String,
    pub directory: PathBuf,
    pub manifest_hash: String,
}
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
    pub patch_hash: String,
    pub changes: Vec<Change>,
    pub blockers: Vec<String>,
}
impl Plan {
    pub fn patch_payload(&self) -> String {
        serde_json::json!({"format":"pui-full-file-apply-v2","changes":self.changes}).to_string()
    }
    pub fn summary(&self) -> String {
        TRANSACTIONAL_BLOCKER.into()
    }
}
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
fn unsupported<T>() -> Result<T, String> {
    Err(TRANSACTIONAL_BLOCKER.into())
}
pub fn capture_receipt(
    _source: &Path,
    _candidate: &Path,
    _base_head: &str,
    _task_id: &str,
    _home: &Path,
) -> Result<BaselineReceipt, String> {
    unsupported()
}
pub fn prepare(_task: &Task) -> Result<Plan, String> {
    unsupported()
}
pub fn check(_task: &Task, _plan: &Plan) -> Result<(), String> {
    unsupported()
}
pub fn apply_owned(
    _home: &Path,
    _claim: &File,
    _expected: &Task,
    _reviewed: &Plan,
) -> Result<ApplyOutcome, String> {
    unsupported()
}
pub fn inspect_recovery(_home: &Path, _task: &Task) -> Result<Vec<RecoveryStatus>, String> {
    unsupported()
}
pub fn recover_owned(
    _home: &Path,
    _claim: &File,
    _expected: &Task,
    _transaction_id: &str,
    _action: RecoveryAction,
) -> Result<ApplyOutcome, String> {
    unsupported()
}
#[cfg(test)]
pub fn apply(_home: &Path, _expected: &Task, _reviewed: &Plan) -> Result<ApplyOutcome, String> {
    unsupported()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_facade_rejects_without_inspecting_paths() {
        let task = Task::new(
            "/path-that-must-not-be-read".into(),
            "p".into(),
            "m".into(),
            "off".into(),
        );
        assert_eq!(prepare(&task).unwrap_err(), TRANSACTIONAL_BLOCKER);
        assert_eq!(
            capture_receipt(
                Path::new("/source"),
                Path::new("/candidate"),
                "head",
                "id",
                Path::new("/store")
            )
            .unwrap_err(),
            TRANSACTIONAL_BLOCKER
        );
        assert_eq!(
            inspect_recovery(Path::new("/store"), &task).unwrap_err(),
            TRANSACTIONAL_BLOCKER
        );
        let receipt = BaselineReceipt {
            version: 1,
            task_id: task.id,
            directory: "/retained".into(),
            manifest_hash: "hash".into(),
        };
        let encoded = serde_json::to_vec(&receipt).unwrap();
        assert_eq!(
            serde_json::from_slice::<BaselineReceipt>(&encoded).unwrap(),
            receipt
        );
    }
}
