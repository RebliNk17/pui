//! Append-only re-check evidence for an existing candidate, never a builder attempt.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryState {
    Checking,
    Verifying,
    Passed,
    Failed,
    Blocked,
    Cancelled,
    Stale,
}
impl RecoveryState {
    pub fn active(self) -> bool {
        matches!(self, Self::Checking | Self::Verifying)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoveryExecution {
    pub id: String,
    pub candidate_attempt: u8,
    pub fingerprint: String,
    pub checks: Vec<Check>,
    pub review: Option<Review>,
    pub state: RecoveryState,
    pub note: String,
    pub runs: Vec<String>,
    /// SHA-256 of the complete authoritative record confirmed by the user.
    pub confirmed_record: String,
    pub consent: String,
}
impl Task {
    /// Recovery evidence supersedes older checks without rewriting builder history.
    pub fn current_evidence(&self) -> Option<Attempt> {
        self.recoveries
            .last()
            .map(RecoveryExecution::evidence)
            .or_else(|| self.latest().cloned())
    }
}
impl RecoveryExecution {
    pub fn evidence(&self) -> Attempt {
        Attempt {
            number: self.candidate_attempt,
            fingerprint: Some(self.fingerprint.clone()),
            checks: self.checks.clone(),
            changes: vec![],
            review: self.review.clone(),
            invalidated: self.state != RecoveryState::Passed,
            note: self.note.clone(),
        }
    }
    #[cfg(test)]
    pub fn passed(&self, contract: &Contract) -> bool {
        self.state == RecoveryState::Passed && self.evidence().passed(contract)
    }
    pub fn checks_and_review_passed(&self, contract: &Contract) -> bool {
        let mut evidence = self.evidence();
        evidence.invalidated = false;
        evidence.passed(contract)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_or_unfinished_recovery_never_reuses_historical_passing_evidence() {
        let contract = Contract {
            criteria: vec![Criterion {
                id: "C1".into(),
                description: "Observable fixture".into(),
                command: "test fixture".into(),
            }],
        };
        let mut attempt = Attempt::new(2);
        attempt.fingerprint = Some("same-candidate".into());
        attempt.checks.push(Check {
            criterion: "C1".into(),
            command: "test fixture".into(),
            exit_code: Some(0),
            output: "passed earlier".into(),
            truncated: false,
            cancelled: false,
            duration_ms: 1,
        });
        attempt.review = Some(Review {
            summary: "Earlier independent pass".into(),
            criteria: vec![Finding {
                id: "C1".into(),
                status: Verdict::Pass,
                evidence: "old inspection".into(),
            }],
        });
        assert!(attempt.passed(&contract));
        let mut task = Task::new(
            "/retained".into(),
            "provider".into(),
            "model".into(),
            "high".into(),
        );
        task.approved = Some(contract.clone());
        task.phase = Phase::Ready;
        task.attempts.push(attempt.clone());
        for state in [
            RecoveryState::Checking,
            RecoveryState::Verifying,
            RecoveryState::Failed,
            RecoveryState::Blocked,
            RecoveryState::Cancelled,
            RecoveryState::Stale,
        ] {
            task.recoveries = vec![RecoveryExecution {
                id: "new-recovery".into(),
                candidate_attempt: 2,
                fingerprint: "same-candidate".into(),
                checks: attempt.checks.clone(),
                review: attempt.review.clone(),
                state,
                note: "not accepted".into(),
                runs: vec![],
                confirmed_record: "hash".into(),
                consent: "explicit separate consent".into(),
            }];
            assert!(task.latest().unwrap().passed(&contract));
            assert!(
                !task.current_evidence().unwrap().passed(&contract),
                "{state:?}"
            );
            assert_eq!(task.current_evidence().unwrap().number, 2);
            assert_eq!(task.attempts.len(), 1);
        }
        task.recoveries[0].state = RecoveryState::Passed;
        assert!(task.current_evidence().unwrap().passed(&contract));
    }
}
