//! App-owned task supervisor. Pi still owns the agent loop and tool permissions.
use super::*;
use crate::tasks::{
    self as domain, Attempt, Check, Contract, Phase, Review, Role, Run, Task, Verdict,
};
use std::collections::BTreeMap;

mod chat;
#[cfg(not(test))]
mod proposal;
mod routing;
mod runtime;
#[cfg(test)]
mod tests;
mod view;
mod workflow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FingerprintStep {
    Candidate,
    AfterChecks,
    AfterReview,
    Accept,
    Revalidate,
}
enum JobResult {
    Inspected(domain::workspace::Inspection),
    Prepared(PathBuf, bool),
    Fingerprint(FingerprintStep, String),
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
    Detach {
        tasks: Vec<Task>,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Delete {
        id: String,
        sessions: Vec<PathBuf>,
        locks: Vec<Option<std::fs::File>>,
        reply: mpsc::Sender<Result<(), String>>,
    },
}
#[derive(Default)]
pub(super) struct Tasks {
    pub open: bool,
    pub selected: Option<String>,
    pub(super) records: Vec<Task>,
    pub(super) parents: BTreeMap<String, String>,
    live: Option<String>,
    processes: Vec<Process>,
    retiring: Vec<Receiver<()>>,
    histories: BTreeMap<String, State>,
    pub(super) selected_run: Option<String>,
    pub(super) transcript: Transcript,
    job: Option<Job>,
    store: Option<mpsc::Sender<StoreCommand>>,
    store_worker: Option<std::thread::JoinHandle<()>>,
    loaded: Option<Receiver<Result<Vec<Task>, String>>>,
    errors: Option<Receiver<String>>,
    pub error: Option<String>,
    storage_failed: bool,
    pub dialog_text: String,
    pub current_dialog: String,
    questions: questions::View,
    dirty_confirmed: bool,
    pub revalidate_at: Option<std::time::Instant>,
    preview: Option<(String, Receiver<Result<crate::history::Preview, String>>)>,
    watched: Option<String>,
    watcher: Option<notify::RecommendedWatcher>,
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
        let ctx = ctx.clone();
        result.store_worker = Some(std::thread::spawn(move || {
            let _ = loaded_tx.send(domain::store::load(&home));
            ctx.request_repaint();
            run_store(&home, rx, errors, &ctx);
        }));
        result.store = Some(tx);
        result.loaded = Some(loaded);
        result.errors = Some(error_rx);
        result
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
        &self,
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
        store
            .send(StoreCommand::Detach {
                tasks: linked,
                reply,
            })
            .map_err(|_| "Task storage unavailable")?;
        Ok(Some(rx))
    }
    pub(super) fn detach_deleted_chat(&mut self, path: &std::path::Path) {
        for task in &mut self.records {
            if task
                .parent_session
                .as_ref()
                .is_some_and(|p| sync::identity(p) == sync::identity(path))
            {
                task.parent_session = None;
                self.parents.remove(&task.id);
                if let Some(store) = &self.store {
                    if store
                        .send(StoreCommand::Save(Box::new(task.clone())))
                        .is_err()
                    {
                        self.error = Some("Cannot save detached task relationship.".into());
                    }
                }
            }
        }
    }
    pub fn running(&self) -> bool {
        self.live_task().is_some_and(|t| t.phase.active())
    }
    pub fn stop(&mut self) {
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
        }
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
        let reason = reason.into();
        if let Some(t) = self.live_mut() {
            for run in &mut t.runs {
                if matches!(run.status.as_str(), "Connecting" | "Working") {
                    run.status = "Blocked".into();
                }
            }
            t.phase = Phase::Blocked;
            t.note = reason;
        }
        self.save();
        self.retire();
        self.live = None;
        self.job = None;
    }
    fn persist_loaded(&mut self) {
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
        if let Some(rx) = &self.errors {
            for e in rx.try_iter() {
                self.error = Some(e);
                self.storage_failed = true;
            }
        }
    }
    pub fn shutdown(&mut self) {
        if self.live.is_some() {
            if let Some(t) = self.live_mut().filter(|t| t.phase.active()) {
                t.phase = Phase::Interrupted;
                t.note = "Pui closed during execution. No automatic replay.".into();
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
    errors: mpsc::Sender<String>,
    ctx: &egui::Context,
) {
    let mut deleted = std::collections::BTreeSet::new();
    let mut owned = std::collections::BTreeMap::new();
    while let Ok(command) = rx.recv() {
        match command {
            StoreCommand::Save(task) => {
                if !deleted.contains(&task.id) {
                    let result = (|| {
                        if !owned.contains_key(&task.id) {
                            owned.insert(task.id.clone(), domain::store::lock(home, &task.id)?);
                        }
                        domain::store::save(home, &task)
                    })();
                    if let Err(error) = result {
                        let _ = errors.send(error);
                    }
                }
            }
            StoreCommand::Detach { mut tasks, reply } => {
                let result = (|| {
                    for task in &tasks {
                        if !owned.contains_key(&task.id) {
                            owned.insert(task.id.clone(), domain::store::lock(home, &task.id)?);
                        }
                    }
                    for task in &mut tasks {
                        task.parent_session = None;
                        domain::store::save(home, task)?;
                    }
                    Ok(())
                })();
                let _ = reply.send(result);
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
                    activity: "Acceptance checks need your approval".into(),
                    model: task.model.clone(),
                    parent: None,
                    action: "Review acceptance checks",
                    path: None,
                });
            }
        }
    }
    pub(super) fn open_task_from_board(&mut self, task: String, run: Option<String>) {
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
        if !name.starts_with("Task ") {
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

    pub(super) fn new_task(&mut self) {
        if self.deletion.in_progress() {
            return;
        }
        self.kanban.open = false;
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
        use notify::Watcher;
        let target = self
            .tasks
            .current()
            .filter(|t| matches!(t.phase, Phase::Ready | Phase::Accepted))
            .and_then(|t| t.workspace.as_ref().map(|p| (t.id.clone(), p.clone())));
        if target.as_ref().map(|(id, _)| id) == self.tasks.watched.as_ref() {
            return;
        }
        self.tasks.watcher = None;
        self.tasks.changes = None;
        self.tasks.watched = target.as_ref().map(|(id, _)| id.clone());
        let Some((_, root)) = target else {
            return;
        };
        if self.demo {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        match notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                if !matches!(event.kind, notify::EventKind::Access(_)) {
                    let _ = tx.send(());
                    ctx.request_repaint();
                }
            }
        })
        .and_then(|mut watcher| {
            watcher.watch(&root, notify::RecursiveMode::Recursive)?;
            Ok(watcher)
        }) {
            Ok(watcher) => {
                self.tasks.watcher = Some(watcher);
                self.tasks.changes = Some(rx);
                self.tasks.revalidate_at = Some(std::time::Instant::now());
            }
            Err(_) => {
                self.tasks.error = Some("Source watching unavailable. Use Revalidate source; acceptance always recomputes the fingerprint.".into());
            }
        }
    }
    pub(super) fn poll_tasks(&mut self) {
        self.tasks.persist_loaded();
        self.tasks
            .retiring
            .retain(|rx| matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        let mut linked = Vec::new();
        for task in &mut self.tasks.records {
            if task.parent_session.is_none() {
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
        self.task_poll_processes();
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
