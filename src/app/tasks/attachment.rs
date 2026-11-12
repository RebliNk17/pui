//! Explicit, acknowledged conversation attachment and fingerprint-only recovery.
use super::*;

struct Request {
    expected: Task,
    parent: PathBuf,
    agent: String,
}
struct PendingAttachment {
    id: String,
    parent: PathBuf,
    agent: String,
    reply: Receiver<Result<Task, String>>,
}
#[derive(Default)]
pub(super) struct Attachment {
    confirm: Option<Request>,
    pending: Option<PendingAttachment>,
}
impl Attachment {
    fn confirming(&self) -> bool {
        self.confirm.is_some()
    }
    pub(super) fn pending_id(&self) -> Option<&str> {
        self.pending.as_ref().map(|pending| pending.id.as_str())
    }
}
impl Tasks {
    pub(in crate::app) fn attachment_confirming(&self) -> bool {
        self.attachment.confirming()
    }
}
impl Pui {
    fn retained_write_blocked(&self, id: &str) -> Option<String> {
        if self.demo || self.is_mirror() || self.readonly_worker() {
            return Some("An owning, writable conversation is required.".into());
        }
        if matches!(
            self.active_permission(),
            settings::PermissionMode::Plan | settings::PermissionMode::DontAsk
        ) {
            return Some("Current permission mode does not allow changing workflows.".into());
        }
        if self.deletion.in_progress()
            || self.tasks.loading_history()
            || self.tasks.storage_failed
            || self.tasks.store.is_none()
        {
            return Some("Wait for workflow storage and deletion to finish.".into());
        }
        let Some(task) = self.tasks.records.iter().find(|task| task.id == id) else {
            return Some("Workflow is no longer available.".into());
        };
        if task.phase.active()
            || self.tasks.live.is_some()
            || self.tasks.job.is_some()
            || self.tasks.revalidate_claim.is_some()
            || self.tasks.attachment.pending.is_some()
            || self
                .tasks
                .retiring
                .iter()
                .any(|rx| matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)))
        {
            return Some(
                "Wait for active workflow work to stop before changing retained history.".into(),
            );
        }
        None
    }
    pub(in crate::app) fn task_attachment_blocked(&self, id: &str) -> Option<String> {
        if let Some(reason) = self.retained_write_blocked(id) {
            return Some(reason);
        }
        let Some(path) = self.state.session_file.as_deref() else {
            return Some("Save this conversation before attaching a retained workflow.".into());
        };
        if self
            .tasks
            .records
            .iter()
            .find(|task| task.id == id)
            .and_then(|task| task.parent_session.as_deref())
            .is_some_and(|parent| {
                sync::identity(parent) == sync::identity(std::path::Path::new(path))
            })
        {
            return Some("This workflow is already attached to this conversation.".into());
        }
        None
    }
    pub(in crate::app) fn request_task_attachment(&mut self, id: &str) {
        if let Some(reason) = self.task_attachment_blocked(id) {
            self.state.notice = Some(reason);
            return;
        }
        let expected = self
            .tasks
            .records
            .iter()
            .find(|task| task.id == id)
            .unwrap()
            .clone();
        let request = Request {
            expected,
            parent: PathBuf::from(self.state.session_file.as_ref().unwrap()),
            agent: self.agents.active.clone(),
        };
        if request.expected.parent_session.is_some()
            || self
                .tasks
                .parents
                .get(id)
                .is_some_and(|parent| parent != &request.agent)
        {
            self.tasks.attachment.confirm = Some(request);
        } else if let Err(error) = self.attach_retained(request) {
            self.state.notice = Some(error);
        }
    }
    fn attach_retained(&mut self, request: Request) -> Result<(), String> {
        if let Some(reason) = self.task_attachment_blocked(&request.expected.id) {
            return Err(reason);
        }
        if self
            .state
            .session_file
            .as_deref()
            .map(std::path::Path::new)
            .is_none_or(|path| sync::identity(path) != sync::identity(&request.parent))
        {
            return Err("The destination conversation changed. Nothing was attached.".into());
        }
        let (reply, rx) = mpsc::channel();
        self.tasks
            .store
            .as_ref()
            .ok_or("Workflow storage unavailable.")?
            .send(StoreCommand::Attach {
                expected: Box::new(request.expected.clone()),
                parent: request.parent.clone(),
                reply,
            })
            .map_err(|_| "Workflow storage unavailable.")?;
        self.tasks.attachment.pending = Some(PendingAttachment {
            id: request.expected.id,
            parent: request.parent,
            agent: request.agent,
            reply: rx,
        });
        Ok(())
    }
    pub(in crate::app) fn task_attachment_confirmation(&mut self, ctx: &egui::Context) {
        let Some(request) = &self.tasks.attachment.confirm else {
            return;
        };
        let reason = self.task_attachment_blocked(&request.expected.id);
        let choice = confirmations::Dialog::new(
            "attach-retained-workflow", "Move workflow to this conversation?",
            "The previous conversation will no longer show this workflow. Its checks, review, approved contract, workspace and spend history stay unchanged.",
            "Cancel", "Attach to this conversation",
        ).destructive(false).enabled(reason.is_none()).show(ctx, |ui| {
            ui.add(egui::Label::new(&request.expected.objective).wrap());
            if let Some(reason) = reason { confirmations::note(ui, &reason); }
        });
        match choice {
            Some(confirmations::Choice::Confirm) => {
                let request = self.tasks.attachment.confirm.take().unwrap();
                if let Err(error) = self.attach_retained(request) {
                    self.state.notice = Some(error);
                }
            }
            Some(confirmations::Choice::Cancel) => {
                self.tasks.attachment.confirm = None;
            }
            None => {}
        }
    }
    pub(super) fn poll_task_attachment(&mut self) {
        let Some(pending) = &self.tasks.attachment.pending else {
            return;
        };
        let result = match pending.reply.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Workflow attachment disconnected. Refresh history before retrying.".into())
            }
        };
        let pending = self.tasks.attachment.pending.take().unwrap();
        // Invalidate any older refresh snapshot queued before this acknowledgment.
        self.tasks.updates.pending = None;
        self.tasks.updates.requested = true;
        match result {
            Ok(task) => {
                if let Some(record) = self.tasks.records.iter_mut().find(|t| t.id == task.id) {
                    *record = task;
                    self.tasks.parents.remove(&pending.id);
                    if self
                        .agents
                        .nodes
                        .get(&pending.agent)
                        .and_then(|node| node.path.as_ref())
                        .is_some_and(|path| sync::identity(path) == sync::identity(&pending.parent))
                        || (self.agents.active == pending.agent
                            && self.state.session_file.as_deref().is_some_and(|path| {
                                sync::identity(std::path::Path::new(path))
                                    == sync::identity(&pending.parent)
                            }))
                    {
                        self.tasks.parents.insert(pending.id, pending.agent);
                    }
                }
            }
            Err(error) => {
                self.state.notice = Some(format!("Could not attach this workflow: {error}"))
            }
        }
    }
    pub(in crate::app) fn task_revalidate_blocked(&self, id: &str) -> Option<String> {
        if let Some(reason) = self.retained_write_blocked(id) {
            return Some(reason);
        }
        let task = self.tasks.records.iter().find(|task| task.id == id)?;
        if task.workspace.is_none() || task.latest().and_then(|a| a.fingerprint.as_ref()).is_none()
        {
            return Some(
                "A retained workspace and captured candidate are required for revalidation.".into(),
            );
        }
        None
    }
    /// Ownership + optimistic comparison precede filesystem fingerprinting. This
    /// never invokes checks/models, retries work, or upgrades invalid evidence.
    pub(in crate::app) fn task_revalidate_retained(&mut self, id: &str) -> Result<(), String> {
        if let Some(reason) = self.task_revalidate_blocked(id) {
            return Err(reason);
        }
        let expected = self
            .tasks
            .records
            .iter()
            .find(|task| task.id == id)
            .unwrap()
            .clone();
        let (reply, rx) = mpsc::channel();
        self.tasks
            .store
            .as_ref()
            .ok_or("Workflow storage unavailable.")?
            .send(StoreCommand::ClaimCurrent {
                expected: Box::new(expected),
                reply,
            })
            .map_err(|_| "Workflow storage unavailable.")?;
        self.tasks.selected = Some(id.into());
        self.tasks.revalidate_at = None;
        self.tasks.revalidate_claim = Some((id.into(), rx));
        Ok(())
    }
}

#[cfg(test)]
#[path = "attachment_tests.rs"]
mod tests;
