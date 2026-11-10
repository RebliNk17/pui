//! Explicit, confirmed deletion. Filesystem work and Pi shutdown stay off the UI thread.
use super::*;
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Chat(PathBuf),
    Task(String),
}
#[derive(Default)]
pub(super) struct View {
    pub pending: Option<Target>,
    jobs: Vec<(Target, Receiver<Result<(), String>>)>,
}
impl View {
    // Task deletion retains its existing modal behavior; chats never block the workspace.
    pub fn in_progress(&self) -> bool {
        self.jobs
            .iter()
            .any(|(target, _)| matches!(target, Target::Task(_)))
    }
    pub fn confirming(&self) -> bool {
        self.pending.is_some() || self.in_progress()
    }
    fn contains(&self, target: &Target) -> bool {
        self.jobs
            .iter()
            .any(|(pending, _)| match (pending, target) {
                (Target::Chat(a), Target::Chat(b)) => same(a, b),
                _ => pending == target,
            })
    }
    pub fn deleting_chat(&self, path: &Path) -> bool {
        self.contains(&Target::Chat(path.into()))
    }
}
fn same(a: &Path, b: &Path) -> bool {
    sync::identity(a) == sync::identity(b)
}
impl Pui {
    pub(super) fn request_delete(&mut self, target: Target) {
        if !self.deletion.in_progress() && !self.deletion.contains(&target) {
            self.deletion.pending = Some(target);
            self.model_picker_open = false;
        }
    }
    fn deletion_blocked(&self, target: &Target) -> Option<String> {
        if self.deletion.contains(target) {
            return Some("This item is already being deleted.".into());
        }
        if self.demo {
            return Some("Design preview only. No saved files can be deleted here.".into());
        }
        if self.tasks.loading_history() {
            return Some("Wait for task history to finish loading.".into());
        }
        match target {
            Target::Task(id) => self.tasks.delete_blocked(id),
            Target::Chat(path) => {
                if self.agents_still_retiring() {
                    return Some("Wait for stopped chat processes to exit before deleting.".into());
                }
                if self
                    .waiting
                    .iter()
                    .any(|(p, messages)| same(p, path) && !messages.is_empty())
                {
                    return Some("Clear this chat's queued messages before deleting it.".into());
                }
                if self.tasks.records.iter().any(|t| {
                    t.runs
                        .iter()
                        .any(|r| r.session.as_ref().is_some_and(|p| same(p, path)))
                }) {
                    return Some(
                        "This transcript belongs to a task. Delete the task instead.".into(),
                    );
                }
                if self.tasks.records.iter().any(|t| {
                    t.phase.active() && t.parent_session.as_ref().is_some_and(|p| same(p, path))
                }) {
                    return Some("Stop this chat's active task before deleting the chat.".into());
                }
                if self
                    .state
                    .session_file
                    .as_deref()
                    .is_some_and(|p| same(Path::new(p), path))
                    && (!self.state.ready()
                        || self.loading_chat
                        || self.history_pending.is_some()
                        || self.interrupt.is_some()
                        || !self.deliveries.is_empty()
                        || !self.state.dialogs.is_empty())
                {
                    return Some(
                        "Stop this chat and wait for it to become idle before deleting it.".into(),
                    );
                }
                if self.agents.parked.values().any(|c| {
                    c.state
                        .session_file
                        .as_deref()
                        .is_some_and(|p| same(Path::new(p), path))
                        && (!c.state.ready() || c.deletion_busy())
                }) {
                    return Some(
                        "Stop this chat and wait for it to become idle before deleting it.".into(),
                    );
                }
                let ids: BTreeSet<_> = self
                    .agents
                    .nodes
                    .values()
                    .filter(|n| n.path.as_ref().is_some_and(|p| same(p, path)))
                    .map(|n| n.id.clone())
                    .collect();
                if self.agents.nodes.values().any(|n| {
                    n.parent.as_ref().is_some_and(|p| ids.contains(p))
                        && !n.stopped
                        && !matches!(n.status.as_str(), "Idle" | "Saved" | "Finished")
                }) {
                    return Some("Stop linked workers before deleting their parent chat.".into());
                }
                None
            }
        }
    }
    pub(super) fn poll_deletions(&mut self) {
        let mut index = 0;
        while index < self.deletion.jobs.len() {
            let result = match self.deletion.jobs[index].1.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Disconnected) => Err(
                    "Deletion worker disconnected. Check the saved files before retrying.".into(),
                ),
                Err(mpsc::TryRecvError::Empty) => {
                    index += 1;
                    continue;
                }
            };
            let (target, _) = self.deletion.jobs.remove(index);
            match result {
                Ok(()) => {
                    self.finish_delete(&target);
                    if matches!(target, Target::Chat(_))
                        && notifications::Notifications::supported()
                    {
                        self.notifications.complete(
                            &self.ctx,
                            notifications::NotificationSettings {
                                sound: self.prefs.notifications.sound,
                                desktop: false,
                            },
                            "Chat deleted",
                            None,
                        );
                    }
                }
                Err(error) => self.state.error = Some(format!("Deletion failed: {error}")),
            }
        }
    }
    pub(super) fn deletion_dialog(&mut self, ctx: &egui::Context) {
        let Some(target) = self.deletion.pending.clone().or_else(|| {
            self.deletion
                .jobs
                .iter()
                .find_map(|(t, _)| matches!(t, Target::Task(_)).then(|| t.clone()))
        }) else {
            return;
        };
        let working = self.deletion.contains(&target);
        let blocked = self.deletion_blocked(&target);
        let (title, description, confirm, name) = match &target {
            Target::Chat(path) => (
                "Delete chat?",
                "Permanently delete this chat's saved Pi transcript and remove it from Pui. This cannot be undone.",
                "Delete chat",
                self.prefs
                    .chats
                    .get(path)
                    .and_then(|p| p.title.clone())
                    .or_else(|| {
                        self.sessions
                            .iter()
                            .find(|s| same(&s.path, path))
                            .map(|s| s.title.clone())
                    })
                    .unwrap_or_else(|| "Untitled session".into()),
            ),
            Target::Task(id) => (
                "Delete workflow history?",
                "Permanently delete this workflow history, its checks/results and its saved run transcripts. This cannot be undone.",
                "Delete workflow history",
                self.tasks
                    .records
                    .iter()
                    .find(|t| &t.id == id)
                    .map(|t| {
                        if t.objective.is_empty() {
                            format!("Task {}", &id[..id.len().min(8)])
                        } else {
                            t.objective.clone()
                        }
                    })
                    .unwrap_or_else(|| "Task no longer available".into()),
            ),
        };
        let choice = confirmations::Dialog::new("delete-item", title, description, "Cancel", if working { "Deleting…" } else { confirm })
            .enabled(!working && blocked.is_none())
            .show(ctx, |ui| {
                ui.add(egui::Label::new(RichText::new(name).strong()).wrap());
                if let Target::Chat(path) = &target { ui.add(egui::Label::new(theme::small(ui, path.display().to_string())).wrap()); }
                confirmations::note(ui, "Project files, Git worktrees and other chats are kept. Deleting a chat keeps its tasks and worker chats as separate items. Close any external terminal Pi process using this session first.");
                if let Some(reason) = &blocked { ui.add(egui::Label::new(theme::small(ui, reason)).wrap()); }
            });
        if !working {
            match choice {
                Some(confirmations::Choice::Cancel) => self.deletion.pending = None,
                Some(confirmations::Choice::Confirm) => {
                    self.deletion.pending = None;
                    if let Err(error) = self.begin_delete(target) {
                        self.state.error = Some(error);
                    }
                }
                None => {}
            }
        }
    }
    fn begin_delete(&mut self, target: Target) -> Result<(), String> {
        if let Some(reason) = self.deletion_blocked(&target) {
            return Err(reason);
        }
        let ctx = self.ctx.clone();
        let rx = match &target {
            Target::Task(id) => {
                let task = self
                    .tasks
                    .records
                    .iter()
                    .find(|t| &t.id == id)
                    .ok_or("Task no longer available")?;
                let paths: Vec<_> = task
                    .runs
                    .iter()
                    .filter_map(|r| r.session.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                for path in &paths {
                    if task.parent_session.as_ref().is_some_and(|p| same(p, path)) {
                        return Err(
                            "Task run points at its parent chat; deletion was refused.".into()
                        );
                    }
                    if self.tasks.records.iter().any(|t| {
                        &t.id != id
                            && (t.parent_session.as_ref().is_some_and(|p| same(p, path))
                                || t.runs
                                    .iter()
                                    .any(|r| r.session.as_ref().is_some_and(|p| same(p, path))))
                    }) {
                        return Err(
                            "A run transcript is shared with another task; deletion was refused."
                                .into(),
                        );
                    }
                    if self
                        .agents
                        .nodes
                        .values()
                        .any(|n| n.path.as_ref().is_some_and(|p| same(p, path)))
                        || self
                            .state
                            .session_file
                            .as_deref()
                            .is_some_and(|p| same(Path::new(p), path))
                    {
                        return Err("A task transcript is also open as a chat. Close that chat before deleting the task.".into());
                    }
                }
                let locks = paths
                    .iter()
                    .map(|p| self.sync.deletion_lock(p))
                    .collect::<Result<Vec<_>, _>>()?;
                self.tasks.delete_saved(id, paths, locks)?
            }
            Target::Chat(path) => {
                sessions::validate_delete(path)?;
                let lock = self.sync.deletion_lock(path)?;
                let detach = self.tasks.prepare_detach_deleted_chat(path)?;
                let mut clients = Vec::new();
                if self
                    .state
                    .session_file
                    .as_deref()
                    .is_some_and(|p| same(Path::new(p), path))
                {
                    if let Some(client) = self.client.take() {
                        clients.push(client);
                    }
                    self.connected = false;
                }
                for chat in self.agents.parked.values_mut().filter(|c| {
                    c.state
                        .session_file
                        .as_deref()
                        .is_some_and(|p| same(Path::new(p), path))
                }) {
                    if let Some(client) = chat.client.take() {
                        clients.push(client);
                    }
                }
                let (tx, rx) = mpsc::channel();
                let path = path.clone();
                std::thread::spawn(move || {
                    // Hold ownership until all writers have exited and the file is gone.
                    let _lock = lock;
                    drop(clients);
                    let result = (|| {
                        if let Some(detach) = detach {
                            detach.recv().map_err(
                                |_| "Cannot detach linked tasks; chat was not deleted",
                            )??;
                        }
                        sessions::delete(&path)
                    })();
                    let _ = tx.send(result);
                    ctx.request_repaint();
                });
                rx
            }
        };
        self.deletion.pending = None;
        self.deletion.jobs.push((target.clone(), rx));
        if let Target::Chat(path) = &target {
            // Detach in-memory links now too, so edits made while the worker
            // runs cannot save a stale parent relationship back to task storage.
            self.tasks.detach_deleted_chat(path);
            if self
                .state
                .session_file
                .as_deref()
                .is_some_and(|p| same(Path::new(p), path))
            {
                let models = self.state.models.clone();
                self.ensure_agent();
                self.park_tree();
                self.state.models = models;
                self.new_chat = true;
                self.output_revision = u64::MAX;
                self.focus_composer = true;
                // Select saved history, not a newly registered blank chat.
                // Keep the deleting chat parked until its worker completes.
                if let Some(next) = self
                    .sessions
                    .iter()
                    .find(|s| {
                        !self.deletion.deleting_chat(&s.path)
                            && self
                                .prefs
                                .chats
                                .get(&s.path)
                                .is_none_or(|meta| !meta.hidden && !meta.archived)
                    })
                    .cloned()
                {
                    self.open_chat(next);
                }
            }
        }
        Ok(())
    }
    fn forget_session(&mut self, path: &Path) {
        self.sessions.retain(|s| !same(&s.path, path));
        self.prefs.chats.retain(|p, _| !same(p, path));
        self.waiting.retain(|p, _| !same(p, path));
        if self
            .prefs
            .last_session
            .as_ref()
            .is_some_and(|p| same(p, path))
        {
            self.prefs.last_session = None;
        }
        self.session_rx = None; // Discard scans started before deletion.
        self.sync.forget_deleted(path);
    }
    fn finish_delete(&mut self, target: &Target) {
        match target {
            Target::Task(id) => {
                let paths: Vec<_> = self
                    .tasks
                    .records
                    .iter()
                    .find(|t| &t.id == id)
                    .into_iter()
                    .flat_map(|t| t.runs.iter().filter_map(|r| r.session.clone()))
                    .collect();
                self.tasks.forget_deleted(id);
                self.prefs.closed_threads.remove(&format!("task:{id}"));
                for path in paths {
                    self.forget_session(&path);
                }
                self.state.notice = Some("Task and saved run transcripts deleted. Project files and worktrees were kept.".into());
            }
            Target::Chat(path) => {
                let active = self
                    .state
                    .session_file
                    .as_deref()
                    .is_some_and(|p| same(Path::new(p), path));
                self.forget_session(path);
                self.forget_deleted_agents(path);
                self.tasks.detach_deleted_chat(path);
                if active {
                    let models = std::mem::take(&mut self.state.models);
                    self.state = State {
                        models,
                        ..Default::default()
                    };
                    self.new_chat = true;
                    self.pending_session = None;
                    self.preview_rx = None;
                    self.transcript.clear();
                    self.draft.clear();
                    self.draft_images.clear();
                    self.image_picker_rx = None;
                    self.completions = Default::default();
                    self.interrupt = None;
                    self.deliveries.clear();
                    self.history_pending = None;
                    self.history_view = Default::default();
                    self.history_drafts.clear();
                    self.completion_run = Default::default();
                    self.questions = Default::default();
                    self.current_dialog.clear();
                    self.pending_dialog = None;
                    self.recall = Default::default();
                    self.connected = false;
                    self.loading_chat = false;
                    self.bypass_pending = None;
                    self.output_revision = u64::MAX;
                }
                self.state.notice =
                    Some("Chat deleted. Project files and worktrees were kept.".into());
            }
        }
        self.refresh_sessions();
    }
}

#[cfg(test)]
#[path = "deletion_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "deletion_background_tests.rs"]
mod background_tests;
