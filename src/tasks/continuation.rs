//! Safe continuation starts a NEW authorization from an unchanged completed
//! boundary. It never resumes an unfinished builder or replays unknown effects.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Boundary {
    Checks,
    Review,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Continuation {
    pub id: String,
    pub boundary: Boundary,
    pub fingerprint: String,
    pub confirmed_record: String,
    pub created: u64,
}
impl Task {
    pub fn continuation_boundary(&self) -> Result<Boundary, String> {
        if !matches!(
            self.phase,
            Phase::Interrupted | Phase::Blocked | Phase::Cancelled
        ) || self.version != 1
            || self.continuations.len() >= 16
            || !self.initial_approved
            || self.pending_route.is_some()
            || !self.recoveries.is_empty()
            || self
                .operations
                .iter()
                .any(|op| op.state != engine::State::Completed)
        {
            return Err("No safe completed continuation boundary. Inspect uncertain effects; they will not replay.".into());
        }
        let contract = self.approved.as_ref().ok_or("No frozen checks.")?;
        contract.validate()?;
        let attempt = self.latest().ok_or("No captured candidate.")?;
        if attempt.invalidated || attempt.fingerprint.is_none() || self.workspace.is_none() {
            return Err("Candidate evidence is unavailable or stale.".into());
        }
        let executable = contract
            .criteria
            .iter()
            .filter(|c| !c.command.trim().is_empty())
            .collect::<Vec<_>>();
        if attempt.checks.len() == executable.len()
            && executable.iter().all(|c| {
                attempt
                    .checks
                    .iter()
                    .filter(|r| r.criterion == c.id && r.command == c.command && r.passed())
                    .count()
                    == 1
            })
        {
            return Ok(Boundary::Review);
        }
        if attempt.checks.is_empty()
            && attempt.review.is_none()
            && !self.runs.iter().any(|run| {
                run.attempt == attempt.number && matches!(run.role, Role::Checks | Role::Verifier)
            })
        {
            return Ok(Boundary::Checks);
        }
        Err("Partial/failed checks require an explicit check rerun, not continuation.".into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uncertainty_or_partial_checks_never_become_safe_continuation() {
        let mut task = Task::new("/fixture".into(), "p".into(), "m".into(), "off".into());
        task.initial_approved = true;
        task.workspace = Some("/fixture".into());
        task.phase = Phase::Interrupted;
        task.approved = Some(Contract {
            criteria: vec![Criterion {
                id: "C1".into(),
                description: "behavior".into(),
                command: "test".into(),
            }],
        });
        let mut a = Attempt::new(0);
        a.fingerprint = Some("source".into());
        task.attempts.push(a);
        assert_eq!(task.continuation_boundary().unwrap(), Boundary::Checks);
        task.attempts[0].checks.push(Check {
            criterion: "C1".into(),
            command: "test".into(),
            exit_code: Some(0),
            output: "pass".into(),
            truncated: false,
            cancelled: false,
            duration_ms: 1,
        });
        assert_eq!(task.continuation_boundary().unwrap(), Boundary::Review);
        let passing = task.clone();
        let mut uncertain = passing.clone();
        uncertain
            .stage_operation("run", Role::Builder, "prompt", b"work")
            .unwrap();
        uncertain.interrupt_operations();
        assert!(uncertain.continuation_boundary().is_err());
        let mut legacy = passing;
        legacy.initial_approved = false;
        assert!(legacy.continuation_boundary().is_err());
        task.attempts[0].checks[0].exit_code = None;
        assert!(task.continuation_boundary().is_err());
    }
}
