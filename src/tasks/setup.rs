//! User-reviewed environment setup evidence, separate from acceptance checks.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupBinding {
    pub task: String,
    pub source: PathBuf,
    pub workspace: PathBuf,
    pub task_digest: String,
    pub source_fingerprint: String,
    pub workspace_fingerprint: String,
    pub candidate_fingerprint: Option<String>,
    pub manifests_digest: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupCommand {
    pub command: String,
    /// Recorded durably before submitting this exact command to Pi.
    pub started: u64,
    pub finished: Option<u64>,
    pub exit_code: Option<i64>,
    pub cancelled: bool,
    pub output: String,
    pub truncated: bool,
    pub error: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetupStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupRecord {
    pub id: String,
    pub binding: SetupBinding,
    pub commands: Vec<String>,
    pub approved: u64,
    pub finished: Option<u64>,
    pub results: Vec<SetupCommand>,
    pub status: SetupStatus,
    pub source_changed: bool,
    pub note: String,
}
