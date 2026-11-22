//! Explicit workflow operations. Navigation alone never starts commands or writes.
use super::*;

#[path = "operations_poll.rs"]
mod polling;
#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;
#[path = "operations_ui.rs"]
mod ui;

pub(super) enum Decision {
    Continue {
        expected: Task,
        boundary: domain::continuation::Boundary,
        detail: String,
    },
    Rerun(recovery::Request),
    Resume(recovery::Request),
    Setup {
        expected: Task,
        plan: setup::Plan,
        commands: String,
    },
    Apply {
        expected: Task,
        plan: domain::apply::Plan,
    },
    RollBack {
        expected: Task,
        transaction: String,
    },
}
pub(super) enum ReadResult {
    Setup(Task, setup::Plan),
    SetupReady(Task, setup::Plan),
    Recovery(Vec<domain::apply::RecoveryStatus>),
}
struct Save {
    task: Task,
    reply: Receiver<Result<(), String>>,
}
#[derive(Default)]
pub(super) struct State {
    pub(super) decision: Option<Decision>,
    reading: Option<(String, Receiver<Result<ReadResult, String>>)>,
    pub(super) review_scope: Option<(String, Option<String>, settings::PermissionMode)>,
    applying: Option<(
        String,
        Receiver<Result<domain::apply::ApplyOutcome, String>>,
    )>,
    saving: Option<Save>,
    reconciling: Option<(String, Receiver<Result<refresh::Snapshot, String>>)>,
    pub(super) quarantined: std::collections::BTreeSet<String>,
    setup_baseline: Option<Task>,
    setup_owner: Option<(String, Option<String>, settings::PermissionMode)>,
    pub(super) status: Option<(String, String)>,
    pub(super) transactions: Vec<domain::apply::RecoveryStatus>,
    pub(super) transactions_task: Option<String>,
    environment: Option<(String, Receiver<String>)>,
}
impl State {
    pub(super) fn busy(&self) -> bool {
        self.reading.is_some()
            || self.applying.is_some()
            || self.saving.is_some()
            || self.reconciling.is_some()
    }
    pub(super) fn confirming(&self) -> bool {
        self.decision.is_some()
    }
}
impl Pui {
    pub(super) fn operation_scope(&self) -> (String, Option<String>, settings::PermissionMode) {
        (
            self.agents.active.clone(),
            self.state.session_file.clone(),
            self.active_permission(),
        )
    }
    fn setup_scope_valid(&self) -> bool {
        self.tasks
            .operations
            .setup_owner
            .as_ref()
            .is_some_and(|scope| *scope == self.operation_scope())
            && !self.demo
            && !self.is_mirror()
            && !self.readonly_worker()
            && !self.deletion.in_progress()
            && !self.tasks.storage_failed
            && self.tasks.live.is_none()
            && self.tasks.job.is_none()
    }
    pub(super) fn task_operation_guard(&self, expected: &Task) -> Result<(), String> {
        if self.demo || self.is_mirror() || self.readonly_worker() || self.deletion.in_progress() {
            return Err(
                "A writable owning conversation is required; no workflow action started.".into(),
            );
        }
        if matches!(
            self.active_permission(),
            settings::PermissionMode::Plan | settings::PermissionMode::DontAsk
        ) {
            return Err("Current permission mode does not authorize workflow actions.".into());
        }
        if self.tasks.loading_history()
            || self.tasks.storage_failed
            || self.tasks.store.is_none()
            || self.tasks.live.is_some()
            || self.tasks.job.is_some()
            || self.tasks.revalidate_claim.is_some()
            || self.tasks.attachment.pending_id().is_some()
            || !self.tasks.requests.is_empty()
            || self.tasks.processes.iter().any(|process| !process.ended)
            || self
                .tasks
                .retiring
                .iter()
                .any(|reply| matches!(reply.try_recv(), Err(mpsc::TryRecvError::Empty)))
            || self.task_recovery_active()
            || self.tasks.setup.is_some()
            || self.tasks.operations.busy()
            || self.tasks.continuation.pending.is_some()
        {
            return Err("Wait for workflow work, storage and pending actions to finish.".into());
        }
        if self.tasks.operations.quarantined.contains(&expected.id) {
            return Err("Workflow persistence is uncertain. Reconcile retained history before another action.".into());
        }
        let current = self
            .tasks
            .records
            .iter()
            .find(|task| task.id == expected.id)
            .ok_or("Workflow is no longer available.")?;
        if serde_json::to_value(current).ok() != serde_json::to_value(expected).ok() {
            return Err("Workflow changed; review the current record before acting.".into());
        }
        if !self.task_in_chat(current) {
            return Err("Open the owning conversation before acting on this workflow.".into());
        }
        Ok(())
    }
    pub(super) fn task_operations_busy(&self) -> bool {
        self.tasks.operations.busy()
            || self.tasks.setup.is_some()
            || self.task_recovery_active()
            || self.tasks.continuation.pending.is_some()
    }
    pub(in crate::app) fn task_operations_confirming(&self) -> bool {
        self.tasks.operations.confirming()
    }
    pub(super) fn request_check_rerun(&mut self, id: &str) {
        let result = self
            .tasks
            .records
            .iter()
            .find(|t| t.id == id)
            .cloned()
            .ok_or("Workflow unavailable.".into())
            .and_then(|task| {
                self.task_operation_guard(&task)?;
                self.task_recovery_request(id)
            });
        match result {
            Ok(request) => self.tasks.operations.decision = Some(Decision::Rerun(request)),
            Err(error) => self.state.notice = Some(error),
        }
    }
    pub(super) fn request_initial_resume(&mut self, id: &str) {
        let result = self
            .tasks
            .records
            .iter()
            .find(|t| t.id == id)
            .cloned()
            .ok_or("Workflow unavailable.".into())
            .and_then(|task| {
                self.task_operation_guard(&task)?;
                self.task_initial_resume_request(id)
            });
        match result {
            Ok(request) => self.tasks.operations.decision = Some(Decision::Resume(request)),
            Err(error) => self.state.notice = Some(error),
        }
    }
    pub(super) fn request_environment_setup(&mut self, id: &str) {
        let result = self
            .tasks
            .records
            .iter()
            .find(|t| t.id == id)
            .cloned()
            .ok_or("Workflow unavailable.".into())
            .and_then(|task| {
                self.task_operation_guard(&task)?;
                Ok(task)
            });
        let task = match result {
            Ok(task) => task,
            Err(error) => {
                self.state.notice = Some(error);
                return;
            }
        };
        let id = task.id.clone();
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        self.tasks.operations.review_scope = Some(self.operation_scope());
        self.tasks.operations.reading = Some((id, rx));
        std::thread::spawn(move || {
            let result = setup::plan(&task).map(|plan| ReadResult::Setup(task, plan));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    pub(super) fn request_apply(&mut self, task: &Task, plan: &domain::apply::Plan) {
        if let Err(error) = self.task_operation_guard(task) {
            self.state.notice = Some(error);
            return;
        }
        if !plan.blockers.is_empty() {
            self.state.notice = Some(plan.blockers.join("\n"));
            return;
        }
        self.tasks.operations.review_scope = Some(self.operation_scope());
        self.tasks.operations.decision = Some(Decision::Apply {
            expected: task.clone(),
            plan: plan.clone(),
        });
    }
    pub(super) fn request_apply_recovery(&mut self, task: &Task) {
        if let Err(error) = self.task_operation_guard(task) {
            self.state.notice = Some(error);
            return;
        }
        let Some(home) = domain::store::home() else {
            return;
        };
        let task = task.clone();
        let id = task.id.clone();
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        self.tasks.operations.reading = Some((id, rx));
        std::thread::spawn(move || {
            let _ =
                tx.send(domain::apply::inspect_recovery(&home, &task).map(ReadResult::Recovery));
            ctx.request_repaint();
        });
    }
    pub(in crate::app) fn task_operation_confirmation(&mut self, ctx: &egui::Context) {
        let Some(mut decision) = self.tasks.operations.decision.take() else {
            return;
        };
        let (title, detail, label, danger) = match &decision {
            Decision::Continue { detail, .. } => (
                "Continue from this completed boundary?",
                detail.clone(),
                "Continue workflow",
                false,
            ),
            Decision::Rerun(request) => (
                "Rerun these checks?",
                format!("{}\n{}", request.summary, request.detail),
                "Rerun checks and review",
                false,
            ),
            Decision::Resume(request) => (
                "Start initial implementation?",
                format!("{}\n{}", request.summary, request.detail),
                "Start initial implementation",
                false,
            ),
            Decision::Setup { expected, plan, .. } => (
                "Set up retained workspace?",
                format!(
                    "{}\nWorkflow: {}\nObjective: {}\nSource: {}\nRetained workspace: {}\nSource fingerprint: {}\nWorkspace fingerprint: {}\nCaptured candidate: {:?}\n\n{}",
                    plan.explanation,
                    expected.id,
                    expected.objective,
                    plan.binding.source.display(),
                    plan.binding.workspace.display(),
                    plan.binding.source_fingerprint,
                    plan.binding.workspace_fingerprint,
                    plan.binding.candidate_fingerprint,
                    setup::RISKS
                ),
                "Authorize setup commands",
                false,
            ),
            Decision::Apply { plan, .. } => (
                "Apply reviewed changes?",
                format!(
                    "{}\n\nThis writes the reviewed file changes to the original checkout. External editors are not locked. A durable recovery journal is retained before mutation; conflicts fail closed. Git index, commits and deployment are not changed.",
                    plan.summary()
                ),
                "Apply reviewed changes",
                true,
            ),
            Decision::RollBack { transaction, .. } => (
                "Roll back interrupted application?",
                format!(
                    "Transaction {transaction}\n\nRestore only paths still matching this transaction. External changes will not be overwritten; unresolved conflicts retain their recovery journal. This never replays an apply or command."
                ),
                "Roll back transaction",
                true,
            ),
        };
        let blocked = match &decision {
            Decision::Rerun(_) | Decision::Resume(_) => None,
            Decision::Continue { expected, .. } => {
                if self.tasks.operations.review_scope.as_ref() != Some(&self.operation_scope()) {
                    Some("Conversation or permission changed; review again.".into())
                } else {
                    self.task_operation_guard(expected).err()
                }
            }
            Decision::Setup { expected, .. } => {
                if self.tasks.operations.review_scope.as_ref() != Some(&self.operation_scope()) {
                    Some("Conversation or permission context changed; cancel and review a fresh setup plan.".into())
                } else {
                    self.task_operation_guard(expected).err()
                }
            }
            Decision::Apply { expected, .. } | Decision::RollBack { expected, .. } => {
                if self.tasks.operations.review_scope.as_ref() != Some(&self.operation_scope()) {
                    Some("Conversation or permission context changed. Cancel and review a fresh action.".into())
                } else {
                    self.task_operation_guard(expected).err()
                }
            }
        };
        let choice = confirmations::Dialog::new("workflow-operation", title, &detail, "Cancel", label)
            .destructive(danger).enabled(blocked.is_none()).width(480.0).show(ctx, |ui| {
                if let Decision::Setup { commands, .. } = &mut decision {
                    ui.add(egui::Label::new("One exact command per line. Review project lifecycle scripts before enabling them.").wrap());
                    ui.add(egui::TextEdit::multiline(commands).desired_rows(5).desired_width(f32::INFINITY));
                }
                if let Some(reason) = &blocked { confirmations::note(ui, reason); }
            });
        match choice {
            None => self.tasks.operations.decision = Some(decision),
            Some(confirmations::Choice::Cancel) => {}
            Some(confirmations::Choice::Confirm) => {
                let result = match decision {
                    Decision::Continue {
                        expected, boundary, ..
                    } => self.task_confirm_continuation(expected, boundary),
                    Decision::Rerun(request) => self.task_confirm_check_rerun(request),
                    Decision::Resume(request) => self.task_confirm_initial_resume(request),
                    Decision::Setup {
                        expected,
                        plan,
                        commands,
                    } => self.confirm_environment_setup(expected, plan, commands),
                    Decision::Apply { expected, plan } => {
                        self.confirm_apply(expected, Some(plan), None)
                    }
                    Decision::RollBack {
                        expected,
                        transaction,
                    } => self.confirm_apply(expected, None, Some(transaction)),
                };
                if let Err(error) = result {
                    self.state.notice = Some(error);
                }
            }
        }
    }
    fn confirm_apply(
        &mut self,
        expected: Task,
        plan: Option<domain::apply::Plan>,
        transaction: Option<String>,
    ) -> Result<(), String> {
        self.task_operation_guard(&expected)?;
        if self.tasks.operations.review_scope.as_ref() != Some(&self.operation_scope()) {
            return Err("Application context changed; nothing was written.".into());
        }
        let (reply, rx) = mpsc::channel();
        let id = expected.id.clone();
        let command = if let Some(plan) = plan {
            StoreCommand::Apply {
                expected: Box::new(expected),
                plan: Box::new(plan),
                reply,
            }
        } else {
            StoreCommand::RollBack {
                expected: Box::new(expected),
                transaction: transaction.ok_or("No transaction selected.")?,
                reply,
            }
        };
        self.tasks
            .store
            .as_ref()
            .ok_or("Workflow storage unavailable.")?
            .send(command)
            .map_err(|_| "Workflow storage disconnected.")?;
        self.tasks.operations.applying = Some((id, rx));
        Ok(())
    }
    fn confirm_environment_setup(
        &mut self,
        expected: Task,
        plan: setup::Plan,
        commands: String,
    ) -> Result<(), String> {
        self.task_operation_guard(&expected)?;
        if self.tasks.operations.review_scope.as_ref() != Some(&self.operation_scope()) {
            return Err(
                "Setup conversation or permission context changed. Review a fresh plan.".into(),
            );
        }
        let plan = plan.with_commands(
            commands
                .lines()
                .map(str::to_owned)
                .filter(|s| !s.trim().is_empty())
                .collect(),
        )?;
        let id = expected.id.clone();
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        self.tasks.operations.reading = Some((id, rx));
        // Validation is IO-bound. The result is dispatched directly, not another consent question.
        std::thread::spawn(move || {
            let _ = tx.send(
                plan.validate_current(&expected)
                    .map(|_| ReadResult::SetupReady(expected, plan)),
            );
            ctx.request_repaint();
        });
        Ok(())
    }
}
