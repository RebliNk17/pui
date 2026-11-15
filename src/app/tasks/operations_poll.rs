use super::*;

impl Pui {
    pub(super) fn reconcile_setup_record(&mut self, id: &str) {
        self.tasks.operations.quarantined.insert(id.into());
        self.tasks.local_owned.remove(id);
        self.tasks.updates.pending = None;
        self.tasks.updates.requested = true;
        if let Some(task) = self.tasks.records.iter_mut().find(|task| task.id == id) {
            task.invalidate("Setup persistence or execution outcome is uncertain. Reconcile authoritative history before further actions; no command was replayed.");
            task.implementation_pending = false;
        }
        let (reply, rx) = mpsc::channel();
        if self.tasks.store.as_ref().is_some_and(|store| {
            store
                .send(StoreCommand::ReleaseCurrent {
                    id: id.into(),
                    reply,
                })
                .is_ok()
        }) {
            self.tasks.operations.reconciling = Some((id.into(), rx));
        }
    }
    pub(in crate::app) fn poll_task_operations(&mut self) {
        if let Some((id, reply)) = &self.tasks.operations.reconciling {
            let result = match reply.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Workflow reconciliation disconnected; source/effects remain uncertain.".into(),
                )),
            };
            if let Some(result) = result {
                let id = id.clone();
                self.tasks.operations.reconciling = None;
                match result {
                    Ok(snapshot) => {
                        self.tasks.local_owned.remove(&id);
                        if let Some(authoritative) =
                            snapshot.records.into_iter().find(|task| task.id == id)
                        {
                            if let Some(task) =
                                self.tasks.records.iter_mut().find(|task| task.id == id)
                            {
                                if task.parent_session != authoritative.parent_session {
                                    self.tasks.parents.remove(&id);
                                }
                                *task = authoritative;
                            }
                        } else {
                            self.tasks.forget_deleted(&id);
                        }
                        self.tasks.operations.quarantined.remove(&id);
                    }
                    Err(error) => self.state.notice = Some(error),
                }
            }
        }
        if let Some((id, reply)) = &self.tasks.operations.environment {
            if let Ok(note) = reply.try_recv() {
                if let Some((task, status)) = &mut self.tasks.operations.status {
                    if task == id {
                        status.push('\n');
                        status.push_str(&note);
                    }
                }
                self.tasks.operations.environment = None;
            }
        }
        if let Some((id, pending)) = &self.tasks.operations.reading {
            let result = match pending.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Workflow inspection disconnected; nothing was run.".into(),
                )),
            };
            if let Some(result) = result {
                let id = id.clone();
                self.tasks.operations.reading = None;
                match result {
                    Ok(ReadResult::Setup(expected, plan)) => {
                        let scope = self.tasks.operations.review_scope.as_ref()
                            == Some(&self.operation_scope());
                        if !scope {
                            self.state.notice = Some("Setup conversation or permission context changed; no command ran. Review a fresh plan.".into());
                        } else if let Err(error) = self.task_operation_guard(&expected) {
                            self.state.notice = Some(error);
                        } else {
                            let commands = plan.commands.join("\n");
                            self.tasks.operations.decision = Some(Decision::Setup {
                                expected,
                                plan,
                                commands,
                            });
                        }
                    }
                    Ok(ReadResult::SetupReady(expected, plan)) => {
                        let scope = self.tasks.operations.review_scope.as_ref()
                            == Some(&self.operation_scope());
                        let result = if !scope {
                            Err(
                                "Setup conversation or permission context changed; nothing ran."
                                    .into(),
                            )
                        } else {
                            self.task_operation_guard(&expected).and_then(|_| {
                                let owner = self
                                    .tasks
                                    .parents
                                    .get(&expected.id)
                                    .map(String::as_str)
                                    .unwrap_or(&self.agents.active);
                                let settings = self.agent_settings(owner).payload();
                                setup::Process::start(&expected, plan, self.ctx.clone(), settings)
                            })
                        };
                        match result {
                            Ok(process) => {
                                self.tasks.operations.setup_owner = Some((
                                    self.agents.active.clone(),
                                    self.state.session_file.clone(),
                                    self.active_permission(),
                                ));
                                self.tasks.operations.setup_baseline = Some(expected);
                                self.tasks.setup = Some(process);
                            }
                            Err(error) => self.state.notice = Some(error),
                        }
                    }
                    Ok(ReadResult::Recovery(status)) => {
                        self.tasks.operations.transactions = status;
                        self.tasks.operations.transactions_task = Some(id.clone());
                        self.tasks.operations.status = Some((
                            id,
                            "Retained apply transactions inspected. Rollback is never automatic."
                                .into(),
                        ));
                    }
                    Err(error) => self.state.notice = Some(error),
                }
            }
        }
        if let Some((id, reply)) = &self.tasks.operations.applying {
            let result = match reply.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("Application outcome is uncertain. Inspect retained transaction journals before any further action; do not retry.".into())),
            };
            if let Some(result) = result {
                let id = id.clone();
                self.tasks.operations.applying = None;
                self.tasks.operations.status = Some((
                    id,
                    match result {
                        Ok(outcome) => format!(
                            "{:?}\n{}\nTransaction {} · recovery retained at {}",
                            outcome.status,
                            outcome.detail,
                            outcome.transaction_id,
                            outcome.retained_at.display()
                        ),
                        Err(error) => error,
                    },
                ));
            }
        }
        let setup_scope_valid = self.tasks.setup.is_none() || self.setup_scope_valid();
        if !setup_scope_valid {
            if let Some(process) = &mut self.tasks.setup {
                process.cancel();
            }
        }
        if let Some(save) = &self.tasks.operations.saving {
            let result = match save.reply.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("Setup persistence disconnected. No further command may start; inspect uncertain effects.".into())),
            };
            if let Some(result) = result {
                let saved = self.tasks.operations.saving.take().unwrap();
                match result {
                    Ok(()) => {
                        let record = saved.task.setup.last().cloned();
                        self.tasks.operations.setup_baseline = Some(saved.task.clone());
                        if let Some(task) = self
                            .tasks
                            .records
                            .iter_mut()
                            .find(|t| t.id == saved.task.id)
                        {
                            *task = saved.task;
                        }
                        if let (Some(process), Some(record)) = (&mut self.tasks.setup, record) {
                            // Process may have completed/cancelled while persistence was pending.
                            // A different staged record is saved again, never acknowledged blindly.
                            if process.record == record && (process.finished() || setup_scope_valid)
                            {
                                if let Err(error) = process.persisted(&record) {
                                    self.state.notice = Some(error);
                                    process.cancel();
                                }
                            }
                        }
                    }
                    Err(error) => {
                        if let Some(process) = &mut self.tasks.setup {
                            process.cancel();
                        }
                        let id = saved.task.id.clone();
                        self.tasks.operations.status = Some((
                            saved.task.id,
                            format!(
                                "Setup evidence could not be saved: {error}. No further command will be submitted. Inspect any already-started effects; no retry occurred."
                            ),
                        ));
                        if let Some(process) = self.tasks.setup.take() {
                            process.retire();
                        }
                        self.tasks.operations.setup_baseline = None;
                        self.tasks.operations.setup_owner = None;
                        self.reconcile_setup_record(&id);
                    }
                }
            }
        }
        let Some(process) = &mut self.tasks.setup else {
            return;
        };
        process.poll();
        if process.needs_persist() && self.tasks.operations.saving.is_none() {
            let Some(expected) = self.tasks.operations.setup_baseline.clone() else {
                process.cancel();
                return;
            };
            let mut task = expected.clone();
            if let Some(record) = task
                .setup
                .iter_mut()
                .find(|record| record.id == process.record.id)
            {
                *record = process.record.clone();
            } else {
                task.setup.push(process.record.clone());
            }
            if process.record.source_changed
                || matches!(
                    process.record.status,
                    domain::setup::SetupStatus::Unknown | domain::setup::SetupStatus::Cancelled
                )
            {
                task.invalidate("Authorized setup changed captured source files or its effects are uncertain; evidence is stale. No workflow stage resumed.");
                task.implementation_pending = false;
            }
            let (reply, rx) = mpsc::channel();
            let command = StoreCommand::SaveAcknowledged {
                expected: Box::new(expected),
                task: Box::new(task.clone()),
                reply,
            };
            let sent = self
                .tasks
                .store
                .as_ref()
                .is_some_and(|store| store.send(command).is_ok());
            if sent {
                self.tasks.operations.saving = Some(Save { task, reply: rx });
            } else {
                process.cancel();
                let id = process.record.binding.task.clone();
                self.state.notice =
                    Some("Setup storage unavailable; no further command may run.".into());
                if let Some(process) = self.tasks.setup.take() {
                    process.retire();
                }
                self.tasks.operations.setup_owner = None;
                self.reconcile_setup_record(&id);
            }
        } else if process.finished()
            && !process.needs_persist()
            && self.tasks.operations.saving.is_none()
        {
            let id = process.record.binding.task.clone();
            let message = format!(
                "Setup {:?}: {}\nNo workflow stage resumed. Inspect refreshed environment evidence, then explicitly request checks or a new workflow.",
                process.record.status, process.record.note
            );
            self.tasks.operations.status = Some((id.clone(), message));
            if let Some(process) = self.tasks.setup.take() {
                process.retire();
            }
            self.tasks.operations.setup_baseline = None;
            self.tasks.operations.setup_owner = None;
            let task = self
                .tasks
                .records
                .iter()
                .find(|task| task.id == id)
                .cloned();
            if let Some(task) = task {
                let ctx = self.ctx.clone();
                let (send, reply) = mpsc::channel();
                self.tasks.operations.environment = Some((id, reply));
                std::thread::spawn(move || {
                    let workspace = task.workspace.as_ref().unwrap_or(&task.source);
                    let _ = send.send(
                        environment::inspect(&task.source, workspace, task.approved.as_ref())
                            .note(),
                    );
                    ctx.request_repaint();
                });
            }
        }
    }
}
