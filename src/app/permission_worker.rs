//! Explicit, idle-only access upgrades. Never replay a delegated task or a user prompt.
use super::*;
use settings::PermissionMode;

pub(super) struct Change {
    result: Receiver<Result<PathBuf, String>>,
    mode: PermissionMode,
}
impl Pui {
    pub(super) fn agent_settings(&self, id: &str) -> settings::Settings {
        let mut settings = self.prefs.settings.clone();
        if let Some(node) = self.agents.nodes.get(id) {
            if node.cloud_host.is_some()
                || (id == self.agents.active && self.active_cloud_host().is_some())
            {
                settings.permission = PermissionMode::Auto;
            } else if node.access == "read-only" {
                settings.permission = PermissionMode::Plan;
            } else if let Some(permission) = node.permission {
                settings.permission = permission;
            }
        } else if id == self.agents.active && self.active_cloud_host().is_some() {
            settings.permission = PermissionMode::Auto;
        }
        settings
    }
    pub(super) fn active_permission(&self) -> PermissionMode {
        self.agent_settings(&self.agents.active).permission
    }
    pub(super) fn readonly_worker(&self) -> bool {
        self.agents
            .nodes
            .get(&self.agents.active)
            .is_some_and(|n| n.access == "read-only")
    }
    pub(super) fn set_permission(&mut self, mode: PermissionMode) {
        if self.agents.nodes.contains_key(&self.agents.active) {
            if self.readonly_worker() && mode != PermissionMode::Plan {
                return;
            }
            if self.is_mirror() {
                let mut settings = self.agent_settings(&self.agents.active);
                settings.permission = mode;
                self.remote_action(sync::Action::Settings(settings));
                return;
            }
            self.agents
                .nodes
                .get_mut(&self.agents.active)
                .unwrap()
                .permission = Some(mode);
            self.prefs.agent_tree = self.agent_records();
            if !self.side_chat.rendering {
                self.task_apply_settings();
            }
            if self.client.is_some() {
                self.send(json!({"type":"prompt","message":format!("/pui-settings {}",self.agent_settings(&self.agents.active).payload())}));
            }
        } else {
            self.prefs.settings.permission = mode;
        }
    }
    /// New chats start in the most recently selected mode (Auto on first launch),
    /// like the model. Chats still inheriting the old default keep it explicitly.
    /// A user's explicit mode choice: applies to this chat and becomes the
    /// default for new chats. Host actions (e.g. plan approval) use
    /// `set_permission` and never change the default.
    pub(super) fn select_permission(&mut self, mode: PermissionMode) {
        let writable_local = self.agents.nodes.contains_key(&self.agents.active)
            && !self.readonly_worker()
            && !self.is_mirror();
        self.set_permission(mode);
        if self.side_chat.rendering {
            return;
        }
        if writable_local && self.active_permission() == mode {
            self.remember_default_permission(mode);
            self.prefs.agent_tree = self.agent_records();
        }
    }
    fn remember_default_permission(&mut self, mode: PermissionMode) {
        let previous = self.prefs.settings.permission;
        // The last explicit choice (including Bypass) becomes the default for
        // new chats. Workers inherit their parent chat's mode, not this default.
        if previous == mode {
            return;
        }
        for node in self.agents.nodes.values_mut() {
            if node.permission.is_none() && node.access != "read-only" {
                node.permission = Some(previous);
            }
        }
        self.prefs.settings.permission = mode;
    }
    fn worker_upgrade_blocked(&self) -> Option<&'static str> {
        if self.agents.exiting {
            return Some("Pui is exiting. Worker access cannot be changed now.");
        }
        if self.demo {
            return Some("Design preview only. No worker access can be changed here.");
        }
        if self.is_mirror() {
            return Some("Open this worker in its owning Pui window to change its access.");
        }
        if !self.state.ready()
            || self.resource_reload.pending()
            || self.loading_chat
            || self.history_pending.is_some()
            || self.interrupt.is_some()
            || !self.deliveries.is_empty()
            || !self.state.dialogs.is_empty()
            || self.agents_still_retiring()
        {
            return Some(
                "Stop this worker and wait for it to become idle before changing its access.",
            );
        }
        let Some(node) = self.agents.nodes.get(&self.agents.active) else {
            return Some("Worker is unavailable.");
        };
        if node.path.is_none()
            || node
                .path
                .as_ref()
                .is_some_and(|p| self.deletion.deleting_chat(p))
            || self.deletion.deleting_agent(&node.id)
            || node.status == "Queued"
            || node.setup_pending()
            || self.agent_mail_pending(&node.id)
            || self.agents.permission_changes.contains_key(&node.id)
        {
            return Some("Wait for this worker's saved session and workspace to become available.");
        }
        None
    }
    pub(super) fn worker_upgrade_confirmation(&mut self, ui: &mut egui::Ui) {
        let Some((scope, mode)) = self.worker_upgrade_pending.clone() else {
            return;
        };
        if scope != self.permission_scope() || !self.readonly_worker() {
            self.worker_upgrade_pending = None;
            return;
        }
        let blocked = self.worker_upgrade_blocked();
        let frame = ui.ctx().cumulative_frame_nr();
        let first = ui.ctx().data_mut(|data| {
            let id = egui::Id::new("worker-upgrade-focus");
            let previous = data.get_temp::<(String, u64)>(id);
            data.insert_temp(id, (scope.clone(), frame));
            previous.is_none_or(|(previous_scope, previous_frame)| {
                previous_scope != scope || previous_frame.saturating_add(1) < frame
            })
        });
        prompt_ui::inline(ui.ctx()).show(ui, |ui| {
            prompt_ui::style(ui);
            confirmations::heading(ui, "Enable writable worker?", mode.label(), true);
            ui.add_space(12.0);
            confirmations::note(ui, "Only this worker will reconnect with shell and write tools in an isolated Git worktree created from committed HEAD. Uncommitted changes are not copied. Its conversation and draft are kept; nothing is automatically sent or rerun. Parent and sibling modes stay unchanged.");
            if mode == PermissionMode::Bypass {
                confirmations::note(ui, "Bypass skips all Pui approval checks for this worker, including commands, deletion, credentials and network access. Only enable this for work you trust.");
            }
            if let Some(reason) = blocked { confirmations::note(ui, reason); }
            ui.add_space(14.0);
            let choice = if prompt_ui::keyboard_available(ui) && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                Some(confirmations::Choice::Cancel)
            } else {
                confirmations::actions(ui, "Cancel", "Enable writable worker", true, blocked.is_none(), first)
            };
            match choice {
                Some(confirmations::Choice::Cancel) => self.worker_upgrade_pending = None,
                Some(confirmations::Choice::Confirm) if blocked.is_none() => {
                    self.worker_upgrade_pending = None;
                    if let Err(error) = self.begin_worker_upgrade(mode) { self.state.error = Some(error); }
                }
                _ => {}
            }
        });
    }
    pub(super) fn begin_worker_upgrade(&mut self, mode: PermissionMode) -> Result<(), String> {
        if !self.readonly_worker() || mode == PermissionMode::Plan {
            return Err("This worker does not need an access upgrade.".into());
        }
        if let Some(reason) = self.worker_upgrade_blocked() {
            return Err(reason.into());
        }
        let id = self.agents.active.clone();
        let node = &self.agents.nodes[&id];
        let path = node.path.as_ref().ok_or("Worker has no saved session")?;
        if self.sync.enabled() && !self.sync.owns(path) {
            return Err("Worker ownership is unavailable.".into());
        }
        let session = path.clone();
        let cwd = node.cwd.clone();
        let worktree = node.worktree.clone();
        let client = self.client.take();
        let (tx, result) = mpsc::channel();
        self.agents
            .permission_changes
            .insert(id.clone(), Change { result, mode });
        self.loading_chat = true;
        self.connected = false;
        self.agents.nodes.get_mut(&id).unwrap().status = "Changing access…".into();
        let ctx = self.ctx.clone();
        let worker = std::thread::spawn(move || {
            drop(client); // All old writers must exit before reconnecting the session.
            let result = worktree
                .map(Ok)
                .unwrap_or_else(|| agents::create_worktree(&cwd, &id))
                .and_then(|path| {
                    relocate_session(&session, &path)?;
                    Ok(path)
                });
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.agents.permission_workers.push(worker);
        Ok(())
    }
    pub(super) fn poll_permission_changes(&mut self) {
        if self.agents.exiting {
            return;
        }
        let mut index = 0;
        while index < self.agents.permission_workers.len() {
            if self.agents.permission_workers[index].is_finished() {
                let _ = self.agents.permission_workers.remove(index).join();
            } else {
                index += 1;
            }
        }
        let ready: Vec<_> = self
            .agents
            .permission_changes
            .iter()
            .filter_map(|(id, change)| match change.result.try_recv() {
                Ok(result) => Some((id.clone(), result)),
                Err(mpsc::TryRecvError::Disconnected) => Some((
                    id.clone(),
                    Err("Worker access setup disconnected; read-only access was kept.".into()),
                )),
                Err(mpsc::TryRecvError::Empty) => None,
            })
            .collect();
        for (id, result) in ready {
            let change = self.agents.permission_changes.remove(&id).unwrap();
            let Some(node) = self.agents.nodes.get_mut(&id) else {
                continue;
            };
            let error = match result {
                Ok(path) => {
                    node.cwd = path.clone();
                    node.worktree = Some(path);
                    node.access = "worktree".into();
                    node.permission = Some(change.mode);
                    node.launch = Some(agents::worker_launch(false, &node.name, None, None));
                    None
                }
                Err(error) => Some(error),
            };
            node.stopped = true;
            node.status = "Stopped".into();
            let path = node.path.clone();
            if let Some(path) = path {
                self.with_live_chat(&path, |app| {
                    app.loading_chat = false;
                    app.prefs.cwd = app.agents.nodes[&id].cwd.clone();
                    app.resume_agent();
                    if let Some(error) = error {
                        app.state.error = Some(error);
                    }
                });
            }
            if self.agents.active == id {
                self.prefs.cwd = self.agents.nodes[&id].cwd.clone();
                self.workspace_rx = None;
                self.refresh_workspace();
            }
            self.prefs.agent_tree = self.agent_records();
        }
    }
}

// Pi restores cwd from the session header, not the process working directory.
// Move only that metadata after all old writers exit, preserving the entire
// transcript and session identity. Atomic replacement leaves failures read-only.
fn relocate_session(session: &std::path::Path, cwd: &std::path::Path) -> Result<(), String> {
    use std::io::Write;
    sessions::validate_delete(session)?;
    let bytes = std::fs::read(session).map_err(|e| format!("Cannot read worker session: {e}"))?;
    let end = bytes
        .iter()
        .position(|b| *b == b'\n')
        .unwrap_or(bytes.len());
    let mut header: Value =
        serde_json::from_slice(&bytes[..end]).map_err(|_| "Invalid worker session header")?;
    if header["type"] != "session" {
        return Err("Invalid worker session header".into());
    }
    header["cwd"] = json!(cwd);
    let temp = session.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.set_permissions(
            std::fs::metadata(session)
                .map_err(|e| e.to_string())?
                .permissions(),
        )
        .map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut file, &header).map_err(|e| e.to_string())?;
        file.write_all(&bytes[end..]).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temp, session).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
#[path = "permission_worker_tests.rs"]
mod tests;
