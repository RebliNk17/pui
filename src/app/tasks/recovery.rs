//! Explicit, snapshot-bound recovery. Viewing a plan grants no replay authority.
use super::*;
use domain::recovery::{RecoveryExecution, RecoveryState};
use sha2::{Digest, Sha256};

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;

pub(in crate::app) struct Plan {
    pub summary: String,
    pub detail: String,
}
/// Only an affirmative host confirmation may consume this request.
pub(in crate::app) struct Request {
    pub summary: String,
    pub detail: String,
    expected: Task,
    record: String,
    owner: String,
    permission: settings::PermissionMode,
    selection: domain::routing::Selection,
    initial_resume: bool,
}
#[derive(Default)]
pub(super) struct State {
    claim: Option<(Request, Receiver<Result<(), String>>)>,
    active: Option<Active>,
    persisted: Option<Write>,
    preparation: Option<Write>,
    builder_start: Option<Write>,
    reconciliation: Option<Reconciliation>,
}
impl State {
    pub(super) fn cancel_pending(&mut self) {
        self.claim = None;
        self.persisted = None;
        self.preparation = None;
        self.builder_start = None;
    }
}
struct Write {
    before: Task,
    submitted: Task,
    reply: Receiver<Result<(), String>>,
}
struct Reconciliation {
    task: String,
    reply: Option<Receiver<Result<refresh::Snapshot, String>>>,
}
enum Acknowledgment {
    Definite(Result<(), String>),
    Unknown(String),
}
fn acknowledgment(write: &Option<Write>) -> Option<Acknowledgment> {
    match write.as_ref()?.reply.try_recv() {
        Ok(result) => Some(Acknowledgment::Definite(result)),
        Err(mpsc::TryRecvError::Empty) => None,
        Err(mpsc::TryRecvError::Disconnected) => Some(Acknowledgment::Unknown("Workflow persistence acknowledgment disconnected; its durable outcome must be reconciled.".into())),
    }
}
struct Active {
    task: String,
    execution: String,
    selection: domain::routing::Selection,
    expected: Task,
    owner: String,
    permission: settings::PermissionMode,
    initial_resume: bool,
}
fn record(task: &Task) -> Result<String, String> {
    serde_json::to_string(task)
        .map_err(|_| "Cannot capture the authoritative workflow record.".into())
}
/// Called by stop/block/shutdown before persistence, without touching old evidence.
pub(super) fn finish_interrupted(task: &mut Task) {
    if let Some(execution) = task.recoveries.last_mut().filter(|r| r.state.active()) {
        execution.state = match task.phase {
            Phase::Cancelled => RecoveryState::Cancelled,
            Phase::Stale => RecoveryState::Stale,
            Phase::Blocked | Phase::Interrupted => RecoveryState::Blocked,
            _ => return,
        };
        execution.note = task.note.clone();
    }
}
impl Pui {
    pub(in crate::app) fn task_recovery_plan(&self, id: &str) -> Plan {
        let Some(task) = self.tasks.records.iter().find(|task| task.id == id) else {
            return Plan {
                summary: "Workflow unavailable".into(),
                detail: "Nothing can be recovered from an unknown workflow.".into(),
            };
        };
        let candidate =
            task.workspace.is_some() && task.latest().is_some_and(|a| a.fingerprint.is_some());
        let remaining = task
            .latest()
            .map(|a| 2_u8.saturating_sub(a.number))
            .unwrap_or(2);
        Plan {
            summary: if task.phase.active() {
                "Workflow is still active"
            } else if candidate {
                "Retained candidate · explicit re-check available"
            } else {
                "No captured candidate to revalidate"
            }
            .into(),
            detail: format!(
                "Existing checks and reviews remain attached, unchanged. A separately confirmed deterministic re-check runs the frozen commands through exact-command permission prompts, then a new paid independent verifier. The retained candidate must match its captured fingerprint before and after execution and review. Recovery does not resume implementation or replay interrupted shell/tool effects automatically. It does not install dependencies, reset repairs, or run a builder. {remaining} repair round(s) remained; recovery never consumes them. Implementation changes need fresh objective-bound consent in a separate workflow. Viewing this plan starts nothing."
            ),
        }
    }
    fn task_recovery_base_guard(&self, id: &str) -> Result<&Task, String> {
        let task = self
            .tasks
            .records
            .iter()
            .find(|t| t.id == id)
            .ok_or("Unknown workflow.")?;
        if self.demo
            || self.is_mirror()
            || self.deletion.in_progress()
            || self.tasks.loading_history()
            || self.tasks.storage_failed
            || self.tasks.store.is_none()
            || self.tasks.live.is_some()
            || self.tasks.job.is_some()
            || self.tasks.revalidate_claim.is_some()
            || !self.tasks.requests.is_empty()
            || self.tasks.setup.is_some()
            || self.tasks.recovery.active.is_some()
            || self.tasks.recovery.reconciliation.is_some()
            || self.tasks.processes.iter().any(|p| !p.ended)
            || self
                .tasks
                .retiring
                .iter()
                .any(|rx| matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)))
        {
            return Err("Workflow recovery is unavailable while storage, deletion, ownership, or another execution is pending.".into());
        }
        if !self.task_in_chat(task)
            || self.readonly_worker()
            || matches!(
                self.active_permission(),
                settings::PermissionMode::Plan | settings::PermissionMode::DontAsk
            )
        {
            return Err("Use the owning writable chat in a permission mode that supports explicit command approval.".into());
        }
        if !task.initial_approved
            || task.version != 1
            || task.phase.active()
            || task.pending_route.is_some()
        {
            return Err("Only a stopped workflow with initial scoped approval can recover.".into());
        }
        task.approved
            .as_ref()
            .ok_or("No frozen checks.")?
            .validate()?;
        if domain::routing::budget_reached(task, 0.0) {
            return Err("Workflow spend threshold is already reached.".into());
        }
        Ok(task)
    }
    fn task_recovery_guard(&self, id: &str) -> Result<&Task, String> {
        let task = self.task_recovery_base_guard(id)?;
        if task.phase.active()
            || !matches!(
                task.phase,
                Phase::Blocked
                    | Phase::Interrupted
                    | Phase::Cancelled
                    | Phase::Failed
                    | Phase::Ready
                    | Phase::Accepted
            )
            || task.workspace.is_none()
            || task.pending_route.is_some()
            || task.latest().is_none_or(|a| {
                a.fingerprint.as_ref().is_none_or(|f| f.is_empty()) || a.invalidated
            })
            || task
                .recoveries
                .last()
                .is_some_and(|r| r.state.active() || r.state == RecoveryState::Stale)
        {
            return Err("Only a stopped, unchanged captured candidate from an approved workflow can be re-checked; stale and legacy workflows cannot resume.".into());
        }
        task.approved
            .as_ref()
            .ok_or("No frozen checks.")?
            .validate()?;
        if domain::routing::budget_reached(task, 0.0) {
            return Err("Workflow spend threshold is already reached.".into());
        }
        Ok(task)
    }
    pub(in crate::app) fn task_recovery_request(&self, id: &str) -> Result<Request, String> {
        if self.tasks.recovery.claim.is_some() {
            return Err("Recovery ownership confirmation is already pending.".into());
        }
        let task = self.task_recovery_guard(id)?;
        let selection = domain::routing::Selection {
            provider: task.provider.clone(),
            model: task.model.clone(),
            thinking: task.thinking.clone(),
            reason: "Separately confirmed fresh recovery verifier; pinned model, no router.".into(),
            exceptional: false,
        };
        domain::routing::validate_launch(
            task,
            self.task_routing_models(task)
                .ok_or("The owner's model catalog is unavailable.")?,
            &selection,
            false,
        )?;
        let commands = task
            .approved
            .as_ref()
            .unwrap()
            .criteria
            .iter()
            .filter(|c| !c.command.trim().is_empty())
            .map(|c| format!("{}: {}", c.id, c.command))
            .collect::<Vec<_>>()
            .join("\n");
        let detail = format!(
            "Objective: {}\nConstraints: {}\nRetained workspace: {}\nCandidate {} · {}\n\nExact check commands (each still needs normal exact-command permission):\n{}\n\nThen start ONE NEW PAID independent verifier: {} / {} / effort {}. No router is authorized for this re-check, including when the original workflow used dynamic routing ({}). Existing spend ${:.4}; original workflow stop threshold {:?} remains in force (not a billing cap).\n\nCommands may modify files; unchanged source is required after checks and review. This explicitly authorizes a NEW execution of these commands, not resumption or automatic retry of an uncertain command. No builder, setup/install, automatic repair, new repair round, commit, merge or deployment. Old evidence is retained separately.",
            task.objective,
            task.constraints,
            task.workspace.as_ref().unwrap().display(),
            task.latest().unwrap().number,
            task.latest().unwrap().fingerprint.as_ref().unwrap(),
            commands,
            selection.provider,
            selection.model,
            selection.thinking,
            task.dynamic_routing,
            task.cost(),
            task.spend_limit
        );
        Ok(Request {
            summary: "Re-run checks and fresh paid verification?".into(),
            detail,
            expected: task.clone(),
            record: record(task)?,
            owner: self.agents.active.clone(),
            permission: self.active_permission(),
            selection,
            initial_resume: false,
        })
    }
    fn task_initial_resume_guard(&self, id: &str) -> Result<&Task, String> {
        let task = self.task_recovery_base_guard(id)?;
        if !task.implementation_pending
            || task
                .prepared_fingerprint
                .as_ref()
                .is_none_or(|f| f.is_empty())
            || task.preparation_receipt.is_none()
            || task.workspace.is_none()
            || !task.isolated
            || !task.attempts.is_empty()
            || !task.recoveries.is_empty()
            || task.runs.iter().any(|r| r.role != Role::Proposal)
            || !matches!(
                task.phase,
                Phase::Blocked | Phase::Cancelled | Phase::Interrupted
            )
        {
            return Err("Cannot prove implementation effects never began. A builder or uncertain execution is never resumed.".into());
        }
        Ok(task)
    }
    pub(in crate::app) fn task_initial_resume_request(&self, id: &str) -> Result<Request, String> {
        if self.tasks.recovery.claim.is_some() {
            return Err("Recovery confirmation is pending.".into());
        }
        let task = self.task_initial_resume_guard(id)?;
        let selection = domain::routing::Selection {
            provider: task.provider.clone(),
            model: task.model.clone(),
            thinking: task.thinking.clone(),
            reason: "Explicit initial implementation resume approval; no builder previously began."
                .into(),
            exceptional: false,
        };
        domain::routing::validate_launch(
            task,
            self.task_routing_models(task)
                .ok_or("Owner model catalog unavailable.")?,
            &selection,
            false,
        )?;
        let detail = format!(
            "Objective: {}\nConstraints: {}\nRetained workspace: {}\nPrepared source fingerprint: {}\nFrozen checks: {}\n\nNo builder, check runner, or repair has started. This NEW approval starts the initial paid builder, checks and fresh independent verification, with at most the original two repair rounds. Original provider/model/effort: {} / {} / {}; dynamic routing: {}; spend so far ${:.4}; original stop threshold {:?} remains (not a billing cap). Exact-command permissions remain separate. Source must match its preparation fingerprint before implementation; ignored dependency setup may remain. No interrupted command replay, new repair budget, install, commit, merge or deployment.",
            task.objective,
            task.constraints,
            task.workspace.as_ref().unwrap().display(),
            task.prepared_fingerprint.as_ref().unwrap(),
            serde_json::to_string(task.approved.as_ref().unwrap()).unwrap(),
            task.provider,
            task.model,
            task.thinking,
            task.dynamic_routing,
            task.cost(),
            task.spend_limit
        );
        Ok(Request {
            summary: "Start initial implementation after setup?".into(),
            detail,
            expected: task.clone(),
            record: record(task)?,
            owner: self.agents.active.clone(),
            permission: self.active_permission(),
            selection,
            initial_resume: true,
        })
    }
    pub(in crate::app) fn task_confirm_initial_resume(
        &mut self,
        request: Request,
    ) -> Result<(), String> {
        if !request.initial_resume {
            return Err("Wrong recovery action.".into());
        }
        self.task_confirm_recovery(request)
    }
    /// The caller must present Request.detail and only call on affirmative consent.
    pub(in crate::app) fn task_confirm_check_rerun(
        &mut self,
        request: Request,
    ) -> Result<(), String> {
        if request.initial_resume {
            return Err("Wrong recovery action.".into());
        }
        self.task_confirm_recovery(request)
    }
    fn task_confirm_recovery(&mut self, request: Request) -> Result<(), String> {
        if self.tasks.recovery.claim.is_some() {
            return Err("A recovery claim is already pending.".into());
        }
        let task = if request.initial_resume {
            self.task_initial_resume_guard(&request.expected.id)?
        } else {
            self.task_recovery_guard(&request.expected.id)?
        };
        if request.owner != self.agents.active
            || request.permission != self.active_permission()
            || request.record != record(task)?
        {
            return Err(
                "Workflow or permission context changed since confirmation; nothing was started."
                    .into(),
            );
        }
        domain::routing::validate_launch(
            task,
            self.task_routing_models(task)
                .ok_or("Owner model catalog unavailable.")?,
            &request.selection,
            false,
        )?;
        let (reply, rx) = mpsc::channel();
        self.tasks
            .store
            .as_ref()
            .ok_or("Storage unavailable.")?
            .send(StoreCommand::ClaimCurrent {
                expected: Box::new(request.expected.clone()),
                reply,
            })
            .map_err(|_| "Storage unavailable.")?;
        self.tasks.recovery.claim = Some((request, rx));
        self.ctx.request_repaint();
        Ok(())
    }
    pub(in crate::app) fn task_recovery_active(&self) -> bool {
        self.tasks.recovery.claim.is_some()
            || self.tasks.recovery.active.is_some()
            || self.tasks.recovery.preparation.is_some()
            || self.tasks.recovery.builder_start.is_some()
            || self.tasks.recovery.reconciliation.is_some()
    }
    /// Re-check authority at async boundaries and before every command/provider launch.
    pub(super) fn task_recovery_runtime_guard(&mut self) -> bool {
        if self.tasks.recovery.reconciliation.is_some() {
            return false;
        }
        let Some(active) = self.tasks.recovery.active.as_ref() else {
            return true;
        };
        let valid = self.tasks.live_task().is_some_and(|task| {
            let mut scope = task.clone();
            // Only runtime-owned progress/evidence may evolve under this consent.
            scope.phase = active.expected.phase;
            scope.reason = active.expected.reason;
            scope.pause_requested = active.expected.pause_requested;
            scope.operations = active.expected.operations.clone();
            scope.feedback = active.expected.feedback.clone();
            scope.note = active.expected.note.clone();
            scope.runs.truncate(active.expected.runs.len());
            if !active.initial_resume {
                scope.recoveries.pop();
            } else {
                scope.implementation_resumptions.pop();
            }
            scope.updated = active.expected.updated;
            active.owner == self.agents.active
                && active.permission == self.active_permission()
                && !matches!(
                    self.active_permission(),
                    settings::PermissionMode::Plan | settings::PermissionMode::DontAsk
                )
                && !self.demo
                && !self.is_mirror()
                && !self.readonly_worker()
                && !self.tasks.storage_failed
                && !self.deletion.in_progress()
                && self.tasks.setup.is_none()
                && self.task_in_chat(task)
                && record(&scope).ok() == record(&active.expected).ok()
        });
        if !valid {
            self.tasks.block("Recovery authority changed. No command or paid verification was retried or resumed.");
        }
        valid
    }
    pub(super) fn task_initial_resume_started(&mut self) {
        if self
            .tasks
            .recovery
            .active
            .as_ref()
            .is_some_and(|a| a.initial_resume)
        {
            self.tasks.recovery.active = None;
        }
    }
    pub(super) fn task_recovery_selection_allowed(
        &mut self,
        role: Role,
        selection: &domain::routing::Selection,
        routing_for: Option<Role>,
    ) -> bool {
        let Some(active) = self.tasks.recovery.active.as_ref() else {
            return true;
        };
        let approved = &active.selection;
        let valid = self.tasks.recovery.persisted.is_none()
            && routing_for.is_none()
            && matches!(role, Role::Checks | Role::Verifier)
            && selection.provider == approved.provider
            && selection.model == approved.model
            && selection.thinking == approved.thinking
            && !selection.exceptional;
        if !valid {
            self.tasks.block("Recovery launch does not match the separately confirmed pinned verification policy.");
        }
        valid
    }
    pub(super) fn task_recovery_execution(&self) -> Option<&RecoveryExecution> {
        let active = self.tasks.recovery.active.as_ref()?;
        self.tasks
            .live_task()
            .filter(|t| t.id == active.task)?
            .recoveries
            .last()
            .filter(|r| r.id == active.execution)
    }
    pub(super) fn task_recovery_execution_mut(&mut self) -> Option<&mut RecoveryExecution> {
        let (task, execution) = self
            .tasks
            .recovery
            .active
            .as_ref()
            .map(|a| (a.task.clone(), a.execution.clone()))?;
        self.tasks
            .live_mut()
            .filter(|t| t.id == task)?
            .recoveries
            .last_mut()
            .filter(|r| r.id == execution)
    }
    pub(super) fn task_persist_preparation(&mut self, expected: Task) {
        let task = self.tasks.live_task().unwrap().clone();
        let Some(store) = self.tasks.store.as_ref() else {
            if cfg!(test) {
                self.task_environment(environment::Stage::Implementation);
            } else {
                self.tasks.block(
                    "Preparation receipt could not be durably recorded; no builder started.",
                );
            }
            return;
        };
        let (reply, rx) = mpsc::channel();
        if store
            .send(StoreCommand::SaveAcknowledged {
                expected: Box::new(expected.clone()),
                task: Box::new(task.clone()),
                reply,
            })
            .is_err()
        {
            self.task_recovery_reconcile(
                expected,
                task,
                "Preparation persistence unavailable; no builder started.".into(),
                true,
            );
            return;
        }
        self.tasks.recovery.preparation = Some(Write {
            before: expected,
            submitted: task,
            reply: rx,
        });
    }
    pub(super) fn task_persist_initial_builder(&mut self, expected: Task) {
        let task = self.tasks.live_task().unwrap().clone();
        let Some(store) = self.tasks.store.as_ref() else {
            if cfg!(test) {
                self.task_initial_resume_started();
                self.task_launch(Role::Builder);
            } else {
                self.tasks.block(
                    "Initial builder start could not be durably recorded; nothing launched.",
                );
            }
            return;
        };
        let (reply, rx) = mpsc::channel();
        if store
            .send(StoreCommand::SaveAcknowledged {
                expected: Box::new(expected.clone()),
                task: Box::new(task.clone()),
                reply,
            })
            .is_err()
        {
            self.task_recovery_reconcile(
                expected,
                task,
                "Initial builder persistence unavailable; nothing launched.".into(),
                true,
            );
            return;
        }
        self.tasks.recovery.builder_start = Some(Write {
            before: expected,
            submitted: task,
            reply: rx,
        });
    }
    pub(super) fn task_recovery_reconcile(
        &mut self,
        before: Task,
        submitted: Task,
        error: String,
        definite: bool,
    ) {
        let id = submitted.id.clone();
        // Nothing from this transition may continue, and failure handling must never
        // enqueue a stale Save over an authoritative record from another writer.
        self.tasks.retire();
        self.tasks.continuation = Default::default();
        self.tasks.job = None;
        self.tasks.live = None;
        self.tasks.recovery.cancel_pending();
        self.tasks.recovery.active = None;
        self.tasks.local_owned.remove(&id);
        self.tasks.updates.pending = None;
        self.tasks.updates.requested = false;
        if let Some(task) = self.tasks.records.iter_mut().find(|task| task.id == id) {
            if definite && record(task).ok() == record(&submitted).ok() {
                *task = before;
            }
            task.phase = Phase::Interrupted;
            task.note = format!(
                "{error} No command or paid stage was launched from this transition. The authoritative record must be reconciled before another approval; nothing was retried or saved over it."
            );
            finish_interrupted(task);
        }
        self.tasks.error = Some(error);
        let reply = self.tasks.store.as_ref().and_then(|store| {
            let (reply, rx) = mpsc::channel();
            store
                .send(StoreCommand::ReleaseCurrent {
                    id: id.clone(),
                    reply,
                })
                .ok()
                .map(|_| rx)
        });
        self.tasks.recovery.reconciliation = Some(Reconciliation { task: id, reply });
        self.ctx.request_repaint();
    }
    fn task_poll_recovery_reconciliation(&mut self) {
        let result = self.tasks.recovery.reconciliation.as_ref().and_then(|pending| pending.reply.as_ref()).and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("Authoritative workflow reconciliation disconnected; approval remains disabled.".into())),
        });
        let Some(result) = result else {
            return;
        };
        let mut pending = self.tasks.recovery.reconciliation.take().unwrap();
        match result {
            Ok(snapshot) => {
                self.tasks.local_owned.remove(&pending.task);
                self.tasks.updates.pending = None;
                self.tasks.updates.requested = true;
                if let Some(authoritative) = snapshot
                    .records
                    .into_iter()
                    .find(|task| task.id == pending.task)
                {
                    if let Some(task) = self
                        .tasks
                        .records
                        .iter_mut()
                        .find(|task| task.id == pending.task)
                    {
                        if task.parent_session != authoritative.parent_session {
                            self.tasks.parents.remove(&pending.task);
                        }
                        *task = authoritative;
                    }
                } else {
                    self.tasks.forget_deleted(&pending.task);
                }
            }
            Err(error) => {
                self.tasks.error = Some(error);
                pending.reply = None;
                self.tasks.recovery.reconciliation = Some(pending);
            }
        }
    }
    /// A reconnect/reopen can retry the read-only release/reconciliation, never the failed execution.
    pub(in crate::app) fn task_retry_recovery_reconciliation(&mut self, id: &str) -> bool {
        let Some(pending) = self
            .tasks
            .recovery
            .reconciliation
            .as_mut()
            .filter(|p| p.task == id && p.reply.is_none())
        else {
            return false;
        };
        let Some(store) = self.tasks.store.as_ref() else {
            return false;
        };
        let (reply, rx) = mpsc::channel();
        if store
            .send(StoreCommand::ReleaseCurrent {
                id: id.into(),
                reply,
            })
            .is_err()
        {
            return false;
        }
        pending.reply = Some(rx);
        self.ctx.request_repaint();
        true
    }
    pub(super) fn task_poll_recovery(&mut self) {
        self.task_poll_recovery_reconciliation();
        if let Some(result) = acknowledgment(&self.tasks.recovery.builder_start) {
            let write = self.tasks.recovery.builder_start.take().unwrap();
            let unchanged = self
                .tasks
                .live_task()
                .is_some_and(|task| record(task).ok() == record(&write.submitted).ok());
            let context_valid = !self.demo
                && !self.is_mirror()
                && !self.readonly_worker()
                && !self.deletion.in_progress()
                && !matches!(
                    self.active_permission(),
                    settings::PermissionMode::Plan | settings::PermissionMode::DontAsk
                )
                && self.tasks.recovery.active.as_ref().is_none_or(|active| {
                    active.owner == self.agents.active
                        && active.permission == self.active_permission()
                });
            match result {
                Acknowledgment::Definite(Ok(()))
                    if unchanged && context_valid && !self.tasks.storage_failed =>
                {
                    self.task_initial_resume_started();
                    self.task_launch(Role::Builder);
                }
                result => {
                    let (error, definite) = match result {
                        Acknowledgment::Definite(Err(error)) => (error, true),
                        Acknowledgment::Unknown(error) => (error, false),
                        _ => (
                            "Workflow changed before initial builder start; nothing launched."
                                .into(),
                            false,
                        ),
                    };
                    self.task_recovery_reconcile(write.before, write.submitted, error, definite);
                    return;
                }
            }
        }
        if let Some(result) = acknowledgment(&self.tasks.recovery.preparation) {
            let write = self.tasks.recovery.preparation.take().unwrap();
            let unchanged = self
                .tasks
                .live_task()
                .is_some_and(|task| record(task).ok() == record(&write.submitted).ok());
            match result {
                Acknowledgment::Definite(Ok(())) if unchanged && !self.tasks.storage_failed => {
                    self.task_environment(environment::Stage::Implementation)
                }
                result => {
                    let (error, definite) = match result {
                        Acknowledgment::Definite(Err(error)) => (error, true),
                        Acknowledgment::Unknown(error) => (error, false),
                        _ => (
                            "Prepared workflow changed before implementation; nothing launched."
                                .into(),
                            false,
                        ),
                    };
                    self.task_recovery_reconcile(write.before, write.submitted, error, definite);
                    return;
                }
            }
        }
        if let Some(result) = acknowledgment(&self.tasks.recovery.persisted) {
            let write = self.tasks.recovery.persisted.take().unwrap();
            match result {
                Acknowledgment::Definite(Ok(()))
                    if self.tasks.recovery.active.is_some()
                        && self.task_recovery_runtime_guard() =>
                {
                    let initial_resume =
                        self.tasks.recovery.active.as_ref().unwrap().initial_resume;
                    self.task_fingerprint(if initial_resume {
                        FingerprintStep::InitialResumeBeforeBuilder
                    } else {
                        FingerprintStep::RecoveryBeforeChecks
                    });
                }
                Acknowledgment::Definite(Ok(())) => {}
                result => {
                    let (error, definite) = match result {
                        Acknowledgment::Definite(Err(error)) => (error, true),
                        Acknowledgment::Unknown(error) => (error, false),
                        _ => unreachable!(),
                    };
                    self.task_recovery_reconcile(write.before, write.submitted, error, definite);
                    return;
                }
            }
        }
        if self
            .tasks
            .recovery
            .active
            .as_ref()
            .is_some_and(|a| self.tasks.live.as_ref() != Some(&a.task))
        {
            if let Some(active) = self.tasks.recovery.active.take() {
                if let Some(task) = self.tasks.records.iter_mut().find(|t| t.id == active.task) {
                    finish_interrupted(task);
                }
            }
        }
        let result = self
            .tasks
            .recovery
            .claim
            .as_ref()
            .and_then(|(_, rx)| match rx.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Recovery ownership check disconnected.".into()))
                }
            });
        let Some(result) = result else {
            return;
        };
        let (request, _) = self.tasks.recovery.claim.take().unwrap();
        let checked = result.and_then(|_| {
            let task = if request.initial_resume {
                self.task_initial_resume_guard(&request.expected.id)?
            } else {
                self.task_recovery_guard(&request.expected.id)?
            };
            if request.owner != self.agents.active
                || request.permission != self.active_permission()
                || request.record != record(task)?
            {
                return Err(
                    "Workflow changed while acquiring ownership; nothing was executed.".into(),
                );
            }
            domain::routing::validate_launch(
                task,
                self.task_routing_models(task)
                    .ok_or("Owner model catalog unavailable.")?,
                &request.selection,
                false,
            )
        });
        if let Err(error) = checked {
            let submitted = self
                .tasks
                .records
                .iter()
                .find(|task| task.id == request.expected.id)
                .cloned()
                .unwrap_or_else(|| request.expected.clone());
            self.task_recovery_reconcile(request.expected, submitted, error, false);
            return;
        }
        let id = request.expected.id.clone();
        let execution = uuid::Uuid::new_v4().to_string();
        let hash = crate::hex::lower(Sha256::digest(request.record.as_bytes()));
        let task = self.tasks.records.iter_mut().find(|t| t.id == id).unwrap();
        if !request.initial_resume {
            task.recoveries.push(RecoveryExecution {
                id: execution.clone(),
                candidate_attempt: task.latest().unwrap().number,
                fingerprint: task.latest().unwrap().fingerprint.clone().unwrap(),
                checks: vec![],
                review: None,
                state: RecoveryState::Checking,
                note: "Explicit re-check confirmed; fingerprint validation pending.".into(),
                runs: vec![],
                confirmed_record: hash.clone(),
                consent: request.detail.clone(),
            });
        }
        if request.initial_resume {
            task.implementation_resumptions.push(format!(
                "Confirmed authoritative record SHA-256 {hash}\n{}",
                request.detail
            ));
        }
        task.pause_requested = false;
        task.phase = if request.initial_resume {
            Phase::Preparing
        } else {
            Phase::Checking
        };
        task.note = request.detail;
        self.tasks.recovery.active = Some(Active {
            task: id.clone(),
            execution,
            selection: request.selection,
            expected: request.expected,
            owner: request.owner,
            permission: request.permission,
            initial_resume: request.initial_resume,
        });
        self.tasks.live = Some(id);
        let (reply, rx) = mpsc::channel();
        let task = self.tasks.live_task().unwrap().clone();
        if self
            .tasks
            .store
            .as_ref()
            .unwrap()
            .send(StoreCommand::SaveAcknowledged {
                expected: Box::new(
                    self.tasks
                        .recovery
                        .active
                        .as_ref()
                        .unwrap()
                        .expected
                        .clone(),
                ),
                task: Box::new(task.clone()),
                reply,
            })
            .is_err()
        {
            let before = self
                .tasks
                .recovery
                .active
                .as_ref()
                .unwrap()
                .expected
                .clone();
            self.task_recovery_reconcile(
                before,
                task,
                "Cannot durably record recovery consent; nothing was executed.".into(),
                true,
            );
            return;
        }
        self.tasks.recovery.persisted = Some(Write {
            before: self
                .tasks
                .recovery
                .active
                .as_ref()
                .unwrap()
                .expected
                .clone(),
            submitted: task,
            reply: rx,
        });
    }
    pub(super) fn task_recovery_fingerprinted(&mut self, step: FingerprintStep, fingerprint: &str) {
        if !self.task_recovery_runtime_guard() {
            return;
        }
        if self
            .tasks
            .recovery
            .active
            .as_ref()
            .is_some_and(|a| a.initial_resume)
        {
            if step != FingerprintStep::InitialResumeBeforeBuilder {
                return;
            }
            let expected = self
                .tasks
                .recovery
                .active
                .as_ref()
                .unwrap()
                .expected
                .prepared_fingerprint
                .as_deref();
            if expected != Some(fingerprint) {
                self.tasks.block("Prepared candidate changed. Initial implementation was not started; no uncertain effects were replayed.");
                return;
            }
            self.task_environment(environment::Stage::Implementation);
            return;
        }
        let Some(execution) = self.task_recovery_execution() else {
            return;
        };
        if execution.fingerprint != fingerprint {
            let note = "Retained candidate changed. Recovery evidence is stale; no verifier, builder, automatic repair or replay was started.";
            let task = self.tasks.live_mut().unwrap();
            task.phase = Phase::Stale;
            task.note = note.into();
            finish_interrupted(task);
            self.tasks.save();
            self.tasks.retire();
            self.tasks.live = None;
            return;
        }
        match step {
            FingerprintStep::RecoveryBeforeChecks => {
                self.task_environment(environment::Stage::Checks)
            }
            FingerprintStep::RecoveryAfterChecks => {
                let selection = self
                    .tasks
                    .recovery
                    .active
                    .as_ref()
                    .unwrap()
                    .selection
                    .clone();
                self.task_recovery_execution_mut().unwrap().state = RecoveryState::Verifying;
                self.tasks.live_mut().unwrap().phase = Phase::Verifying;
                self.tasks.save();
                self.task_launch_selected(Role::Verifier, selection, None, None);
            }
            FingerprintStep::RecoveryAfterReview => {
                let task = self.tasks.live_mut().unwrap();
                let execution = task.recoveries.last_mut().unwrap();
                let pass = execution.checks_and_review_passed(task.approved.as_ref().unwrap());
                execution.state = if pass {
                    RecoveryState::Passed
                } else {
                    RecoveryState::Failed
                };
                execution.note = if pass { "Explicit checks and fresh independent verification passed for the unchanged retained candidate." } else { "Explicit recovery checks/review did not pass. No builder or automatic repair was started; repair budget and old evidence are unchanged." }.into();
                task.phase = if pass { Phase::Ready } else { Phase::Failed };
                task.reason = Some(if pass {
                    domain::decision::Reason::Verified
                } else {
                    domain::decision::Reason::VerificationFailed
                });
                task.note = execution.note.clone();
                self.tasks.save();
                self.tasks.retire();
                self.tasks.live = None;
            }
            _ => unreachable!(),
        }
    }
}
