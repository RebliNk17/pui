//! Explicit, confirmed deletion. Filesystem work and Pi shutdown stay off the UI thread.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Chat(PathBuf),
    Task(String),
}
#[derive(Default)]
struct ChatScope {
    // Descendants first: a failed deletion must not remove their parent.
    paths: Vec<PathBuf>,
    members: BTreeMap<String, Option<PathBuf>>,
    completed: Option<Receiver<PathBuf>>,
}
impl ChatScope {
    fn contains(&self, path: &Path) -> bool {
        self.paths.iter().any(|p| same(p, path))
    }
}
#[derive(Default)]
pub(super) struct View {
    pub pending: Option<Target>,
    jobs: Vec<(Target, Receiver<Result<(), String>>)>,
    chats: BTreeMap<PathBuf, ChatScope>,
    empty_chats: BTreeMap<PathBuf, Receiver<Result<(), String>>>,
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
            || self.empty_chats.keys().any(|pending| same(pending, path))
            || self.chats.values().any(|scope| scope.contains(path))
    }
    pub fn deleting_agent(&self, id: &str) -> bool {
        self.chats
            .values()
            .any(|scope| scope.members.contains_key(id))
    }
}
fn same(a: &Path, b: &Path) -> bool {
    sync::identity(a) == sync::identity(b)
}
fn chat_deletion_locks<L>(
    paths: &[PathBuf],
    mut lock: impl FnMut(&Path) -> Result<L, String>,
) -> Result<Vec<L>, String> {
    for path in paths {
        sessions::validate_delete(path)?;
    }
    paths.iter().map(|path| lock(path)).collect()
}
fn delete_chat_paths(paths: &[PathBuf], mut completed: impl FnMut(&Path)) -> Result<(), String> {
    for (index, path) in paths.iter().enumerate() {
        if let Err(error) = sessions::delete(path) {
            return Err(format!(
                "Deleted {index} of {} transcripts. Failed at {}: {error}. Remaining chats were kept; retry deletion to finish.",
                paths.len(),
                path.display()
            ));
        }
        completed(path);
    }
    Ok(())
}
impl Pui {
    fn chat_delete_scope(&self, path: &Path) -> ChatScope {
        // Include persisted-only nodes too (for example beyond the restore limit).
        let mut records: BTreeMap<_, _> = self
            .prefs
            .agent_tree
            .iter()
            .map(|n| (n.id.clone(), (n.parent.clone(), Some(n.path.clone()))))
            .collect();
        for node in self.agents.nodes.values() {
            let live_path = if node.id == self.agents.active {
                self.state.session_file.as_ref().map(PathBuf::from)
            } else {
                self.agents
                    .parked
                    .get(&node.id)
                    .and_then(|c| c.state.session_file.as_ref().map(PathBuf::from))
            };
            let saved_path = records.get(&node.id).and_then(|(_, path)| path.clone());
            records.insert(
                node.id.clone(),
                (
                    node.parent.clone(),
                    node.path.clone().or(live_path).or(saved_path),
                ),
            );
        }
        let mut ids = BTreeSet::new();
        let mut paths = vec![path.to_path_buf()];
        loop {
            let previous = ids.len();
            for (id, (parent, file)) in &records {
                if parent.as_ref().is_some_and(|p| ids.contains(p))
                    || file
                        .as_ref()
                        .is_some_and(|p| paths.iter().any(|q| same(p, q)))
                {
                    ids.insert(id.clone());
                    if let Some(file) = file {
                        if !paths.iter().any(|p| same(p, file)) {
                            paths.push(file.clone());
                        }
                    }
                }
            }
            if ids.len() == previous {
                break;
            }
        }
        // Rank each path by its shallowest node. Shared transcripts are deleted once.
        let depth = |id: &str| {
            let mut seen = BTreeSet::new();
            let mut cursor = Some(id.to_string());
            while let Some(id) = cursor.filter(|id| ids.contains(id)) {
                if !seen.insert(id.clone()) {
                    break;
                }
                cursor = records.get(&id).and_then(|(parent, _)| parent.clone());
            }
            seen.len()
        };
        paths.sort_by_key(|path| {
            std::cmp::Reverse(
                records
                    .iter()
                    .filter(|(id, (_, p))| {
                        ids.contains(*id) && p.as_ref().is_some_and(|p| same(p, path))
                    })
                    .map(|(id, _)| depth(id))
                    .min()
                    .unwrap_or(0),
            )
        });
        ChatScope {
            paths,
            members: ids
                .into_iter()
                .map(|id| {
                    // Pathless saved/queued nodes belong to their nearest saved ancestor.
                    let mut cursor = Some(id.clone());
                    let mut seen = BTreeSet::new();
                    let mut path = None;
                    while let Some(next) = cursor {
                        if !seen.insert(next.clone()) {
                            break;
                        }
                        let Some((parent, file)) = records.get(&next) else {
                            break;
                        };
                        if file.is_some() {
                            path = file.clone();
                            break;
                        }
                        cursor = parent.clone();
                    }
                    (id, path)
                })
                .collect(),
            completed: None,
        }
    }
    pub(super) fn request_delete(&mut self, target: Target) {
        if !self.deletion.in_progress() && !self.deletion.contains(&target) {
            self.deletion.pending = Some(target);
            self.close_model_popovers();
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
                let scope = self.chat_delete_scope(path);
                for path in &scope.paths {
                    if self.deletion.deleting_chat(path) {
                        return Some("This chat or a descendant is already being deleted.".into());
                    }
                    if let Some(reason) = self.chat_deletion_blocked(path) {
                        return Some(reason);
                    }
                }
                if scope.members.keys().any(|id| {
                    self.deletion.deleting_agent(id)
                        || self.agents.permission_changes.contains_key(id)
                        || self.agent_mail_pending(id)
                        || (id == &self.agents.active
                            && (!self.state.ready()
                                || self.loading_chat
                                || self.history_pending.is_some()
                                || self.interrupt.is_some()
                                || !self.deliveries.is_empty()
                                || !self.state.dialogs.is_empty()))
                        || self
                            .agents
                            .parked
                            .get(id)
                            .is_some_and(|c| !c.state.ready() || c.deletion_busy())
                        || self.agents.nodes.get(id).is_some_and(|n| {
                            !n.path.as_ref().is_some_and(|p| same(p, path))
                                && !n.stopped
                                && !matches!(n.status.as_str(), "Idle" | "Saved" | "Finished")
                        })
                }) {
                    return Some(
                        "Stop this chat and all descendant workers before deleting.".into(),
                    );
                }
                None
            }
        }
    }
    fn chat_deletion_blocked(&self, path: &Path) -> Option<String> {
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
            return Some("This transcript belongs to a task. Delete the task instead.".into());
        }
        if self
            .tasks
            .records
            .iter()
            .any(|t| t.phase.active() && t.parent_session.as_ref().is_some_and(|p| same(p, path)))
        {
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
        None
    }
    pub(super) fn begin_empty_chat_cleanup(&mut self, path: PathBuf) -> bool {
        if self.deletion.deleting_chat(&path) {
            return false;
        }
        let Ok(lock) = self.sync.deletion_lock(&path) else {
            return false;
        };
        let client = self.client.take();
        let (tx, rx) = mpsc::channel();
        self.deletion.empty_chats.insert(path.clone(), rx);
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _lock = lock;
            // Stop the only writer before checking the entire on-disk session.
            drop(client);
            let _ = tx.send(sessions::delete_if_empty(&path));
            ctx.request_repaint();
        });
        true
    }
    pub(super) fn poll_deletions(&mut self) {
        let finished: Vec<_> = self
            .deletion
            .empty_chats
            .iter()
            .filter_map(|(path, rx)| match rx.try_recv() {
                Ok(result) => Some((path.clone(), result)),
                Err(mpsc::TryRecvError::Disconnected) => Some((
                    path.clone(),
                    Err("Empty chat cleanup interrupted; saved history was kept.".into()),
                )),
                Err(mpsc::TryRecvError::Empty) => None,
            })
            .collect();
        for (path, result) in finished {
            self.deletion.empty_chats.remove(&path);
            match result {
                Ok(()) => self.forget_session(&path),
                Err(error) => self.state.notice = Some(error),
            }
            self.refresh_sessions();
        }
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
            let scope = match &target {
                Target::Chat(path) => self.deletion.chats.remove(&sync::identity(path)),
                Target::Task(_) => None,
            };
            if let Some(scope) = &scope {
                let deleted = if result.is_ok() {
                    scope.paths.clone()
                } else {
                    scope
                        .completed
                        .as_ref()
                        .map(|rx| rx.try_iter().collect())
                        .unwrap_or_default()
                };
                let ids = scope
                    .members
                    .iter()
                    .filter(|(_, path)| {
                        result.is_ok()
                            || path
                                .as_ref()
                                .is_some_and(|p| deleted.iter().any(|d| same(d, p)))
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                self.finish_deleted_chats(&deleted, &ids);
            }
            match result {
                Ok(()) => {
                    if scope.is_none() {
                        self.finish_delete(&target);
                    } else {
                        self.state.notice = Some("Chat and descendant chats deleted. Workflow history, project files and worktrees were kept.".into());
                    }
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
                "Permanently delete this chat and every descendant worker chat, including their saved Pi transcripts. This cannot be undone.",
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
                if let Target::Chat(path) = &target {
                    let scope = self.chat_delete_scope(path);
                    confirmations::note(ui, &format!("Includes {} saved transcript(s) across this chat and {} descendant chat(s). Workflow history, project files, Git worktrees and unrelated chats are kept. Close external terminal Pi processes using any of these sessions first.", scope.paths.len(), scope.members.len().saturating_sub(1)));
                } else {
                    confirmations::note(ui, "Project files, Git worktrees and other chats are kept. Close external terminal Pi processes using these sessions first.");
                }
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
                let mut scope = self.chat_delete_scope(path);
                let locks = chat_deletion_locks(&scope.paths, |p| self.sync.deletion_lock(p))?;
                // All validation and ownership checks precede task or client changes.
                self.prepare_delete_chat_jobs(
                    &scope.members.keys().cloned().collect(),
                    &scope.paths,
                )?;
                let detach = scope
                    .paths
                    .iter()
                    .map(|p| self.tasks.prepare_detach_deleted_chat(p))
                    .collect::<Result<Vec<_>, _>>()?;
                if self.agents.active.is_empty()
                    && self
                        .state
                        .session_file
                        .as_deref()
                        .is_some_and(|p| scope.contains(Path::new(p)))
                {
                    let file = self.state.session_file.as_ref().map(PathBuf::from);
                    let id = self.ensure_agent();
                    scope.members.insert(id, file);
                }
                let ids: Vec<_> = scope.members.keys().cloned().collect();
                for id in ids {
                    if let Some(node) = self.agents.nodes.get_mut(&id) {
                        node.stopped = true;
                    }
                }
                let mut clients = Vec::new();
                if self
                    .state
                    .session_file
                    .as_deref()
                    .is_some_and(|p| scope.contains(Path::new(p)))
                    || scope.members.contains_key(&self.agents.active)
                {
                    if let Some(client) = self.client.take() {
                        clients.push(client);
                    }
                    self.connected = false;
                }
                for (_, chat) in self.agents.parked.iter_mut().filter(|(id, c)| {
                    scope.members.contains_key(*id)
                        || c.state
                            .session_file
                            .as_deref()
                            .is_some_and(|p| scope.contains(Path::new(p)))
                }) {
                    if let Some(client) = chat.client.take() {
                        clients.push(client);
                    }
                }
                let (tx, rx) = mpsc::channel();
                let paths = scope.paths.clone();
                let (progress, completed) = mpsc::channel();
                scope.completed = Some(completed);
                self.deletion.chats.insert(sync::identity(path), scope);
                std::thread::spawn(move || {
                    // Hold every lock until all writers exit and deletion completes.
                    let _locks = locks;
                    drop(clients);
                    let result = (|| {
                        for detach in detach.into_iter().flatten() {
                            detach.recv().map_err(
                                |_| "Cannot detach linked tasks; chats were not deleted",
                            )??;
                        }
                        delete_chat_paths(&paths, |path| {
                            let _ = progress.send(path.to_path_buf());
                        })
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
            let paths = self.deletion.chats[&sync::identity(path)].paths.clone();
            for path in paths {
                self.tasks.detach_deleted_chat(&path);
            }
            if self
                .state
                .session_file
                .as_deref()
                .is_some_and(|p| self.deletion.deleting_chat(Path::new(p)))
                || self.deletion.deleting_agent(&self.agents.active)
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
    fn finish_deleted_chats(&mut self, paths: &[PathBuf], ids: &BTreeSet<String>) {
        let active = ids.contains(&self.agents.active)
            || self
                .state
                .session_file
                .as_deref()
                .is_some_and(|p| paths.iter().any(|path| same(Path::new(p), path)));
        for path in paths {
            self.forget_session(path);
            self.tasks.detach_deleted_chat(path);
        }
        for id in ids {
            self.session_layouts.forget_agent(id);
            self.prefs
                .workspace_layouts
                .retain(|layout| layout.key != format!("agent:{id}"));
        }
        self.forget_deleted_agent_ids(ids);
        if active {
            self.files = Default::default();
            self.changes.stale_review_comments();
            let review_comments = std::mem::take(&mut self.changes.review_comments);
            self.changes = Default::default();
            self.changes.review_comments = review_comments;
            self.inspector_panel = false;
            let models = std::mem::take(&mut self.state.models);
            self.state = State {
                models,
                ..Default::default()
            };
            self.new_chat = true;
            self.pending_session = None;
            self.preview_rx = None;
            self.transcript.clear();
            self.context_chips.clear();
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
            self.attention_observed = Default::default();
            self.questions = Default::default();
            self.current_dialog.clear();
            self.pending_dialog = None;
            self.recall = Default::default();
            self.connected = false;
            self.loading_chat = false;
            self.bypass_pending = None;
            self.worker_upgrade_pending = None;
            self.output_revision = u64::MAX;
        }
        self.refresh_sessions();
    }
    fn forget_session(&mut self, path: &Path) {
        self.attention
            .forget_path(&self.sync.attention_identity(path));
        self.session_layouts.forget_path(path);
        self.prefs.workspace_layouts.retain(|layout| {
            layout.key.starts_with("agent:") || !same(Path::new(&layout.key), path)
        });
        self.forget_restart_checkpoint(path);
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
                if let Some(scope) = self.deletion.chats.remove(&sync::identity(path)) {
                    self.finish_deleted_chats(
                        &scope.paths,
                        &scope.members.keys().cloned().collect(),
                    );
                } else {
                    let ids = self
                        .chat_delete_scope(path)
                        .members
                        .keys()
                        .cloned()
                        .collect();
                    self.finish_deleted_chats(std::slice::from_ref(path), &ids);
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

#[cfg(test)]
#[path = "deletion_tree_tests.rs"]
mod tree_tests;
