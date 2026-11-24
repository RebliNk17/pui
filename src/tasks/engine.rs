//! Durable semantic operations. Persist dispatch intent before effects. A crash
//! between intent and result is unknown, never permission for exactly-once replay.
use super::{Role, Task};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Dispatching,
    Completed,
    UnknownEffect,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Operation {
    pub version: u32,
    pub id: String,
    pub run: String,
    pub role: Role,
    pub method: String,
    pub input_hash: String,
    pub candidate: Option<String>,
    pub state: State,
    pub created: u64,
}
impl Task {
    pub fn stage_operation(
        &mut self,
        run: &str,
        role: Role,
        method: &str,
        input: &[u8],
    ) -> Result<String, String> {
        use sha2::{Digest, Sha256};
        if self.operations.len() >= 128
            || !matches!(method, "prompt" | "bash")
            || self.operations.iter().any(|op| {
                op.state == State::Dispatching
                    && !(role == Role::Checks
                        && self.phase == super::Phase::Building
                        && op.role == Role::Builder)
            })
            || self
                .operations
                .iter()
                .any(|op| op.state == State::UnknownEffect)
                && !self.recoveries.last().is_some_and(|execution| {
                    execution.state.active() && execution.runs.iter().any(|id| id == run)
                })
        {
            return Err("Workflow has an unresolved operation or reached its operation bound; inspect retained evidence. Nothing was replayed.".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.operations.push(Operation {
            version: 1,
            id: id.clone(),
            run: run.into(),
            role,
            method: method.into(),
            input_hash: crate::hex::lower(Sha256::digest(input)),
            candidate: self.latest().and_then(|a| a.fingerprint.clone()),
            state: State::Dispatching,
            created: super::now(),
        });
        Ok(id)
    }
    pub fn complete_operation(&mut self, id: &str, run: &str) -> Result<(), String> {
        let operation = self
            .operations
            .iter_mut()
            .rev()
            .find(|op| op.id == id && op.run == run && op.state == State::Dispatching)
            .ok_or("Late or mismatched workflow result; no transition was authorized.")?;
        operation.state = State::Completed;
        Ok(())
    }
    pub fn interrupt_operations(&mut self) {
        for op in &mut self.operations {
            if op.state == State::Dispatching {
                op.state = State::UnknownEffect;
            }
        }
    }
    pub fn validate_operations(&self) -> Result<(), String> {
        let mut ids = std::collections::BTreeSet::new();
        if self.operations.len() > 128
            || self.operations.iter().any(|op| {
                op.version != 1
                    || uuid::Uuid::parse_str(&op.id).is_err()
                    || !ids.insert(&op.id)
                    || !matches!(op.method.as_str(), "prompt" | "bash")
                    || op.input_hash.len() != 64
                    || !op.input_hash.bytes().all(|b| b.is_ascii_hexdigit())
                    || !self
                        .runs
                        .iter()
                        .any(|run| run.id == op.run && run.role == op.role)
            })
        {
            return Err("Invalid workflow operation receipts; execution is unavailable.".into());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_correlated_result_completes_and_restart_never_replays_uncertain_effect() {
        let mut task = Task::new("/fixture".into(), "p".into(), "m".into(), "off".into());
        let op = task
            .stage_operation("run", Role::Builder, "prompt", b"work")
            .unwrap();
        assert!(
            task.stage_operation("run", Role::Builder, "prompt", b"again")
                .is_err()
        );
        assert!(task.complete_operation(&op, "another-run").is_err());
        task.interrupt_operations();
        assert_eq!(task.operations[0].state, State::UnknownEffect);
        assert!(task.complete_operation(&op, "run").is_err());
        assert!(
            task.stage_operation("new", Role::Builder, "prompt", b"replay")
                .is_err()
        );
        assert!(task.operations[0].input_hash.len() == 64);
    }
    #[test]
    fn completed_receipts_survive_while_duplicate_results_are_rejected() {
        let mut task = Task::new("/fixture".into(), "p".into(), "m".into(), "off".into());
        let op = task
            .stage_operation("run", Role::Checks, "bash", b"test")
            .unwrap();
        task.complete_operation(&op, "run").unwrap();
        assert!(task.complete_operation(&op, "run").is_err());
        task.interrupt_operations();
        assert_eq!(task.operations[0].state, State::Completed);
        assert!(
            task.stage_operation("next", Role::Verifier, "prompt", b"review")
                .is_ok()
        );
    }
}
