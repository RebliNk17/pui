//! Pure completion policy shared by execution, recovery and presentation.
use super::{Attempt, Contract, Task, Verdict};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Verified,
    MissingPrerequisite,
    UnknownEffect,
    EvidenceBlocked,
    RepairLimitReached,
    VerificationFailed,
    Cancelled,
    Interrupted,
    SourceChanged,
}
impl Reason {
    pub fn label(self) -> &'static str {
        match self {
            Self::Verified => "Checks and review passed",
            Self::MissingPrerequisite => "Environment prerequisite unavailable",
            Self::UnknownEffect => "Execution outcome uncertain",
            Self::EvidenceBlocked => "Behavioral evidence missing",
            Self::RepairLimitReached => "Repair budget exhausted",
            Self::VerificationFailed => "Checks or review failed",
            Self::Cancelled => "Stopped by you",
            Self::Interrupted => "Interrupted · nothing replayed",
            Self::SourceChanged => "Source changed · evidence stale",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Verified,
    Blocked(Reason),
    Repair(u8),
    Failed(Reason),
}
pub fn finish(attempt: &Attempt, contract: &Contract, recovery: bool) -> Decision {
    if attempt.passed(contract) {
        return Decision::Verified;
    }
    if attempt
        .checks
        .iter()
        .any(|c| c.cancelled || c.exit_code.is_none())
    {
        return Decision::Blocked(Reason::UnknownEffect);
    }
    if attempt.review.as_ref().is_none_or(|r| {
        r.validate(contract).is_err() || r.criteria.iter().any(|f| f.status == Verdict::Blocked)
    }) {
        return Decision::Blocked(Reason::EvidenceBlocked);
    }
    if recovery {
        return Decision::Failed(Reason::VerificationFailed);
    }
    if attempt.number >= 2 {
        Decision::Failed(Reason::RepairLimitReached)
    } else {
        Decision::Repair(attempt.number + 1)
    }
}
impl Task {
    pub fn acceptance_status(&self) -> &'static str {
        if self.execution_plan.is_some() && self.approved.is_some() {
            "Approved plan supplied"
        } else if self.approved.is_some() {
            "Established"
        } else {
            "Not established"
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{Check, Criterion, Finding, Review};
    fn candidate() -> (Attempt, Contract) {
        let contract = Contract {
            criteria: vec![Criterion {
                id: "C1".into(),
                description: "Behavior".into(),
                command: "check".into(),
            }],
        };
        let mut attempt = Attempt::new(0);
        attempt.fingerprint = Some("candidate".into());
        attempt.checks.push(Check {
            criterion: "C1".into(),
            command: "check".into(),
            exit_code: Some(1),
            output: "failed".into(),
            truncated: false,
            cancelled: false,
            duration_ms: 1,
        });
        attempt.review = Some(Review {
            summary: "Found a defect".into(),
            criteria: vec![Finding {
                id: "C1".into(),
                status: Verdict::Fail,
                evidence: "actual check".into(),
            }],
        });
        (attempt, contract)
    }
    #[test]
    fn failed_recovery_is_not_an_exhausted_repair_budget() {
        let (mut a, c) = candidate();
        assert_eq!(
            finish(&a, &c, true),
            Decision::Failed(Reason::VerificationFailed)
        );
        assert_eq!(finish(&a, &c, false), Decision::Repair(1));
        a.number = 2;
        assert_eq!(
            finish(&a, &c, false),
            Decision::Failed(Reason::RepairLimitReached)
        );
        a.checks[0].exit_code = None;
        assert_eq!(
            finish(&a, &c, false),
            Decision::Blocked(Reason::UnknownEffect)
        );
    }
    #[test]
    fn review_cannot_override_failed_missing_or_cancelled_execution() {
        let (mut a, c) = candidate();
        a.review.as_mut().unwrap().criteria[0].status = Verdict::Pass;
        assert_ne!(finish(&a, &c, false), Decision::Verified);
        a.checks[0].exit_code = Some(0);
        assert_eq!(finish(&a, &c, false), Decision::Verified);
        a.review = None;
        assert_eq!(
            finish(&a, &c, false),
            Decision::Blocked(Reason::EvidenceBlocked)
        );
    }
}
