//! App-owned task supervisor. Pi still owns the agent loop and tool permissions.
use super::*;
use crate::tasks::{
    self as domain, Attempt, Check, Contract, Phase, Review, Role, Run, Task, Verdict,
};
use std::collections::BTreeMap;

mod attachment;
mod changes_preview;
pub(super) mod chat;
mod consent;
pub(super) mod environment;
mod operations;
pub(super) mod recovery;
mod refresh;
mod routing;
mod runtime;
#[cfg(test)]
#[path = "tasks/save_conflict_tests.rs"]
mod save_conflict_tests;
mod setup;
#[cfg(test)]
#[path = "tasks/store_safety_tests.rs"]
mod store_safety_tests;
#[cfg(test)]
mod tests;
mod view;
pub(super) mod work_item;
mod workflow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FingerprintStep {
    Candidate,
    AfterChecks,
    AfterReview,
    Revalidate,
    RecoveryBeforeChecks,
    RecoveryAfterChecks,
    RecoveryAfterReview,
    InitialResumeBeforeBuilder,
}
enum JobResult {
    Inspected(domain::workspace::Inspection),
    Prepared(
        PathBuf,
        bool,
        Option<domain::apply::BaselineReceipt>,
        Option<String>,
    ),
    Fingerprint(FingerprintStep, String),
    Environment(environment::Stage, environment::Report),
}
struct Job {
    task: String,
    rx: Receiver<Result<JobResult, String>>,
}
#[derive(Clone)]
struct Pending {
    id: String,
    command: String,
    started: std::time::Instant,
    observed: bool,
    accepted: bool,
    settled: bool,
    aborted: bool,
}
struct Process {
    run: String,
    role: Role,
    client: rpc::Client,
    state: State,
    pending: Option<Pending>,
    initializing: bool,
    ended: bool,
    final_error: bool,
    routing_for: Option<Role>,
    route_prompt: Option<String>,
}
enum StoreCommand {
    Save(Box<Task>),
    SaveAcknowledged {
        expected: Box<Task>,
        task: Box<Task>,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Apply {
        expected: Box<Task>,
        plan: Box<domain::apply::Plan>,
        reply: mpsc::Sender<Result<domain::apply::ApplyOutcome, String>>,
    },
    RollBack {
        expected: Box<Task>,
        transaction: String,
        reply: mpsc::Sender<Result<domain::apply::ApplyOutcome, String>>,
    },
    // Observers may inspect evidence without stealing a task's writer lock.
    // Conflict is local to this revalidation, not a broken storage subsystem.
    ClaimCurrent {
        expected: Box<Task>,
        reply: mpsc::Sender<Result<(), String>>,
    },
    ReleaseCurrent {
        id: String,
        reply: mpsc::Sender<Result<refresh::Snapshot, String>>,
    },
    Attach {
        expected: Box<Task>,
        parent: PathBuf,
        reply: mpsc::Sender<Result<Task, String>>,
    },
    Refresh {
        reply: mpsc::Sender<Result<refresh::Snapshot, String>>,
    },
    Detach {
        tasks: Vec<Task>,
        reply: mpsc::Sender<Result<(), String>>,
        projection: Option<mpsc::Sender<Result<Vec<Task>, String>>>,
    },
    Delete {
        id: String,
        sessions: Vec<PathBuf>,
        locks: Vec<Option<std::fs::File>>,
        reply: mpsc::Sender<Result<(), String>>,
    },
}
struct StoreError {
    task: String,
    message: String,
    conflict: bool,
}
#[derive(Default)]
pub(super) struct Tasks {
    pub open: bool,
    pub selected: Option<String>,
    pub(super) records: Vec<Task>,
    pub(super) parents: BTreeMap<String, String>,
    display_paths: chat::DisplayPaths,
    live: Option<String>,
    processes: Vec<Process>,
    retiring: Vec<Receiver<()>>,
    histories: BTreeMap<String, State>,
    pub(super) selected_run: Option<String>,
    activity_target: Option<(String, usize)>,
    pub(super) views: BTreeMap<String, work_item::View>,
    pub(super) transcript: Transcript,
    job: Option<Job>,
    revalidate_claim: Option<(String, Receiver<Result<(), String>>)>,
    attachment: attachment::Attachment,
    recovery: recovery::State,
    operations: operations::State,
    setup: Option<setup::Process>,
    updates: refresh::Updates,
    local_owned: std::collections::BTreeSet<String>,
    store: Option<mpsc::Sender<StoreCommand>>,
    store_worker: Option<std::thread::JoinHandle<()>>,
    loaded: Option<Receiver<Result<Vec<Task>, String>>>,
    errors: Option<Receiver<StoreError>>,
    detached: Vec<Receiver<Result<Vec<Task>, String>>>,
    pub error: Option<String>,
    storage_failed: bool,
    pub dialog_text: String,
    pub current_dialog: String,
    questions: questions::View,
    requests: BTreeMap<String, consent::Request>,
    pub revalidate_at: Option<std::time::Instant>,
    preview: Option<(String, Receiver<Result<crate::history::Preview, String>>)>,
    watched: Option<(String, PathBuf)>,
    watcher: Option<crate::file_watch::Watcher>,
    changes: Option<Receiver<()>>,
}
impl Tasks {
    pub fn new(ctx: &egui::Context, enabled: bool) -> Self {
        let mut result = Self::default();
        if !enabled {
            return result;
        }
        let Some(home) = domain::store::home() else {
            result.error = Some("Cannot locate private task storage.".into());
            return result;
        };
        let (tx, rx) = mpsc::channel::<StoreCommand>();
        let (errors, error_rx) = mpsc::channel();
        let (loaded_tx, loaded) = mpsc::channel();
        let (watch_tx, watch_rx) = mpsc::channel();
        result.updates.setup = Some(watch_rx);
        let ctx = ctx.clone();
        result.store_worker = Some(std::thread::spawn(move || {
            let _ = loaded_tx.send(domain::store::load(&home));
            let _ = watch_tx.send(refresh::watch(home.clone(), &ctx));
            ctx.request_repaint();
            run_store(&home, rx, errors, &ctx);
        }));
        result.store = Some(tx);
        result.loaded = Some(loaded);
        result.errors = Some(error_rx);
        result
    }
    pub(super) fn search_dialogs_pending(&self) -> bool {
        self.processes.iter().any(|p| !p.state.dialogs.is_empty())
    }
    pub(super) fn search_state(&self, run: &str) -> Option<&State> {
        self.processes
            .iter()
            .find(|p| p.run == run)
            .map(|p| &p.state)
            .or_else(|| self.histories.get(run))
    }
    pub(super) fn search_revision(&self) -> u64 {
        self.processes
            .iter()
            .map(|p| {
                p.state
                    .transcript_revision
                    .wrapping_add(p.state.prose_revision)
                    .wrapping_add(p.state.messages.len() as u64)
            })
            .chain(self.histories.values().map(|s| {
                s.transcript_revision
                    .wrapping_add(s.prose_revision)
                    .wrapping_add(s.messages.len() as u64)
            }))
            .fold(0, u64::wrapping_add)
    }
    pub(super) fn search_jump(
        &mut self,
        task: &str,
        run: &str,
        role: &str,
        timestamp: i64,
        fingerprint: u64,
    ) -> bool {
        if self.selected.as_deref() != Some(task) || self.selected_run.as_deref() != Some(run) {
            return true;
        }
        let Some(state) = self.search_state(run) else {
            return false;
        };
        let indices: Vec<_> = state
            .messages
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                m.role == role
                    && m.timestamp == timestamp
                    && super::search::text_fingerprint(&m.text) == fingerprint
            })
            .map(|(index, _)| index)
            .take(2)
            .collect();
        if indices.len() == 1 {
            self.transcript.jump_to_message(indices[0]);
            self.activity_target = Some((run.into(), indices[0]));
            self.views.entry(task.into()).or_default().tab = work_item::Tab::Activity;
        }
        true
    }
    pub(super) fn loading_history(&self) -> bool {
        self.loaded.is_some()
    }
    pub(super) fn demo_proposal(&mut self) {
        if self
            .current()
            .is_some_and(|t| t.runs.iter().any(|r| r.id == "demo-Proposal"))
        {
            self.selected_run = Some("demo-Proposal".into());
        }
    }
    pub(super) fn current(&self) -> Option<&Task> {
        let id = self.selected.as_ref()?;
        self.records.iter().find(|t| &t.id == id)
    }
    fn current_mut(&mut self) -> Option<&mut Task> {
        let id = self.selected.as_ref()?;
        self.records.iter_mut().find(|t| &t.id == id)
    }
    fn live_task(&self) -> Option<&Task> {
        let id = self.live.as_ref()?;
        self.records.iter().find(|t| &t.id == id)
    }
    fn live_mut(&mut self) -> Option<&mut Task> {
        let id = self.live.as_ref()?;
        self.records.iter_mut().find(|t| &t.id == id)
    }
    fn save(&mut self) {
        let Some(t) = self
            .live
            .as_ref()
            .or(self.selected.as_ref())
            .and_then(|id| self.records.iter_mut().find(|t| &t.id == id))
        else {
            return;
        };
        t.updated = domain::now();
        self.local_owned.insert(t.id.clone());
        if let Some(store) = &self.store {
            if store.send(StoreCommand::Save(Box::new(t.clone()))).is_err() {
                self.error = Some("Task storage is unavailable; stop before continuing.".into());
                self.storage_failed = true;
            }
        }
    }
    pub(super) fn delete_blocked(&self, id: &str) -> Option<String> {
        let Some(task) = self.records.iter().find(|t| t.id == id) else {
            return Some("Task no longer available.".into());
        };
        if self
            .retiring
            .iter()
            .any(|rx| matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)))
        {
            return Some("Wait for stopped task processes to exit before deleting it.".into());
        }
        if task.phase.active()
            || self.live.as_deref() == Some(id)
            || self.job.as_ref().is_some_and(|j| j.task == id)
        {
            return Some("Stop the task before deleting it.".into());
        }
        None
    }
    pub(super) fn delete_saved(
        &self,
        id: &str,
        sessions: Vec<PathBuf>,
        locks: Vec<Option<std::fs::File>>,
    ) -> Result<Receiver<Result<(), String>>, String> {
        let (reply, rx) = mpsc::channel();
        self.store
            .as_ref()
            .ok_or("Task storage is unavailable")?
            .send(StoreCommand::Delete {
                id: id.into(),
                sessions,
                locks,
                reply,
            })
            .map_err(|_| "Task storage is unavailable")?;
        Ok(rx)
    }
    pub(super) fn forget_deleted(&mut self, id: &str) {
        if let Some(task) = self.records.iter().find(|t| t.id == id) {
            for run in &task.runs {
                self.histories.remove(&run.id);
            }
        }
        self.records.retain(|t| t.id != id);
        self.parents.remove(id);
        self.views.remove(id);
        if self.selected.as_deref() == Some(id) {
            self.selected = self.records.first().map(|t| t.id.clone());
            self.selected_run = None;
            self.transcript = Default::default();
            self.preview = None;
            self.watcher = None;
            self.watched = None;
            self.changes = None;
            self.revalidate_at = None;
            self.open = self.selected.is_some();
        }
    }
    pub(super) fn prepare_detach_deleted_chat(
        &mut self,
        path: &std::path::Path,
    ) -> Result<Option<Receiver<Result<(), String>>>, String> {
        let linked: Vec<_> = self
            .records
            .iter()
            .filter(|t| {
                t.parent_session
                    .as_ref()
                    .is_some_and(|p| sync::identity(p) == sync::identity(path))
            })
            .cloned()
            .collect();
        if linked.is_empty() {
            return Ok(None);
        }
        let Some(store) = &self.store else {
            if cfg!(test) {
                return Ok(None);
            }
            return Err(
                "Task storage is unavailable; linked tasks cannot be detached safely.".into(),
            );
        };
        // The store owns task locks; this command must complete before the chat
        // disappears, so another window cannot restore its old parent link.
        let (reply, rx) = mpsc::channel();
        let (projection, projected) = mpsc::channel();
        store
            .send(StoreCommand::Detach {
                tasks: linked,
                reply,
                projection: Some(projection),
            })
            .map_err(|_| "Task storage unavailable")?;
        self.detached.push(projected);
        Ok(Some(rx))
    }
    pub(super) fn detach_deleted_chat(&mut self, path: &std::path::Path) {
        // Production detachment is exclusively the validated store transaction.
        // The deletion caller invokes this before its acknowledgment too; never
        // enqueue a blind stale Save or optimistically erase a surviving link.
        if self.store.is_some() {
            self.updates.requested = true;
            return;
        }
        for task in &mut self.records {
            if task
                .parent_session
                .as_ref()
                .is_some_and(|p| sync::identity(p) == sync::identity(path))
            {
                task.parent_session = None;
                self.parents.remove(&task.id);
            }
        }
    }
    pub fn running(&self) -> bool {
        self.live_task().is_some_and(|t| t.phase.active())
    }
    pub fn stop(&mut self) {
        self.recovery.cancel_pending();
        if let Some(process) = &mut self.setup {
            process.cancel();
        }
        if self.live.is_none() {
            return;
        }
        if let Some(t) = self.live_mut() {
            for run in &mut t.runs {
                if matches!(run.status.as_str(), "Connecting" | "Working") {
                    run.status = "Cancelled".into();
                }
            }
            t.phase = Phase::Cancelled;
            t.note = "Stopped by you. No commands were replayed; files and sessions are retained."
                .into();
            recovery::finish_interrupted(t);
        }
        self.revalidate_claim = None;
        if self.job.take().is_some() {
            if let Some(t) = self.live_mut() {
                t.note.push_str(" An already-started filesystem preparation may finish; any resulting worktree is retained, never automatically deleted.");
            }
        }
        for p in &mut self.processes {
            let _ = p.client.send(protocol::command("clear_queue"));
            let _ = p.client.send(protocol::command("abort_bash"));
            let _ = p.client.send(protocol::command("abort"));
            p.pending = None;
            p.ended = true;
        }
        self.save();
        self.retire();
        self.live = None;
    }
    fn retire(&mut self) {
        for p in std::mem::take(&mut self.processes) {
            self.histories.insert(p.run, p.state);
            let (tx, rx) = mpsc::channel();
            self.retiring.push(rx);
            std::thread::spawn(move || {
                drop(p.client);
                let _ = tx.send(());
            });
        }
    }
    fn block(&mut self, reason: impl Into<String>) {
        self.revalidate_claim = None;
        let reason = reason.into();
        if let Some(t) = self.live_mut() {
            for run in &mut t.runs {
                if matches!(run.status.as_str(), "Connecting" | "Working") {
                    run.status = "Blocked".into();
                }
            }
            t.phase = Phase::Blocked;
            t.note = reason;
            recovery::finish_interrupted(t);
        }
        self.save();
        self.retire();
        self.live = None;
        self.job = None;
    }
    fn persist_loaded(&mut self) {
        let mut completed = Vec::new();
        self.detached.retain(|rx| match rx.try_recv() {
            Ok(result) => {
                completed.push(result);
                false
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                completed.push(Err("Workflow detachment disconnected.".into()));
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
        });
        for result in completed {
            // A refresh queued before the detach acknowledgment still contains
            // its old relationship. It must not undo the durable projection.
            self.updates.pending = None;
            self.updates.requested = true;
            match result {
                Ok(tasks) => {
                    for task in tasks {
                        self.parents.remove(&task.id);
                        if let Some(record) =
                            self.records.iter_mut().find(|record| record.id == task.id)
                        {
                            *record = task;
                        }
                    }
                }
                Err(error) => self.error = Some(error),
            }
        }
        if let Some(rx) = &self.loaded {
            match rx.try_recv() {
                Ok(Ok(mut tasks)) => {
                    self.records.append(&mut tasks);
                    self.loaded = None;
                }
                Ok(Err(e)) => {
                    self.error = Some(e);
                    self.loaded = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("Task store disconnected.".into());
                    self.loaded = None;
                }
                Err(_) => {}
            }
        }
        let errors: Vec<_> = self
            .errors
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for error in errors {
            self.error = Some(error.message);
            if error.conflict {
                self.local_owned.remove(&error.task);
                self.updates.requested = true;
                // A rejected writer must not continue work or enqueue a final
                // stale save. Other workflows and shared storage remain usable.
                if self.live.as_ref() == Some(&error.task) {
                    for process in &mut self.processes {
                        let _ = process.client.send(protocol::command("clear_queue"));
                        let _ = process.client.send(protocol::command("abort_bash"));
                        let _ = process.client.send(protocol::command("abort"));
                    }
                    self.retire();
                    self.live = None;
                    self.job = None;
                    self.revalidate_claim = None;
                }
            } else {
                self.storage_failed = true;
            }
        }
    }
    pub fn shutdown(&mut self) {
        self.recovery.cancel_pending();
        if let Some(process) = &mut self.setup {
            process.cancel();
        }
        if self.live.is_some() {
            if let Some(t) = self.live_mut().filter(|t| t.phase.active()) {
                t.phase = Phase::Interrupted;
                t.note = "Pui closed during execution. No automatic replay.".into();
                recovery::finish_interrupted(t);
            }
            self.save();
        }
        for mut p in std::mem::take(&mut self.processes) {
            let _ = p.client.send(protocol::command("abort_bash"));
            drop(p.client);
        }
        self.store.take();
        if let Some(worker) = self.store_worker.take() {
            let _ = worker.join();
        }
    }
}
fn run_store(
    home: &std::path::Path,
    rx: Receiver<StoreCommand>,
    errors: mpsc::Sender<StoreError>,
    ctx: &egui::Context,
) {
    let mut deleted = std::collections::BTreeSet::new();
    let mut rejected = std::collections::BTreeSet::new();
    let mut owned = std::collections::BTreeMap::new();
    while let Ok(command) = rx.recv() {
        match command {
            StoreCommand::Refresh { reply } => {
                let _ = reply.send(domain::store::load(home).map(|records| refresh::Snapshot {
                    records,
                    owned: owned.keys().cloned().collect(),
                }));
            }
            StoreCommand::ReleaseCurrent { id, reply } => {
                owned.remove(&id);
                rejected.insert(id.clone());
                let _ = reply.send(domain::store::load(home).map(|records| refresh::Snapshot {
                    records,
                    owned: owned.keys().cloned().collect(),
                }));
            }
            StoreCommand::ClaimCurrent { expected, reply } => {
                let id = expected.id.clone();
                let result = (|| {
                    // Temporary lock is dropped on conflict; a stale observer
                    // must not block the actual owner after a failed claim.
                    let guard = if owned.contains_key(&id) {
                        None
                    } else {
                        Some(domain::store::lock(home, &id)?)
                    };
                    let raw = domain::store::current_record(home, &expected)?;
                    if raw.phase.active()
                        && (guard.is_none() || expected.phase != Phase::Interrupted)
                    {
                        return Err("This workflow is active in another window.".into());
                    }
                    // A fresh real OS lock proves a crashed writer is gone.
                    // current_record compared the exact recovered Interrupted
                    // view; this claim authorizes fingerprinting, never resume.
                    if let Some(guard) = guard {
                        owned.insert(id.clone(), guard);
                    }
                    rejected.remove(&id);
                    Ok(())
                })();
                let _ = reply.send(result);
            }
            StoreCommand::Attach {
                expected,
                parent,
                reply,
            } => {
                let result = (|| {
                    let _guard = if owned.contains_key(&expected.id) {
                        None
                    } else {
                        Some(domain::store::lock(home, &expected.id)?)
                    };
                    domain::store::attach(home, &expected, &parent)
                })();
                let _ = reply.send(result);
            }
            StoreCommand::SaveAcknowledged {
                expected,
                task,
                reply,
            } => {
                let result = (|| {
                    if deleted.contains(&task.id) || expected.id != task.id {
                        return Err(
                            "Workflow changed or was deleted; command was not authorized.".into(),
                        );
                    }
                    if !owned.contains_key(&task.id) {
                        owned.insert(task.id.clone(), domain::store::lock(home, &task.id)?);
                    }
                    domain::store::current_record(home, &expected)?;
                    domain::store::save(home, &task)?;
                    rejected.remove(&task.id);
                    Ok(())
                })();
                let _ = reply.send(result);
            }
            StoreCommand::Apply {
                expected,
                plan,
                reply,
            } => {
                let result = (|| {
                    if !owned.contains_key(&expected.id) {
                        owned.insert(
                            expected.id.clone(),
                            domain::store::lock(home, &expected.id)?,
                        );
                    }
                    domain::apply::apply_owned(home, &owned[&expected.id], &expected, &plan)
                })();
                let _ = reply.send(result);
            }
            StoreCommand::RollBack {
                expected,
                transaction,
                reply,
            } => {
                let result = (|| {
                    if !owned.contains_key(&expected.id) {
                        owned.insert(
                            expected.id.clone(),
                            domain::store::lock(home, &expected.id)?,
                        );
                    }
                    domain::apply::recover_owned(
                        home,
                        &owned[&expected.id],
                        &expected,
                        &transaction,
                        domain::apply::RecoveryAction::RollBack,
                    )
                })();
                let _ = reply.send(result);
            }
            StoreCommand::Save(task) => {
                if !deleted.contains(&task.id) {
                    let result = (|| {
                        // A foreign owner may release its lock after our first
                        // rejection, while stale saves are still queued. Only
                        // a fresh, validated claim can reopen this writer.
                        if rejected.contains(&task.id) {
                            return Err(domain::store::LOCK_CONFLICT.into());
                        }
                        if !owned.contains_key(&task.id) {
                            owned.insert(task.id.clone(), domain::store::lock(home, &task.id)?);
                        }
                        domain::store::save(home, &task)
                    })();
                    if let Err(message) = result {
                        let conflict = message == domain::store::LOCK_CONFLICT;
                        if conflict {
                            rejected.insert(task.id.clone());
                        }
                        let _ = errors.send(StoreError {
                            task: task.id.clone(),
                            message,
                            conflict,
                        });
                    }
                }
            }
            StoreCommand::Detach {
                tasks,
                reply,
                projection,
            } => {
                let result = (|| {
                    // Validate the entire batch while holding every writer lock,
                    // before changing any relationship. Failed validation must
                    // release new locks rather than monopolize foreign history.
                    let mut acquired = BTreeMap::new();
                    for task in &tasks {
                        if !owned.contains_key(&task.id) && !acquired.contains_key(&task.id) {
                            acquired.insert(task.id.clone(), domain::store::lock(home, &task.id)?);
                        }
                    }
                    let mut authoritative = Vec::with_capacity(tasks.len());
                    for task in &tasks {
                        let raw = domain::store::current_record(home, task)?;
                        if raw.phase.active() {
                            return Err(
                                "Stop the active workflow before deleting its conversation.".into(),
                            );
                        }
                        authoritative.push(raw);
                    }
                    for task in &mut authoritative {
                        task.parent_session = None;
                        domain::store::save(home, task)?;
                    }
                    owned.extend(acquired);
                    Ok(authoritative)
                })();
                if let Some(projection) = projection {
                    let _ = projection.send(result.clone());
                }
                let _ = reply.send(result.map(|_| ()));
            }
            StoreCommand::Delete {
                id,
                sessions,
                locks,
                reply,
            } => {
                let result = (|| {
                    if !owned.contains_key(&id) {
                        owned.insert(id.clone(), domain::store::lock(home, &id)?);
                    }
                    domain::store::delete(home, &id, &sessions)
                })();
                if result.is_ok() {
                    deleted.insert(id);
                }
                let _ = reply.send(result);
                drop(locks);
            }
        }
        ctx.request_repaint();
    }
}
impl Drop for Tasks {
    fn drop(&mut self) {
        self.shutdown();
    }
}
impl Pui {
    pub(super) fn task_board_cards(&self, cards: &mut Vec<kanban::AgentCard>) {
        use kanban::{AgentCard, Status, Target};
        for task in &self.tasks.records {
            for process in self
                .tasks
                .processes
                .iter()
                .filter(|p| !p.ended && task.runs.iter().any(|r| r.id == p.run))
            {
                let mut card = AgentCard::from_state(
                    Target::Task {
                        task: task.id.clone(),
                        run: Some(process.run.clone()),
                    },
                    format!("{} · {}", task.objective, process.role.label()),
                    task.source.clone(),
                    if process.initializing {
                        "Connecting"
                    } else if process.pending.is_none() && process.state.ready() {
                        "Idle"
                    } else {
                        "Working"
                    },
                    &process.state,
                    Some(task.objective.clone()),
                );
                if card.model.is_empty() {
                    card.model = task.model.clone();
                }
                cards.push(card);
            }
            if task.phase == Phase::AwaitingApproval {
                cards.push(AgentCard {
                    target: Target::Task {
                        task: task.id.clone(),
                        run: None,
                    },
                    title: task.objective.clone(),
                    project: task.source.clone(),
                    status: Status::Waiting,
                    activity: "Legacy workflow · not started".into(),
                    model: task.model.clone(),
                    parent: None,
                    action: "View workflow",
                    path: None,
                });
            }
        }
    }
    pub(super) fn open_work_item_tab(&mut self, task: String, tab: work_item::Tab) {
        self.open_task_from_board(task.clone(), None);
        self.tasks.views.entry(task).or_default().tab = tab;
    }
    pub(super) fn work_item_tab(&self, task: &str) -> work_item::Tab {
        self.tasks
            .views
            .get(task)
            .map(|view| view.tab)
            .unwrap_or_default()
    }
    pub(super) fn open_task_from_board(&mut self, task: String, run: Option<String>) {
        self.tasks.activity_target = None;
        self.task_retry_recovery_reconciliation(&task);
        self.tasks.updates.requested = true;
        let view = self.tasks.views.entry(task.clone()).or_default();
        view.tab = if run.is_some() {
            work_item::Tab::Activity
        } else {
            work_item::Tab::Overview
        };
        self.tasks.selected = Some(task);
        self.tasks.selected_run = run;
        self.tasks.transcript = Default::default();
        self.tasks.revalidate_at = Some(std::time::Instant::now());
        self.tasks.open = true;
    }
    /// Match known retained run paths first; names support older role sessions opened from Recent.
    pub(super) fn task_session(&self, state: &State) -> Option<Role> {
        if let Some(path) = state.session_file.as_deref() {
            if let Some(role) = self
                .tasks
                .records
                .iter()
                .flat_map(|t| &t.runs)
                .find(|r| r.session.as_deref() == Some(std::path::Path::new(path)))
                .map(|r| r.role)
            {
                return Some(role);
            }
        }
        let name = &state.session_name;
        if !name.starts_with("Task ") && !name.starts_with("Workflow ") {
            return None;
        }
        if name.contains(" · Acceptance proposal · ") {
            Some(Role::Proposal)
        } else if name.contains(" · Fresh verifier · ") {
            Some(Role::Verifier)
        } else {
            None
        }
    }

    #[cfg(test)]
    pub(super) fn new_task(&mut self) {
        self.create_workflow();
    }
    fn create_workflow(&mut self) {
        if self.deletion.in_progress() {
            return;
        }
        self.kanban.open = false;
        self.automation.open = false;
        if self.tasks.running() {
            if let Some(id) = self
                .tasks
                .live_task()
                .filter(|t| self.task_in_chat(t))
                .map(|t| t.id.clone())
            {
                self.open_task_from_board(id, None);
            } else {
                self.state.notice = Some("Another session has an active task. Wait for it to finish before creating a new task; it has not been moved to this chat.".into());
            }
            return;
        }
        self.tasks.open = true;
        let mut task = Task::new(
            self.prefs.cwd.clone(),
            self.state.provider.clone(),
            self.state.model.clone(),
            self.state.thinking.clone(),
        );
        task.dynamic_routing = true;
        task.parent_session = self.state.session_file.as_ref().map(PathBuf::from);
        let parent = self.ensure_agent();
        self.tasks.parents.insert(task.id.clone(), parent);
        self.tasks.selected = Some(task.id.clone());
        self.tasks.records.insert(0, task);
        self.tasks.selected_run = None;
        self.prefs.hide_inspector = false;
        self.files.close();
        self.inspector_panel = false;
        self.prefs.cards.hidden.remove(&Card::Workflow);
        self.prefs.cards.collapsed.remove(&Card::Workflow);
        self.prefs
            .cards
            .order
            .retain(|card| *card != Card::Workflow);
        self.prefs.cards.order.insert(0, Card::Workflow);
        self.tasks.save();
    }
    pub(super) fn task_watch_source(&mut self) {
        let target = self
            .tasks
            .current()
            .filter(|t| matches!(t.phase, Phase::Ready | Phase::Accepted))
            .and_then(|t| t.workspace.as_ref().map(|p| (t.id.clone(), p.clone())));
        if target == self.tasks.watched {
            return;
        }
        self.tasks.watcher = None;
        self.tasks.changes = None;
        self.tasks.watched = target.clone();
        let Some((_, root)) = target else {
            return;
        };
        if self.demo {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        self.tasks.watcher = Some(crate::file_watch::Watcher::new_resolved(
            &self.ctx,
            move || domain::workspace::fingerprint_root(&root),
            notify::RecursiveMode::Recursive,
            move |event: notify::Result<notify::Event>| {
                // Lost/error events also require a fresh fingerprint; never
                // interpret missing notification details as unchanged evidence.
                if !matches!(event, Ok(ref event) if matches!(event.kind, notify::EventKind::Access(_)))
                {
                    let _ = tx.send(());
                    ctx.request_repaint();
                }
            },
        ));
        self.tasks.changes = Some(rx);
        self.tasks.revalidate_at = Some(std::time::Instant::now());
    }
    pub(super) fn poll_tasks(&mut self) {
        if let Some(status) = self
            .tasks
            .watcher
            .as_ref()
            .and_then(|watcher| watcher.poll())
        {
            match status {
                Ok(()) => self.tasks.revalidate_at = Some(std::time::Instant::now()),
                Err(_) => {
                    self.tasks.watcher = None;
                    self.tasks.changes = None;
                    // Setup may fail after the initial fingerprint completed.
                    // Close that gap before leaving only reopening as fallback.
                    self.tasks.revalidate_at = Some(std::time::Instant::now());
                    self.tasks.error = Some("Source watching unavailable. Reopening this workflow recomputes its evidence fingerprint.".into());
                }
            }
        }
        self.tasks.persist_loaded();
        self.poll_task_operations();
        self.task_poll_recovery();
        self.poll_task_updates();
        self.poll_task_attachment();
        self.tasks
            .retiring
            .retain(|rx| matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        let mut linked = Vec::new();
        for task in &mut self.tasks.records {
            if task.parent_session.is_none()
                && !self.tasks.operations.quarantined.contains(&task.id)
                && self.tasks.attachment.pending_id() != Some(task.id.as_str())
            {
                if let Some(path) = self
                    .tasks
                    .parents
                    .get(&task.id)
                    .and_then(|id| self.agents.nodes.get(id))
                    .and_then(|n| n.path.clone())
                {
                    task.parent_session = Some(path);
                    linked.push(task.clone());
                }
            }
        }
        if let Some(store) = &self.tasks.store {
            for task in linked {
                if store.send(StoreCommand::Save(Box::new(task))).is_err() {
                    self.tasks.storage_failed = true;
                    self.tasks.error = Some("Cannot save task parent relationship.".into());
                }
            }
        }
        if let Some((run, rx)) = &self.tasks.preview {
            match rx.try_recv() {
                Ok(Ok(preview)) => {
                    self.tasks.histories.insert(
                        run.clone(),
                        State {
                            messages: preview.messages,
                            ..Default::default()
                        },
                    );
                    self.tasks.preview = None;
                }
                Ok(Err(e)) => {
                    self.tasks.error = Some(e);
                    self.tasks.preview = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.tasks.preview = None;
                }
                Err(_) => {}
            }
        }
        if self
            .tasks
            .changes
            .as_ref()
            .is_some_and(|rx| rx.try_iter().count() > 0)
        {
            self.tasks.revalidate_at = Some(std::time::Instant::now() + Duration::from_millis(250));
        }
        if self.tasks.storage_failed && self.tasks.running() {
            self.tasks.block("Task persistence failed; execution stopped. Inspect the storage error before starting another task.");
        }
        if let Some(job) = &self.tasks.job {
            let result = match job.rx.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Task worker disconnected. No work was retried.".into()))
                }
                Err(_) => None,
            };
            if let Some(result) = result {
                let id = job.task.clone();
                self.tasks.job = None;
                if self.tasks.live.as_ref() == Some(&id) {
                    self.task_job_result(result);
                }
            }
        }
        if let Some((id, rx)) = &self.tasks.revalidate_claim {
            let result = match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Workflow ownership check disconnected. No evidence was changed.".into(),
                )),
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                let id = id.clone();
                self.tasks.revalidate_claim = None;
                match result {
                    Ok(())
                        if self.tasks.live.is_none()
                            && self.tasks.selected.as_ref() == Some(&id) =>
                    {
                        self.task_fingerprint_owned(FingerprintStep::Revalidate)
                    }
                    Ok(()) => {}
                    Err(error) => {
                        self.state.notice = Some(format!(
                            "Could not revalidate this workflow: {error} Other workflows are still available."
                        ))
                    }
                }
            }
        }
        self.task_poll_processes();
        self.finish_workflow_requests();
        if let Some(deadline) = self.tasks.revalidate_at {
            if std::time::Instant::now() >= deadline {
                self.tasks.revalidate_at = None;
                if self.tasks.live.is_none()
                    && self
                        .tasks
                        .current()
                        .is_some_and(|t| matches!(t.phase, Phase::Ready | Phase::Accepted))
                {
                    self.task_fingerprint(FingerprintStep::Revalidate);
                }
            } else {
                self.ctx.request_repaint_after(
                    deadline.saturating_duration_since(std::time::Instant::now()),
                );
            }
        }
    }
}
