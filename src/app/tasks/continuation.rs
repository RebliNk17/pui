//! Native-confirmed continuation; persisted consent and candidate revalidation
//! precede the next stage. Never an interrupted builder replay.
use super::*;
pub(super) struct Pending {
    expected: Task,
    task: Task,
    boundary: domain::continuation::Boundary,
    reply: Receiver<Result<(), String>>,
}
#[derive(Default)]
pub(super) struct State {
    pub pending: Option<Pending>,
    pub boundary: Option<domain::continuation::Boundary>,
    owner: Option<(String, Option<String>, settings::PermissionMode)>,
    active: Option<String>,
    cancelled: bool,
}
impl State {
    pub(super) fn cancel(&mut self) {
        if self.pending.is_some() {
            self.cancelled = true;
        } else {
            *self = Self::default();
        }
    }
}
impl Pui {
    pub(super) fn task_confirm_continuation(
        &mut self,
        expected: Task,
        boundary: domain::continuation::Boundary,
    ) -> Result<(), String> {
        use sha2::{Digest, Sha256};
        self.task_operation_guard(&expected)?;
        if expected.continuation_boundary()? != boundary {
            return Err("Workflow changed after continuation review.".into());
        }
        let mut task = expected.clone();
        task.pause_requested = false;
        task.reason = None;
        task.phase = match boundary {
            domain::continuation::Boundary::Checks => Phase::Checking,
            domain::continuation::Boundary::Review => Phase::Verifying,
        };
        task.continuations.push(domain::continuation::Continuation {
            id: uuid::Uuid::new_v4().to_string(),
            boundary,
            fingerprint: task.latest().unwrap().fingerprint.clone().unwrap(),
            confirmed_record: crate::hex::lower(Sha256::digest(
                serde_json::to_vec(&expected).unwrap(),
            )),
            created: domain::now(),
        });
        let (reply, rx) = mpsc::channel();
        self.tasks
            .store
            .as_ref()
            .ok_or("Workflow storage unavailable.")?
            .send(StoreCommand::SaveAcknowledged {
                expected: Box::new(expected.clone()),
                task: Box::new(task.clone()),
                reply,
            })
            .map_err(|_| "Workflow storage unavailable.")?;
        self.tasks.local_owned.insert(task.id.clone());
        self.tasks.continuation.cancelled = false;
        self.tasks.continuation.pending = Some(Pending {
            expected,
            task,
            boundary,
            reply: rx,
        });
        self.tasks.continuation.owner = Some(self.operation_scope());
        self.ctx.request_repaint();
        Ok(())
    }
    pub(super) fn poll_task_continuation(&mut self) {
        let Some(pending) = self.tasks.continuation.pending.as_ref() else {
            return;
        };
        let result = match pending.reply.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err(
                "Continuation acknowledgment lost; inspect authoritative state, no replay.".into(),
            ),
        };
        let Pending {
            expected,
            task,
            boundary,
            ..
        } = self.tasks.continuation.pending.take().unwrap();
        if let Err(error) = result {
            self.task_recovery_reconcile(expected, task, error, false);
            return;
        }
        let unchanged = self
            .tasks
            .records
            .iter()
            .find(|current| current.id == task.id)
            .is_some_and(|current| {
                use sha2::{Digest, Sha256};
                crate::hex::lower(Sha256::digest(serde_json::to_vec(current).unwrap()))
                    == task.continuations.last().unwrap().confirmed_record
            });
        let owner_valid = self
            .tasks
            .continuation
            .owner
            .as_ref()
            .is_some_and(|scope| *scope == self.operation_scope());
        if !unchanged
            || self.tasks.continuation.cancelled
            || !owner_valid
            || self.tasks.live.is_some()
            || !self.task_in_chat(&task)
            || self.readonly_worker()
            || self.is_mirror()
            || self.demo
            || self.deletion.in_progress()
            || self.tasks.storage_failed
            || matches!(
                self.active_permission(),
                settings::PermissionMode::Plan | settings::PermissionMode::DontAsk
            )
        {
            self.task_recovery_reconcile(
                expected,
                task,
                "Continuation context changed; nothing launched.".into(),
                false,
            );
            return;
        }
        if let Some(current) = self.tasks.records.iter_mut().find(|t| t.id == task.id) {
            *current = task.clone();
        } else {
            return;
        }
        self.tasks.continuation.active = Some(task.id.clone());
        self.tasks.live = Some(task.id);
        self.tasks.continuation.boundary = Some(boundary);
        self.task_fingerprint(FingerprintStep::Continue);
    }
    pub(super) fn task_continuation_active(&self) -> bool {
        self.tasks.live.is_some() && self.tasks.continuation.active == self.tasks.live
    }
    pub(super) fn task_continuation_guard(&mut self) -> bool {
        if self.tasks.continuation.active.is_none() {
            return true;
        }
        let valid = self.task_continuation_active()
            && self.tasks.continuation.owner.as_ref() == Some(&self.operation_scope())
            && !self.is_mirror()
            && !self.readonly_worker()
            && !self.demo
            && !self.deletion.in_progress()
            && !self.tasks.storage_failed;
        if !valid {
            self.tasks
                .block("Continuation authority changed; no stage or repair was started.");
        }
        valid
    }
    pub(super) fn task_continue_fingerprinted(&mut self, fingerprint: &str) {
        if !self.task_continuation_guard() {
            return;
        }
        if self
            .tasks
            .live_task()
            .and_then(|task| task.latest())
            .and_then(|a| a.fingerprint.as_deref())
            != Some(fingerprint)
        {
            self.tasks.block(
                "Candidate changed; continuation refused. Approve a new plan or inspect evidence.",
            );
            return;
        }
        match self.tasks.continuation.boundary.take() {
            Some(domain::continuation::Boundary::Checks) => {
                self.tasks.live_mut().unwrap().phase = Phase::Checking;
                self.tasks.save();
                self.task_environment(environment::Stage::Checks);
            }
            Some(domain::continuation::Boundary::Review) => {
                self.tasks.live_mut().unwrap().phase = Phase::Verifying;
                self.tasks.save();
                self.task_launch(Role::Verifier);
            }
            None => {}
        }
    }
}
