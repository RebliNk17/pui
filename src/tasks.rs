//! Verified-task domain. No renderer, provider calls, or implicit side effects.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub mod proposal;
pub mod routing;
pub mod store;
pub mod workspace;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Draft,
    Proposing,
    AwaitingApproval,
    Preparing,
    Building,
    Capturing,
    Checking,
    Verifying,
    Ready,
    Accepted,
    Failed,
    Blocked,
    Interrupted,
    Cancelled,
    Stale,
}
impl Phase {
    pub fn active(self) -> bool {
        matches!(
            self,
            Self::Proposing
                | Self::Preparing
                | Self::Building
                | Self::Capturing
                | Self::Checking
                | Self::Verifying
        )
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "Draft",
            Self::Proposing => "Proposing checks",
            Self::AwaitingApproval => "Approve acceptance checks",
            Self::Preparing => "Preparing workspace",
            Self::Building => "Implementing",
            Self::Capturing => "Fingerprinting candidate",
            Self::Checking => "Running checks",
            Self::Verifying => "Independent review",
            Self::Ready => "Checks passed · review ready",
            Self::Accepted => "Accepted by you",
            Self::Failed => "Checks failed · repair limit reached",
            Self::Blocked => "Blocked",
            Self::Interrupted => "Interrupted · not resumed",
            Self::Cancelled => "Cancelled",
            Self::Stale => "Evidence stale · revalidation needed",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Proposal,
    Builder,
    Checks,
    Verifier,
}
impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Self::Proposal => "Acceptance proposal",
            Self::Builder => "Builder",
            Self::Checks => "Check runner",
            Self::Verifier => "Fresh verifier",
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Criterion {
    pub id: String,
    pub description: String,
    /// Empty means independently inspected behavior, not an executed test.
    pub command: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub criteria: Vec<Criterion>,
}
impl Contract {
    pub fn validate(&self) -> Result<(), String> {
        if self.criteria.is_empty() || self.criteria.len() > 8 {
            return Err("Use 1–8 acceptance criteria.".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for c in &self.criteria {
            if c.id.trim().is_empty()
                || c.id.len() > 64
                || !ids.insert(&c.id)
                || c.description.trim().is_empty()
                || c.description.len() > 4000
                || c.command.len() > 4000
                || c.command.contains('\0')
            {
                return Err("Each criterion needs a unique ID, a description, and a bounded command (or an empty command for review).".into());
            }
        }
        if self.criteria.iter().all(|c| c.command.trim().is_empty()) {
            return Err("Include at least one executable check. Review alone cannot establish checked completion.".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Check {
    pub criterion: String,
    pub command: String,
    pub exit_code: Option<i64>,
    pub output: String,
    pub truncated: bool,
    pub cancelled: bool,
    pub duration_ms: u64,
}
impl Check {
    pub fn passed(&self) -> bool {
        self.exit_code == Some(0) && !self.cancelled
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub id: String,
    pub status: Verdict,
    pub evidence: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Pass,
    Fail,
    Blocked,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub summary: String,
    pub criteria: Vec<Finding>,
}
impl Review {
    pub fn validate(&self, contract: &Contract) -> Result<(), String> {
        let expected: std::collections::BTreeSet<_> =
            contract.criteria.iter().map(|c| c.id.as_str()).collect();
        let actual: std::collections::BTreeSet<_> =
            self.criteria.iter().map(|c| c.id.as_str()).collect();
        if expected != actual
            || actual.len() != self.criteria.len()
            || self.summary.trim().is_empty()
            || self.criteria.iter().any(|f| f.evidence.trim().is_empty())
        {
            return Err(
                "Verifier must report every criterion exactly once with concrete evidence.".into(),
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attempt {
    pub number: u8,
    pub fingerprint: Option<String>,
    pub checks: Vec<Check>,
    #[serde(default)]
    pub changes: Vec<crate::activity::RecordedFile>,
    pub review: Option<Review>,
    pub invalidated: bool,
    pub note: String,
}
impl Attempt {
    pub fn new(number: u8) -> Self {
        Self {
            number,
            fingerprint: None,
            checks: vec![],
            changes: vec![],
            review: None,
            invalidated: false,
            note: String::new(),
        }
    }
    pub fn passed(&self, contract: &Contract) -> bool {
        !self.invalidated
            && self.fingerprint.is_some()
            && contract
                .criteria
                .iter()
                .filter(|c| !c.command.trim().is_empty())
                .all(|c| {
                    self.checks
                        .iter()
                        .filter(|r| r.criterion == c.id && r.command == c.command)
                        .count()
                        == 1
                        && self
                            .checks
                            .iter()
                            .any(|r| r.criterion == c.id && r.passed())
                })
            && self.review.as_ref().is_some_and(|r| {
                r.validate(contract).is_ok() && r.criteria.iter().all(|f| f.status == Verdict::Pass)
            })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub role: Role,
    pub attempt: u8,
    pub status: String,
    pub session: Option<PathBuf>,
    pub summary: String,
    pub cost: f64,
    pub permission_cost: f64,
    #[serde(default)]
    pub selection: Option<routing::Selection>,
    #[serde(default)]
    pub routing_for: Option<Role>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub version: u32,
    pub id: String,
    pub source: PathBuf,
    #[serde(default)]
    pub parent_session: Option<PathBuf>,
    pub workspace: Option<PathBuf>,
    pub isolated: bool,
    pub provider: String,
    pub model: String,
    pub thinking: String,
    pub objective: String,
    pub constraints: String,
    pub proposed: Contract,
    /// Frozen at execution approval; agent output cannot replace this.
    pub approved: Option<Contract>,
    pub phase: Phase,
    pub note: String,
    pub dirty_source: bool,
    #[serde(default)]
    pub base_head: String,
    pub attempts: Vec<Attempt>,
    pub runs: Vec<Run>,
    pub updated: u64,
    #[serde(default)]
    pub dynamic_routing: bool,
    #[serde(default)]
    pub spend_limit: Option<f64>,
    #[serde(default)]
    pub pending_route: Option<routing::PendingRoute>,
}
impl Task {
    pub fn new(source: PathBuf, provider: String, model: String, thinking: String) -> Self {
        Self {
            version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            source,
            parent_session: None,
            workspace: None,
            isolated: false,
            provider,
            model,
            thinking,
            objective: String::new(),
            constraints: String::new(),
            proposed: Contract::default(),
            approved: None,
            phase: Phase::Draft,
            note: String::new(),
            dirty_source: false,
            base_head: String::new(),
            attempts: vec![],
            runs: vec![],
            updated: now(),
            dynamic_routing: false,
            spend_limit: None,
            pending_route: None,
        }
    }
    pub fn validate_draft(&self) -> Result<(), String> {
        if self.objective.trim().is_empty()
            || self.objective.len() > 16000
            || self.constraints.len() > 12000
        {
            return Err("Supply a bounded objective (up to 16,000 bytes) and constraints (up to 12,000 bytes).".into());
        }
        if self.provider.is_empty() || self.model.is_empty() {
            return Err(
                "Select a confirmed model in ordinary chat before creating the task.".into(),
            );
        }
        Ok(())
    }
    pub fn latest(&self) -> Option<&Attempt> {
        self.attempts.last()
    }
    pub fn invalidate(&mut self, note: &str) {
        if let Some(a) = self.attempts.last_mut() {
            a.invalidated = true;
            a.note = note.into();
        }
        self.phase = Phase::Stale;
        self.note = note.into();
    }
    pub fn cost(&self) -> f64 {
        self.runs.iter().map(|r| r.cost + r.permission_cost).sum()
    }
    pub fn recover(&mut self) {
        if self.phase.active() {
            for run in &mut self.runs {
                if matches!(run.status.as_str(), "Connecting" | "Working") {
                    run.status = "Interrupted".into();
                }
            }
            self.phase = Phase::Interrupted;
            self.note = "Pui stopped during execution. No prompts or commands were replayed. Inspect the retained workspace and start a new task if needed.".into();
        }
    }
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn json_result<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, String> {
    let text = text.trim();
    let text = if text.starts_with("```json\n") || text.starts_with("```\n") {
        text.split_once('\n')
            .and_then(|(_, s)| s.strip_suffix("```"))
            .ok_or("Unclosed JSON fence")?
            .trim()
    } else {
        text
    };
    if text.len() > 64000 {
        return Err("Agent result is too large.".into());
    }
    serde_json::from_str(text)
        .map_err(|_| "Agent did not return the required JSON. Nothing was approved.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn contract() -> Contract {
        Contract {
            criteria: vec![Criterion {
                id: "draft".into(),
                description: "Switching chats preserves the draft".into(),
                command: "cargo test draft".into(),
            }],
        }
    }
    #[test]
    fn reports_cannot_override_failed_checks_or_missing_evidence() {
        let c = contract();
        let mut a = Attempt::new(0);
        a.review = Some(Review {
            summary: "Looks good".into(),
            criteria: vec![Finding {
                id: "draft".into(),
                status: Verdict::Pass,
                evidence: "Inspected event handling".into(),
            }],
        });
        assert!(!a.passed(&c));
        a.fingerprint = Some("candidate".into());
        assert!(!a.passed(&c));
        a.checks.push(Check {
            criterion: "draft".into(),
            command: "cargo test draft".into(),
            exit_code: Some(1),
            output: "failed".into(),
            truncated: false,
            cancelled: false,
            duration_ms: 1,
        });
        assert!(!a.passed(&c));
        a.checks[0].exit_code = Some(0);
        assert!(a.passed(&c));
        a.invalidated = true;
        assert!(!a.passed(&c));
    }
    #[test]
    fn strict_contracts_and_reviews_reject_weak_or_duplicate_results() {
        assert!(contract().validate().is_ok());
        let mut c = contract();
        c.criteria[0].command.clear();
        assert!(c.validate().is_err());
        c = contract();
        c.criteria.push(c.criteria[0].clone());
        assert!(c.validate().is_err());
        let r = Review {
            summary: "Done".into(),
            criteria: vec![],
        };
        assert!(r.validate(&contract()).is_err());
        assert!(json_result::<Review>("Done!").is_err());
        assert!(json_result::<Contract>("```json\n{\"criteria\":[]}\n```").is_ok());
    }
    #[test]
    fn interrupted_work_is_not_automatically_replayed() {
        let mut t = Task::new("/tmp/project".into(), "p".into(), "m".into(), "high".into());
        for phase in [
            Phase::Proposing,
            Phase::Preparing,
            Phase::Building,
            Phase::Checking,
            Phase::Verifying,
        ] {
            t.phase = phase;
            t.recover();
            assert_eq!(t.phase, Phase::Interrupted);
        }
        t.phase = Phase::Ready;
        t.recover();
        assert_eq!(t.phase, Phase::Ready);
    }
}
