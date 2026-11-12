//! Event-driven shared history refresh. All directory reads stay on the store worker.
use super::*;
use std::collections::BTreeSet;

pub(super) struct Snapshot {
    pub records: Vec<Task>,
    pub owned: BTreeSet<String>,
}
pub(super) struct Watch {
    watcher: crate::file_watch::Watcher,
    events: Receiver<()>,
}
#[derive(Default)]
pub(super) struct Updates {
    pub setup: Option<Receiver<Watch>>,
    watch: Option<Watch>,
    pub(super) pending: Option<Receiver<Result<Snapshot, String>>>,
    deadline: Option<std::time::Instant>,
    pub requested: bool,
}
pub(super) fn watch(home: PathBuf, ctx: &egui::Context) -> Watch {
    // A burst of atomic saves needs one refresh, not an unbounded event queue.
    let (tx, events) = mpsc::sync_channel(1);
    let repaint = ctx.clone();
    let watcher = crate::file_watch::Watcher::new(
        home,
        notify::RecursiveMode::NonRecursive,
        ctx,
        move |event: notify::Result<notify::Event>| {
            let relevant = event.as_ref().map_or(true, |event| {
                !matches!(event.kind, notify::EventKind::Access(_))
                    && event.paths.iter().any(|path| {
                        path.extension()
                            .is_some_and(|ext| ext == "json" || ext == "deleted")
                    })
            });
            if relevant {
                let _ = tx.try_send(());
                repaint.request_repaint();
            }
        },
    );
    Watch { watcher, events }
}
impl Tasks {
    fn merge_history(&mut self, snapshot: Snapshot) {
        // Locally queued writes and live jobs win over any earlier disk snapshot.
        let protected = |id: &str| {
            snapshot.owned.contains(id)
                || self.local_owned.contains(id)
                || self.live.as_deref() == Some(id)
                || self.job.as_ref().is_some_and(|job| job.task == id)
                || self.attachment.pending_id() == Some(id)
                || self
                    .revalidate_claim
                    .as_ref()
                    .is_some_and(|(claim, _)| claim == id)
        };
        let keep: BTreeSet<_> = self
            .records
            .iter()
            .filter(|t| protected(&t.id))
            .map(|t| t.id.clone())
            .collect();
        let incoming: BTreeSet<_> = snapshot.records.iter().map(|t| t.id.clone()).collect();
        let removed: Vec<_> = self
            .records
            .iter()
            .filter(|t| !keep.contains(&t.id) && !incoming.contains(&t.id))
            .map(|t| t.id.clone())
            .collect();
        for id in removed {
            self.forget_deleted(&id);
        }
        for task in snapshot.records {
            if keep.contains(&task.id) {
                continue;
            }
            if let Some(existing) = self.records.iter_mut().find(|t| t.id == task.id) {
                if existing.parent_session != task.parent_session {
                    // An in-memory fallback must never resurrect the old chat link.
                    self.parents.remove(&task.id);
                }
                if serde_json::to_value(&*existing).ok() != serde_json::to_value(&task).ok() {
                    for run in existing.runs.iter().chain(&task.runs) {
                        self.histories.remove(&run.id);
                    }
                    *existing = task;
                }
            } else {
                self.records.push(task);
            }
        }
    }
}
impl Pui {
    pub(super) fn poll_task_updates(&mut self) {
        if let Some(setup) = &self.tasks.updates.setup {
            if let Ok(watch) = setup.try_recv() {
                self.tasks.updates.watch = Some(watch);
                self.tasks.updates.setup = None;
            }
        }
        if let Some(watch) = &self.tasks.updates.watch {
            if let Some(status) = watch.watcher.poll() {
                match status {
                    Ok(()) => self.tasks.updates.requested = true,
                    Err(_) => self.state.notice = Some("Workflow history watching is unavailable. Opening a workflow refreshes retained history.".into()),
                }
            }
            if watch.events.try_recv().is_ok() {
                // Fixed debounce window, not extended indefinitely by busy writers.
                self.tasks
                    .updates
                    .deadline
                    .get_or_insert_with(|| std::time::Instant::now() + Duration::from_millis(150));
            }
        }
        if let Some(pending) = &self.tasks.updates.pending {
            let result = match pending.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Workflow refresh disconnected.".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                self.tasks.updates.pending = None;
                match result {
                    Ok(snapshot) => self.tasks.merge_history(snapshot),
                    Err(error) => self.state.notice = Some(error),
                }
            }
        }
        if let Some(deadline) = self.tasks.updates.deadline {
            if deadline <= std::time::Instant::now() {
                self.tasks.updates.requested = true;
                self.tasks.updates.deadline = None;
            } else {
                self.ctx.request_repaint_after(
                    deadline.saturating_duration_since(std::time::Instant::now()),
                );
            }
        }
        if self.tasks.updates.requested
            && self.tasks.updates.pending.is_none()
            && self.tasks.loaded.is_none()
        {
            self.tasks.updates.requested = false;
            if let Some(store) = &self.tasks.store {
                let (reply, rx) = mpsc::channel();
                if store.send(StoreCommand::Refresh { reply }).is_ok() {
                    self.tasks.updates.pending = Some(rx);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "refresh_tests.rs"]
mod tests;
