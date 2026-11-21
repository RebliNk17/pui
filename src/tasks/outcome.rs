//! Model-facing terminal handoff. Tool completion is not evidence of success.
use super::*;
use serde_json::{Value, json};

pub fn summary(task: &Task) -> Value {
    let evidence = task.current_evidence();
    let verified = matches!(task.phase, Phase::Ready | Phase::Accepted)
        && task
            .approved
            .as_ref()
            .zip(evidence.as_ref())
            .is_some_and(|(contract, attempt)| attempt.passed(contract));
    let running = task.phase.active();
    let instruction = if running {
        "Workflow is still running, not verified. Use an event-driven wait when needed; do not poll or mutate its source/candidate concurrently."
    } else if verified {
        "Report the verified candidate and concrete checks/review. Clearly say isolated changes remain in the retained workspace unless a separate application is confirmed. Workflow success does not authorize applying, committing, merging or deploying."
    } else {
        "Report this as NOT VERIFIED, with the blocker or missing evidence. A completed tool call or approved request is not success. Do not automatically retry, replay effects, or claim the original checkout was updated."
    };
    let attempts = task.attempts.iter().map(|attempt| json!({
        "number":attempt.number,"fingerprint":attempt.fingerprint,"invalidated":attempt.invalidated,
        "checks":attempt.checks.iter().map(check_summary).collect::<Vec<_>>(),"review":attempt.review,"note":attempt.note
    })).collect::<Vec<_>>();
    json!({
        "workflow_id":task.id,"approved":true,"phase":task.phase.label(),"reason":task.reason,"verified":verified,
        "outcome":if running {"running"} else if verified {"verified_candidate"} else {"not_verified"}, "note":task.note,"pause_requested":task.pause_requested,
        "workspace":task.workspace,"isolated":task.isolated,
        "application_status":if task.isolated {"separate_confirmation_required_not_established_by_workflow_completion"} else {"in_place_edits_may_already_exist"},
        "reported_cost":task.cost(),"profile":task.profile,"operations":task.operations,"continuations":task.continuations,"execution_plan":task.execution_plan,"checks":task.approved,"attempts":attempts,
        "current_evidence":evidence.as_ref().map(|attempt| json!({"number":attempt.number,"fingerprint":attempt.fingerprint,"invalidated":attempt.invalidated,
            "checks":attempt.checks.iter().map(check_summary).collect::<Vec<_>>(),"review":attempt.review,"note":attempt.note})),
        "instruction":instruction
    })
}
fn check_summary(check: &Check) -> Value {
    json!({"criterion":check.criterion,"command":check.command,"exit_code":check.exit_code,"cancelled":check.cancelled,
        "output":check.output.chars().take(2000).collect::<String>(),"truncated":check.truncated || check.output.chars().count()>2000})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_or_approved_never_implies_verified_and_output_remains_bounded() {
        let mut task = Task::new(
            "/fixture".into(),
            "provider".into(),
            "model".into(),
            "high".into(),
        );
        task.initial_approved = true;
        for phase in [
            Phase::Ready,
            Phase::Accepted,
            Phase::Blocked,
            Phase::Failed,
            Phase::Cancelled,
            Phase::Interrupted,
            Phase::Stale,
        ] {
            task.phase = phase;
            let result = summary(&task);
            assert_eq!(result["approved"], true);
            assert_eq!(result["verified"], false);
            assert_eq!(result["outcome"], "not_verified");
            assert!(
                result["instruction"]
                    .as_str()
                    .unwrap()
                    .contains("NOT VERIFIED")
            );
        }
        let check = Check {
            criterion: "C1".into(),
            command: "cargo test".into(),
            exit_code: Some(0),
            output: "x".repeat(4000),
            truncated: false,
            cancelled: false,
            duration_ms: 1,
        };
        assert_eq!(
            check_summary(&check)["output"].as_str().unwrap().len(),
            2000
        );
        assert_eq!(check_summary(&check)["truncated"], true);
    }
}

#[cfg(test)]
mod evidence_tests {
    use super::*;
    #[test]
    fn fresh_passing_evidence_is_explicit_and_new_failed_recovery_supersedes_it() {
        let mut task = Task::new("/fixture".into(), "p".into(), "m".into(), "high".into());
        let contract = Contract {
            criteria: vec![Criterion {
                id: "C1".into(),
                description: "Observable behavior".into(),
                command: "cargo test".into(),
            }],
        };
        let mut attempt = Attempt::new(0);
        attempt.fingerprint = Some("captured".into());
        attempt.checks.push(Check {
            criterion: "C1".into(),
            command: "cargo test".into(),
            exit_code: Some(0),
            output: "passed".into(),
            truncated: false,
            cancelled: false,
            duration_ms: 1,
        });
        attempt.review = Some(Review {
            summary: "Independent review passed".into(),
            criteria: vec![Finding {
                id: "C1".into(),
                status: Verdict::Pass,
                evidence: "Inspected regression".into(),
            }],
        });
        task.approved = Some(contract);
        task.attempts.push(attempt.clone());
        task.phase = Phase::Ready;
        task.isolated = true;
        assert_eq!(summary(&task)["verified"], true);
        assert_eq!(summary(&task)["outcome"], "verified_candidate");
        assert!(
            summary(&task)["application_status"]
                .as_str()
                .unwrap()
                .contains("separate_confirmation")
        );
        task.recoveries.push(recovery::RecoveryExecution {
            id: "recheck".into(),
            candidate_attempt: 0,
            fingerprint: "captured".into(),
            checks: attempt.checks,
            review: attempt.review,
            state: recovery::RecoveryState::Failed,
            note: "New review failed".into(),
            runs: vec![],
            confirmed_record: "record".into(),
            consent: "reviewed".into(),
        });
        assert_eq!(summary(&task)["verified"], false);
        assert_eq!(
            summary(&task)["current_evidence"]["note"],
            "New review failed"
        );
    }
}
