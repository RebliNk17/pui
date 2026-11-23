//! Bridges Pui Desktop's live app state with the opt-in `crate::remote::RemoteServer`,
//! handles incoming mobile commands, stamps session origins, and renders Settings → Remote & Mobile.
use super::*;
use crate::remote::{
    BindMode, OriginPlatform, RemoteCommand, RemoteSnapshot, SessionDetail, SessionOrigin,
    SessionSummary, WaitingItem, extract_artifacts, qr,
};
use base64::Engine as _;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::OpenOptions,
    io::Write as _,
    time::UNIX_EPOCH,
};

impl Pui {
    pub(super) fn session_origin_for(&self, path: &std::path::Path) -> SessionOrigin {
        if let Some(origin) = self
            .prefs
            .chats
            .get(path)
            .and_then(|meta| meta.origin.clone())
        {
            return origin;
        }
        if let Some(origin) = sessions::origin_for(path) {
            return origin;
        }
        SessionOrigin::desktop()
    }

    pub(super) fn record_session_origin(&mut self, path: &std::path::Path, origin: SessionOrigin) {
        let origin = origin.sanitized();
        let entry = self.prefs.chats.entry(path.to_path_buf()).or_default();
        if entry.origin.is_some() {
            return;
        }
        entry.origin = Some(origin.clone());
        sessions::remember_origin(path, origin.clone());
        // Best-effort durable stamp into Pi's JSONL file so origin survives even if preferences reset.
        if path.is_file() {
            if let Ok(mut file) = OpenOptions::new().append(true).open(path) {
                if let Ok(line) = serde_json::to_string(&origin.custom_jsonl_entry()) {
                    let _ = writeln!(file, "{line}");
                }
            }
        }
    }

    fn flush_pending_origins(&mut self) {
        if self.pending_origins.is_empty() {
            return;
        }
        let active_id = self.agents.active.clone();
        if let Some(path_str) = self.state.session_file.clone() {
            if let Some(origin) = self.pending_origins.remove(&active_id) {
                self.record_session_origin(std::path::Path::new(&path_str), origin);
            }
        }
        let pairs: Vec<(String, PathBuf)> = self
            .agents
            .nodes
            .iter()
            .filter_map(|(id, node)| {
                self.pending_origins.contains_key(id).then(|| {
                    node.path
                        .clone()
                        .or_else(|| {
                            self.agents
                                .parked
                                .get(id)
                                .and_then(|c| c.state.session_file.as_ref().map(PathBuf::from))
                        })
                        .map(|p| (id.clone(), p))
                })?
            })
            .collect();
        for (id, path) in pairs {
            if let Some(origin) = self.pending_origins.remove(&id) {
                self.record_session_origin(&path, origin);
            }
        }
    }

    pub(super) fn poll_remote(&mut self) {
        self.flush_pending_origins();
        let commands: Vec<RemoteCommand> = self.remote.cmds.try_iter().collect();
        if !commands.is_empty() {
            for cmd in commands {
                self.handle_remote_command(cmd);
            }
            self.flush_pending_origins();
        }
        if self.remote.is_running() {
            self.publish_remote_snapshot();
        }
    }

    /// Ensures a saved session is loaded as a live Pi agent in `self.agents.parked` (or already active)
    /// WITHOUT switching Desktop's visible chat, draft, or cwd.
    fn ensure_background_live_chat(&mut self, path: &std::path::Path) -> bool {
        if self
            .state
            .session_file
            .as_deref()
            .is_some_and(|p| sidebar_tree::same_path(std::path::Path::new(p), path))
        {
            return true;
        }
        if let Some(id) = self.agent_for_path(path) {
            if let Some(parked) = self.agents.parked.get(&id) {
                let needs_reconnect = parked.client.is_none();
                if self.agents.idle.dormant.contains(&id) {
                    let wake_id = id.clone();
                    let path_buf = path.to_path_buf();
                    let _ = self.with_live_chat(path, |app| {
                        let _ = app.wake_idle_agent(&wake_id);
                        if app.state.messages.is_empty() {
                            if let Ok(preview) = crate::history::load(&path_buf) {
                                app.state.messages = preview.messages;
                                app.state.outputs_revision =
                                    app.state.outputs_revision.wrapping_add(1);
                            }
                        }
                    });
                    return true;
                }
                if needs_reconnect {
                    let path_buf = path.to_path_buf();
                    let _ = self.with_live_chat(path, |app| {
                        let ctx = app.ctx.clone();
                        app.connect(&ctx, Some(path_buf.clone()));
                        if app.state.messages.is_empty() {
                            if let Ok(preview) = crate::history::load(&path_buf) {
                                app.state.messages = preview.messages;
                                app.state.outputs_revision =
                                    app.state.outputs_revision.wrapping_add(1);
                            }
                        }
                    });
                }
                return true;
            }
        }
        let Some(session) = self
            .sessions
            .iter()
            .find(|s| sidebar_tree::same_path(&s.path, path))
            .cloned()
        else {
            return false;
        };

        // Snapshot Desktop's current visible chat and spawn the mobile-selected session parked in background.
        let original_active = self.ensure_agent();
        let original_cwd = self.prefs.cwd.clone();
        let original_bg = self.agents.background;
        let visible = self.take_chat();

        self.agents.active.clear();
        self.prefs.cwd = session.cwd.clone();
        self.new_chat = false;
        self.state = State::default();
        self.state.session_file = Some(path.display().to_string());
        self.state.session_name = session.title.clone();
        let new_id = self.ensure_agent();
        if let Some(node) = self.agents.nodes.get_mut(&new_id) {
            node.path = Some(path.to_path_buf());
            node.name = session.title;
        }
        self.agents.background = true;
        let ctx = self.ctx.clone();
        self.connect(&ctx, Some(path.to_path_buf()));
        // Pre-populate messages from disk immediately so mobile never sees a blank frame
        // while the background Pi process boots and replies to get_entries/get_state.
        if self.state.messages.is_empty() {
            if let Ok(preview) = crate::history::load(path) {
                self.state.messages = preview.messages;
                self.state.outputs_revision = self.state.outputs_revision.wrapping_add(1);
            }
        }
        let parked_chat = self.take_chat();
        self.agents.parked.insert(new_id, parked_chat);

        self.put_chat(visible);
        self.agents.active = original_active;
        self.prefs.cwd = original_cwd;
        self.agents.background = original_bg;
        true
    }

    /// Runs `action` against `target_path` (if provided) inside a background swap so Desktop's
    /// active chat is never disturbed. Falls back to active chat only when `target_path` is None.
    fn run_on_remote_target(
        &mut self,
        target_path: Option<&std::path::Path>,
        action: impl FnOnce(&mut Self),
    ) {
        if let Some(path) = target_path {
            if self.ensure_background_live_chat(path) {
                let _ = self.with_live_chat(path, action);
            }
        } else {
            action(self);
        }
    }

    fn handle_remote_command(&mut self, cmd: RemoteCommand) {
        match cmd {
            RemoteCommand::CreateSession {
                cwd,
                permission_mode,
                prompt,
                origin,
            } => {
                let target_cwd = if cwd.is_dir() {
                    sync::identity(&cwd)
                } else {
                    self.prefs.cwd.clone()
                };
                let perm = permission_mode
                    .as_deref()
                    .and_then(settings::PermissionMode::from_key);

                // Create the new phone session parked in background so Desktop stays on its current chat!
                let original_active = self.ensure_agent();
                let original_cwd = self.prefs.cwd.clone();
                let original_bg = self.agents.background;
                let visible = self.take_chat();

                self.agents.active.clear();
                self.prefs.cwd = target_cwd;
                self.new_chat = true;
                self.state = State::default();
                let new_id = self.ensure_agent();
                if let Some(node) = self.agents.nodes.get_mut(&new_id) {
                    node.permission = perm;
                    node.name = "New conversation".into();
                }
                self.agents.background = true;
                let ctx = self.ctx.clone();
                self.connect(&ctx, None);
                if let Some(path_str) = self.state.session_file.clone() {
                    self.record_session_origin(std::path::Path::new(&path_str), origin);
                } else {
                    self.pending_origins.insert(new_id.clone(), origin);
                }
                if let Some(text) = prompt {
                    self.draft = text;
                    self.submit_mode(&ctx, self.prefs.settings.delivery);
                }
                let parked_chat = self.take_chat();
                self.agents.parked.insert(new_id, parked_chat);

                // Restore Desktop's visible chat immediately.
                self.put_chat(visible);
                self.agents.active = original_active;
                self.prefs.cwd = original_cwd;
                self.agents.background = original_bg;
                self.refresh_sessions();
            }
            RemoteCommand::OpenSession { path } => {
                // Load the session in background for mobile without switching Desktop's active chat!
                let _ = self.ensure_background_live_chat(&path);
            }
            RemoteCommand::SendMessage {
                path,
                text,
                mode,
                images,
                origin,
            } => {
                self.run_on_remote_target(path.as_deref(), |app| {
                    if let Some(origin) = origin {
                        if let Some(path_str) = app.state.session_file.clone() {
                            let p = std::path::Path::new(&path_str);
                            if app.prefs.chats.get(p).is_none_or(|m| m.origin.is_none())
                                && app.state.messages.is_empty()
                            {
                                app.record_session_origin(p, origin);
                            }
                        }
                    }
                    let delivery = match mode.as_deref() {
                        Some("queue") => settings::DeliveryMode::Queue,
                        Some("steer") => settings::DeliveryMode::Steer,
                        _ => app.prefs.settings.delivery,
                    };
                    if !images.is_empty() {
                        let mut decoded = Vec::new();
                        for img in images {
                            if let Ok(bytes) = base64::engine::general_purpose::STANDARD
                                .decode(img.data_base64.trim())
                            {
                                if let Ok(att) = crate::media::Attachment::from_bytes(&bytes) {
                                    decoded.push(att);
                                }
                            }
                        }
                        if !decoded.is_empty() {
                            app.draft_images.set_images(decoded);
                        }
                    }
                    app.draft = text;
                    let ctx = app.ctx.clone();
                    app.submit_mode(&ctx, delivery);
                });
            }
            RemoteCommand::Abort { path } => {
                self.run_on_remote_target(path.as_deref(), |app| {
                    app.stop();
                });
            }
            RemoteCommand::AnswerDialog {
                path,
                dialog_id,
                response,
            } => {
                self.run_on_remote_target(path.as_deref(), |app| {
                    app.answer_dialog(&dialog_id, response);
                });
            }
            RemoteCommand::UpdateSettings {
                path,
                permission_mode,
                model,
                provider,
                thinking,
            } => {
                self.run_on_remote_target(path.as_deref(), |app| {
                    if let Some(key) = permission_mode.as_deref() {
                        if let Some(mode) = settings::PermissionMode::from_key(key) {
                            app.select_permission(mode);
                            app.state.permission_mode = Some(mode.key().to_owned());
                        }
                    }
                    if let Some(model_id) = model {
                        let prov = provider.unwrap_or_else(|| app.state.provider.clone());
                        if !prov.is_empty() {
                            app.send(json!({
                                "type": "set_model",
                                "provider": prov,
                                "modelId": model_id,
                            }));
                        }
                    }
                    if let Some(level) = thinking {
                        app.send(json!({
                            "type": "set_thinking_level",
                            "level": level,
                        }));
                    }
                });
            }
            RemoteCommand::RemoveQueued { path, index } => {
                let target = path.or_else(|| self.state.session_file.as_ref().map(PathBuf::from));
                if let Some(target_path) = target {
                    let key = self
                        .waiting
                        .keys()
                        .find(|p| sidebar_tree::same_path(p, &target_path))
                        .cloned();
                    if let Some(key) = key {
                        if let Some(queue) = self.waiting.get_mut(&key) {
                            if index < queue.len() {
                                queue.remove(index);
                            }
                            if queue.is_empty() {
                                self.waiting.remove(&key);
                            }
                        }
                    }
                }
            }
            RemoteCommand::SteerQueued { path, index } => {
                let target = path.or_else(|| self.state.session_file.as_ref().map(PathBuf::from));
                if let Some(target_path) = target {
                    let key = self
                        .waiting
                        .keys()
                        .find(|p| sidebar_tree::same_path(p, &target_path))
                        .cloned();
                    if let Some(key) = key {
                        if let Some(queue) = self.waiting.get_mut(&key) {
                            if let Some(item) = queue.get_mut(index) {
                                item.mode = settings::DeliveryMode::Steer;
                            }
                        }
                    }
                }
            }
            RemoteCommand::RefreshUsage => {
                let ctx = self.ctx.clone();
                self.usage.refresh(&ctx, true);
            }
        }
    }

    fn waiting_items_for(&self, path: &std::path::Path) -> Vec<WaitingItem> {
        self.waiting
            .iter()
            .find(|(p, _)| sidebar_tree::same_path(p, path))
            .map(|(_, list)| {
                list.iter()
                    .enumerate()
                    .map(|(index, msg)| WaitingItem {
                        index,
                        text: msg.text.clone(),
                        mode: msg.mode.label().to_ascii_lowercase(),
                        image_count: msg.images.len(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn waiting_fingerprint(items: &[WaitingItem]) -> u64 {
        let mut h: u64 = (items.len() as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        for item in items {
            h = h.rotate_left(7) ^ (item.index as u64) ^ (item.image_count as u64).rotate_left(13);
            for b in item.mode.as_bytes() {
                h = h.wrapping_mul(31).wrapping_add(u64::from(*b));
            }
            for b in item.text.as_bytes() {
                h = h.wrapping_mul(31).wrapping_add(u64::from(*b));
            }
        }
        h
    }

    fn detail_fingerprint(state: &State, waiting: &[WaitingItem]) -> u64 {
        let mut h = state
            .transcript_revision
            .wrapping_mul(31)
            .wrapping_add(state.outputs_revision)
            .wrapping_add((state.messages.len() as u64).rotate_left(17))
            .wrapping_add((state.models.len() as u64).rotate_left(25))
            .wrapping_add(u64::from(state.busy) << 56)
            .wrapping_add((state.dialogs.len() as u64) << 48)
            .wrapping_add((state.pending_messages.len() as u64) << 40)
            ^ Self::waiting_fingerprint(waiting);
        for s in [
            state.model.as_str(),
            state.thinking.as_str(),
            state.permission_mode.as_deref().unwrap_or(""),
        ] {
            for b in s.as_bytes() {
                h = h.wrapping_mul(31).wrapping_add(u64::from(*b));
            }
        }
        h
    }

    fn publish_remote_snapshot(&mut self) {
        self.remote_snapshot_tick = self.remote_snapshot_tick.wrapping_add(1);
        let active_path = self.state.session_file.clone();
        let active_waiting = active_path
            .as_deref()
            .map(|p| self.waiting_items_for(std::path::Path::new(p)))
            .unwrap_or_default();
        let active_detail_rev = Self::detail_fingerprint(&self.state, &active_waiting);

        let mut details = BTreeMap::new();
        let mut allowed_artifacts = BTreeSet::new();

        if let Some(path_str) = &active_path {
            let path = PathBuf::from(path_str);
            let origin = self
                .pending_origins
                .get(&self.agents.active)
                .cloned()
                .unwrap_or_else(|| self.session_origin_for(&path));
            let (artifacts, paths) = extract_artifacts(&self.state.messages);
            allowed_artifacts.extend(paths);
            let title = self
                .prefs
                .chats
                .get(&path)
                .and_then(|m| m.title.clone())
                .or_else(|| {
                    (!self.state.session_name.is_empty()).then(|| self.state.session_name.clone())
                })
                .unwrap_or_else(|| "New conversation".into());
            let (status_label, _) = self.run_label();
            let detail = SessionDetail {
                path: path_str.clone(),
                title,
                cwd: self.prefs.cwd.to_string_lossy().into_owned(),
                project: self.prefs.cwd.to_string_lossy().into_owned(),
                status: status_label.to_owned(),
                busy: self.state.busy,
                ready: self.state.ready(),
                model: self.state.model.clone(),
                model_name: self.state.model_name.clone(),
                provider: self.state.provider.clone(),
                thinking: self.state.thinking.clone(),
                levels: self.state.levels.clone(),
                permission_mode: self
                    .state
                    .permission_mode
                    .clone()
                    .unwrap_or_else(|| self.active_permission().key().to_owned()),
                cost: self.state.cost,
                context_tokens: self.state.context_tokens,
                context_window: self.state.context_window,
                error: self.state.error.clone(),
                notice: self.state.notice.clone(),
                origin,
                messages: self.state.messages.clone(),
                pending_messages: self.state.pending_messages.clone(),
                waiting_messages: active_waiting,
                dialogs: self.state.dialogs.iter().cloned().collect(),
                commands: self.state.commands.clone(),
                models: self.state.models.clone(),
                artifacts,
                revision: active_detail_rev,
            };
            details.insert(path_str.clone(), detail);
        }

        // Also expose live parked agents so mobile can view running background chats seamlessly.
        // Skip evicted/stopped parked shells (client.is_none() with cleared messages) so they don't
        // shadow disk history in GET /v1/sessions/detail.
        for (id, chat) in &self.agents.parked {
            let Some(path_str) = chat.state.session_file.clone() else {
                continue;
            };
            if details.contains_key(&path_str) {
                continue;
            }
            if chat.client.is_none() && chat.state.messages.is_empty() {
                continue;
            }
            let path = PathBuf::from(&path_str);
            let origin = self
                .pending_origins
                .get(id)
                .cloned()
                .unwrap_or_else(|| self.session_origin_for(&path));
            let (artifacts, paths) = extract_artifacts(&chat.state.messages);
            allowed_artifacts.extend(paths);
            let node = self.agents.nodes.get(id);
            let cwd = node
                .map(|n| n.cwd.to_string_lossy().into_owned())
                .unwrap_or_default();
            let title = self
                .prefs
                .chats
                .get(&path)
                .and_then(|m| m.title.clone())
                .or_else(|| node.map(|n| n.name.clone()))
                .unwrap_or_else(|| "Conversation".into());
            let parked_waiting = self.waiting_items_for(&path);
            let detail_rev = Self::detail_fingerprint(&chat.state, &parked_waiting);
            details.insert(
                path_str.clone(),
                SessionDetail {
                    path: path_str,
                    title,
                    cwd: cwd.clone(),
                    project: cwd,
                    status: node
                        .map(|n| n.status.clone())
                        .unwrap_or_else(|| "Idle".into()),
                    busy: chat.state.busy,
                    ready: chat.state.ready(),
                    model: chat.state.model.clone(),
                    model_name: chat.state.model_name.clone(),
                    provider: chat.state.provider.clone(),
                    thinking: chat.state.thinking.clone(),
                    levels: chat.state.levels.clone(),
                    permission_mode: chat
                        .state
                        .permission_mode
                        .clone()
                        .unwrap_or_else(|| self.agent_settings(id).permission.key().to_owned()),
                    cost: chat.state.cost,
                    context_tokens: chat.state.context_tokens,
                    context_window: chat.state.context_window,
                    error: chat.state.error.clone(),
                    notice: chat.state.notice.clone(),
                    origin,
                    messages: chat.state.messages.clone(),
                    pending_messages: chat.state.pending_messages.clone(),
                    waiting_messages: parked_waiting,
                    dialogs: chat.state.dialogs.iter().cloned().collect(),
                    commands: chat.state.commands.clone(),
                    models: chat.state.models.clone(),
                    artifacts,
                    revision: detail_rev,
                },
            );
        }

        let mut seen_paths = BTreeSet::new();
        let mut sessions = Vec::new();
        for s in &self.sessions {
            let path_str = s.path.to_string_lossy().into_owned();
            if !seen_paths.insert(path_str.clone()) {
                continue;
            }
            let is_active = active_path
                .as_deref()
                .is_some_and(|p| sidebar_tree::same_path(std::path::Path::new(p), &s.path));
            let node = self.agents.nodes.values().find(|n| {
                n.path
                    .as_deref()
                    .is_some_and(|p| sidebar_tree::same_path(p, &s.path))
            });
            let detail = details.get(&path_str);
            let status = if let Some(d) = detail {
                d.status.clone()
            } else if let Some(n) = node {
                n.status.clone()
            } else {
                "Saved".into()
            };
            let busy = detail.is_some_and(|d| d.busy);
            let has_pending_dialog = detail.is_some_and(|d| !d.dialogs.is_empty());
            let unread = self.saved_unread(&s.path);
            let origin = detail
                .map(|d| d.origin.clone())
                .unwrap_or_else(|| self.session_origin_for(&s.path));
            let parent_session = self
                .prefs
                .chats
                .get(&s.path)
                .and_then(|m| m.parent_session.as_ref())
                .map(|p| p.to_string_lossy().into_owned());
            let modified_ms = s
                .modified
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let created_ms = s
                .created
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64);
            sessions.push(SessionSummary {
                path: path_str,
                title: self.session_title(s),
                project: s.project.to_string_lossy().into_owned(),
                project_name: sessions::project_name(&s.project),
                cwd: s.cwd.to_string_lossy().into_owned(),
                status,
                busy,
                unread,
                has_pending_dialog,
                model: detail.map(|d| d.model.clone()).unwrap_or_default(),
                modified_ms,
                created_ms,
                origin,
                parent_session,
                is_active,
            });
        }

        // Ensure a brand-new live session that hasn't been discovered on disk yet is still listed!
        for (path_str, detail) in &details {
            if seen_paths.insert(path_str.clone()) {
                sessions.insert(
                    0,
                    SessionSummary {
                        path: path_str.clone(),
                        title: detail.title.clone(),
                        project: detail.project.clone(),
                        project_name: sessions::project_name(std::path::Path::new(&detail.project)),
                        cwd: detail.cwd.clone(),
                        status: detail.status.clone(),
                        busy: detail.busy,
                        unread: false,
                        has_pending_dialog: !detail.dialogs.is_empty(),
                        model: detail.model.clone(),
                        modified_ms: chrono::Utc::now().timestamp_millis(),
                        created_ms: detail.origin.created_at,
                        origin: detail.origin.clone(),
                        parent_session: None,
                        is_active: active_path.as_deref() == Some(path_str.as_str()),
                    },
                );
            }
        }

        let mut projects = self.chat_projects();
        if projects.is_empty() && !self.prefs.cwd.as_os_str().is_empty() {
            projects.push(self.prefs.cwd.clone());
        }

        self.remote.update_snapshot(RemoteSnapshot {
            revision: self.remote_snapshot_tick,
            host_name: self.prefs.remote.host_name.clone(),
            tailscale_ip: self.remote.tailscale_ip.clone(),
            active_path,
            projects,
            sessions,
            details,
            allowed_artifacts,
            usage: self.usage.cached().unwrap_or_default(),
        });
    }

    pub(super) fn settings_remote(&mut self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        let before = self.prefs.remote.clone();
        self.prefs.remote.ensure_populated();

        settings::page_card(ui, |ui| {
            settings::page_row(
                ui,
                "Enable remote access",
                "Allow Pui Mobile on Android (and iOS) to connect securely to this computer over Tailscale.",
                |ui| {
                    settings::toggle_switch(
                        ui,
                        &mut self.prefs.remote.enabled,
                        "Enable remote access",
                    );
                },
            );
            if !self.prefs.remote.enabled {
                settings::page_separator(ui);
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(18, 12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    "Remote server is off. Turn it on to generate a Tailscale pairing QR code.",
                                )
                                .size(12.0)
                                .color(colors.muted),
                            )
                            .wrap(),
                        );
                    });
            }
        });

        if self.prefs.remote != before && !self.demo {
            self.remote.apply_config(self.prefs.remote.clone());
        }

        // Status, pairing QR code, and advanced host settings are shown only when enabled.
        if !self.prefs.remote.enabled {
            return;
        }

        settings::page_section(ui, "Pair your phone", "");
        settings::page_card(ui, |ui| {
            let status_text = if self.demo {
                "Preview mode · Remote server paused".to_owned()
            } else if let Some(addr) = self.remote.bound_addr {
                let count = self.remote.connected_devices().len();
                if count == 0 {
                    format!("Listening · {addr}")
                } else {
                    format!("Listening · {addr} · {count} connected")
                }
            } else if let Some(err) = &self.remote.last_error {
                err.clone()
            } else {
                "Starting remote listener…".to_owned()
            };
            let status_color = if self.remote.is_running() {
                colors.green
            } else if self.remote.last_error.is_some() {
                colors.red
            } else {
                colors.muted
            };
            let uri = self.remote.pairing_uri();
            let ws = self.remote.ws_url();
            let matrix = uri.as_deref().and_then(qr::encode);
            let copied_id = ui.id().with("remote-copied-label");
            let copied_notice: Option<String> = ui.ctx().data(|d| d.get_temp::<String>(copied_id));

            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(20, 18))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let wide = ui.available_width() >= 480.0;

                    let render_details = |ui: &mut egui::Ui, app: &mut Pui| {
                        ui.spacing_mut().item_spacing.y = 0.0;

                        // Top row: crisp 24px status pill on the left, quiet Refresh IP on the right.
                        ui.horizontal(|ui| {
                            status_pill(ui, &status_text, status_color);
                            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                if ghost_button(ui, "Refresh IP", Some(Icon::Refresh)).clicked()
                                    && !app.demo
                                {
                                    app.remote.refresh_tailscale_ip();
                                    if app.prefs.remote.enabled && !app.remote.is_running() {
                                        app.remote.start_listener();
                                    }
                                }
                            });
                        });

                        ui.add_space(10.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new("Scan with Pui Mobile")
                                    .size(14.0)
                                    .strong()
                                    .color(colors.text),
                            )
                            .wrap(),
                        );
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    "Open Pui on your phone → Pair with Pui Desktop → point camera at this code.",
                                )
                                .size(12.0)
                                .color(colors.muted),
                            )
                            .wrap(),
                        );

                        if let Some(ws) = &ws {
                            ui.add_space(10.0);
                            egui::Frame::new()
                                .fill(colors.bg)
                                .stroke(Stroke::new(1.0_f32, colors.border))
                                .corner_radius(7)
                                .inner_margin(egui::Margin::symmetric(10, 7))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(ws)
                                                .font(FontId::monospace(11.5))
                                                .color(colors.muted),
                                        )
                                        .wrap()
                                        .selectable(true),
                                    );
                                });
                        }

                        if let Some(uri) = &uri {
                            ui.add_space(10.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                                if accent_button(ui, "Copy pairing link").clicked() {
                                    ui.ctx().copy_text(uri.clone());
                                    ui.ctx().data_mut(|d| {
                                        d.insert_temp(copied_id, "Link copied".to_owned())
                                    });
                                }
                                if secondary_button(ui, "Copy token").clicked() {
                                    ui.ctx().copy_text(app.prefs.remote.token.clone());
                                    ui.ctx().data_mut(|d| {
                                        d.insert_temp(copied_id, "Token copied".to_owned())
                                    });
                                }
                                if ghost_button(ui, "Regenerate", None).clicked() {
                                    app.prefs.remote.regenerate_token();
                                    if !app.demo {
                                        app.remote.apply_config(app.prefs.remote.clone());
                                    }
                                    ui.ctx().data_mut(|d| {
                                        d.insert_temp(copied_id, "New token".to_owned())
                                    });
                                }
                                if let Some(notice) = &copied_notice {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(notice)
                                                .size(11.0)
                                                .color(colors.accent),
                                        )
                                        .wrap(),
                                    );
                                }
                            });
                        }
                    };

                    if wide && let Some(mat) = &matrix {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 20.0;
                            paint_qr(ui, mat, 148.0);
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width());
                                render_details(ui, self);
                            });
                        });
                    } else {
                        if let Some(mat) = &matrix {
                            paint_qr(ui, mat, 156.0_f32.min(ui.available_width()));
                            ui.add_space(12.0);
                        }
                        render_details(ui, self);
                    }
                });

            let devices = self.remote.connected_devices();
            settings::page_separator(ui);
            egui::Frame::new()
                .fill(colors.bg.gamma_multiply(0.45))
                .corner_radius(egui::CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 9,
                    se: 9,
                })
                .inner_margin(egui::Margin::symmetric(20, 11))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Connected devices")
                                .size(11.5)
                                .color(colors.muted),
                        );
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            if devices.is_empty() {
                                ui.label(
                                    RichText::new("None yet · waiting for Pui Mobile")
                                        .size(11.5)
                                        .color(colors.dim),
                                );
                            } else {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                for dev in devices.iter().rev() {
                                    device_pill(ui, dev);
                                }
                            }
                        });
                    });
                });
        });

        let adv_id = ui.id().with("remote-advanced-open");
        let mut adv_open = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(adv_id))
            .unwrap_or(false);
        let before_adv = self.prefs.remote.clone();
        settings::page_section(ui, "Advanced", "");
        settings::page_card(ui, |ui| {
            let summary = format!(
                "{} · Port {} · {}",
                self.prefs.remote.bind_mode.label(),
                self.prefs.remote.port,
                self.prefs.remote.host_name
            );
            settings::page_row(ui, "Network & host settings", &summary, |ui| {
                let label = if adv_open {
                    "▾ Hide"
                } else {
                    "▸ Configure"
                };
                if ghost_button(ui, label, None).clicked() {
                    adv_open = !adv_open;
                    ui.ctx().data_mut(|d| d.insert_temp(adv_id, adv_open));
                }
            });

            if adv_open {
                settings::page_separator(ui);
                settings::page_row(
                    ui,
                    "Network binding",
                    self.prefs.remote.bind_mode.description(),
                    |ui| {
                        egui::ComboBox::from_id_salt("remote-bind-mode")
                            .selected_text(self.prefs.remote.bind_mode.label())
                            .width(230.0_f32.min(ui.available_width()))
                            .show_ui(ui, |ui| {
                                for mode in BindMode::ALL {
                                    ui.selectable_value(
                                        &mut self.prefs.remote.bind_mode,
                                        mode,
                                        mode.label(),
                                    );
                                }
                            });
                    },
                );
                settings::page_separator(ui);
                settings::page_row(
                    ui,
                    "Port",
                    "TCP port used for the WebSocket and REST API (default 4817).",
                    |ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.prefs.remote.port)
                                .range(1024..=65535)
                                .speed(1),
                        );
                    },
                );
                settings::page_separator(ui);
                settings::page_row(
                    ui,
                    "Host name",
                    "Shown in the mobile app so you can tell multiple computers apart.",
                    |ui| {
                        ui.add_sized(
                            [210.0_f32.min(ui.available_width()), theme::CONTROL_HEIGHT],
                            theme::text_field(&mut self.prefs.remote.host_name, "My Mac"),
                        );
                    },
                );
                settings::page_separator(ui);
                settings::page_row(
                    ui,
                    "Custom public hostname (optional)",
                    "Override the QR host (for example a Tailscale MagicDNS name like mac.tail1234.ts.net).",
                    |ui| {
                        ui.add_sized(
                            [210.0_f32.min(ui.available_width()), theme::CONTROL_HEIGHT],
                            theme::text_field(
                                &mut self.prefs.remote.public_host,
                                "Auto (Tailscale IP)",
                            ),
                        );
                    },
                );
                settings::page_separator(ui);
                settings::page_row(
                    ui,
                    "Use TLS (wss://) in QR",
                    "Turn on when exposing Pui through `tailscale serve` or an HTTPS reverse proxy.",
                    |ui| {
                        settings::toggle_switch(
                            ui,
                            &mut self.prefs.remote.use_tls_in_qr,
                            "Use TLS in QR",
                        );
                    },
                );
            }
        });
        if self.prefs.remote != before_adv && !self.demo {
            self.remote.apply_config(self.prefs.remote.clone());
        }
    }
}

fn status_pill(ui: &mut egui::Ui, label: &str, color: Color32) {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), FontId::proportional(11.5), color);
    let height = 24.0;
    let width = (galley.size().x + 28.0).min(ui.available_width());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        12,
        color.gamma_multiply(0.12),
        Stroke::new(1.0_f32, color.gamma_multiply(0.32)),
        egui::StrokeKind::Inside,
    );
    painter.circle_filled(egui::pos2(rect.left() + 11.0, rect.center().y), 3.5, color);
    painter.galley(
        egui::pos2(rect.left() + 20.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

fn device_pill(ui: &mut egui::Ui, dev: &crate::remote::ConnectedDevice) {
    let colors = crate::theme::palette(ui.ctx());
    let label = format!("{} · {}", dev.device, dev.platform.label());
    let galley = ui
        .painter()
        .layout_no_wrap(label, FontId::proportional(11.5), colors.text);
    let height = 24.0;
    let width = (galley.size().x + 32.0).min(ui.available_width());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    ui.painter().rect(
        rect,
        12,
        colors.surface,
        Stroke::new(1.0_f32, colors.border),
        egui::StrokeKind::Inside,
    );
    let icon = if dev.platform == OriginPlatform::Web {
        Icon::Globe
    } else {
        Icon::Phone
    };
    icons::draw(
        ui,
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 13.0, rect.center().y),
            Vec2::splat(12.0),
        ),
        icon,
        colors.accent,
    );
    ui.painter().galley(
        egui::pos2(rect.left() + 23.0, rect.center().y - galley.size().y / 2.0),
        galley,
        colors.text,
    );
}

fn accent_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), FontId::proportional(12.0), colors.accent);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(galley.size().x + 24.0, 28.0), Sense::click());
    let fill = if response.hovered() {
        colors.accent.gamma_multiply(0.24)
    } else {
        colors.accent_bg
    };
    ui.painter().rect(
        rect,
        7,
        fill,
        Stroke::new(1.0_f32, colors.accent.gamma_multiply(0.45)),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        colors.accent,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn secondary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), FontId::proportional(12.0), colors.text);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(galley.size().x + 24.0, 28.0), Sense::click());
    let fill = if response.hovered() {
        colors.hover
    } else {
        colors.surface
    };
    ui.painter().rect(
        rect,
        7,
        fill,
        Stroke::new(1.0_f32, colors.border),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        colors.text,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn ghost_button(ui: &mut egui::Ui, label: &str, icon: Option<Icon>) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    let fg = colors.muted;
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), FontId::proportional(12.0), fg);
    let icon_pad = if icon.is_some() { 18.0 } else { 0.0 };
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(galley.size().x + icon_pad + 18.0, 28.0),
        Sense::click(),
    );
    let text_color = if response.hovered() { colors.text } else { fg };
    if response.hovered() {
        ui.painter().rect_filled(rect, 7, colors.hover);
    }
    let mut text_x = rect.left() + 9.0;
    if let Some(icon) = icon {
        icons::draw(
            ui,
            egui::Rect::from_center_size(
                egui::pos2(rect.left() + 14.0, rect.center().y),
                Vec2::splat(12.0),
            ),
            icon,
            text_color,
        );
        text_x += 18.0;
    }
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), FontId::proportional(12.0), text_color);
    ui.painter().galley(
        egui::pos2(text_x, rect.center().y - galley.size().y / 2.0),
        galley,
        text_color,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn paint_qr(ui: &mut egui::Ui, matrix: &qr::QrMatrix, target_size: f32) {
    // ISO/IEC 18004 mandates a 4-module quiet zone so phone cameras scan reliably on dark UIs.
    let quiet = 4usize;
    let total = matrix.size + quiet * 2;
    let cell = (target_size / total as f32).floor().max(2.0);
    let side = cell * total as f32;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 10, Color32::WHITE);
    for y in 0..matrix.size {
        for x in 0..matrix.size {
            if matrix.get(x, y) {
                let min =
                    rect.min + egui::vec2((x + quiet) as f32 * cell, (y + quiet) as f32 * cell);
                painter.rect_filled(
                    egui::Rect::from_min_size(min, Vec2::splat(cell)),
                    0,
                    Color32::BLACK,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_create_session_records_android_origin_on_session_path() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        let desktop_file = "/desktop/active-chat.jsonl".to_owned();
        app.state.session_file = Some(desktop_file.clone());
        app.draft = "Desktop draft stays intact".into();
        let desktop_agent = app.ensure_agent();

        let dir = std::env::temp_dir().join(format!("pui-origin-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let session_file = dir.join("chat.jsonl");
        std::fs::write(
            &session_file,
            format!("{{\"type\":\"session\",\"cwd\":\"{}\"}}\n", dir.display()),
        )
        .unwrap();

        let origin = SessionOrigin {
            platform: OriginPlatform::Android,
            device: Some("Pixel 9".into()),
            client: Some("pui-mobile".into()),
            created_at: Some(1_763_913_600_000),
        };
        app.handle_remote_command(RemoteCommand::CreateSession {
            cwd: dir.clone(),
            permission_mode: Some("manual".into()),
            prompt: None,
            origin: origin.clone(),
        });
        // Desktop's visible active session and draft MUST NOT have switched!
        assert_eq!(app.agents.active, desktop_agent);
        assert_eq!(
            app.state.session_file.as_deref(),
            Some(desktop_file.as_str())
        );
        assert_eq!(app.draft, "Desktop draft stays intact");

        // The newly created phone session lives parked in background with its pending origin.
        let (phone_id, pending_origin) = app
            .pending_origins
            .iter()
            .next()
            .map(|(k, v)| (k.clone(), v.clone()))
            .expect("pending origin for background phone session");
        assert_ne!(phone_id, desktop_agent);
        assert_eq!(pending_origin, origin);

        // Simulate Pi assigning the concrete session file path on the parked phone chat.
        app.agents
            .parked
            .get_mut(&phone_id)
            .unwrap()
            .state
            .session_file = Some(session_file.to_string_lossy().into_owned());
        app.flush_pending_origins();
        assert!(app.pending_origins.is_empty());
        assert_eq!(app.session_origin_for(&session_file), origin);

        // Opening another saved session from phone also keeps Desktop untouched.
        let saved_other = dir.join("other-saved.jsonl");
        std::fs::write(
            &saved_other,
            format!("{{\"type\":\"session\",\"cwd\":\"{}\"}}\n", dir.display()),
        )
        .unwrap();
        app.sessions.push(Session {
            path: saved_other.clone(),
            cwd: dir.clone(),
            project: dir.clone(),
            title: "Other saved chat".into(),
            created: None,
            modified: std::time::SystemTime::now(),
        });
        app.handle_remote_command(RemoteCommand::OpenSession {
            path: saved_other.clone(),
        });
        assert_eq!(app.agents.active, desktop_agent);
        assert_eq!(
            app.state.session_file.as_deref(),
            Some(desktop_file.as_str())
        );
        assert_eq!(app.draft, "Desktop draft stays intact");
        assert!(app.agent_for_path(&saved_other).is_some());

        // Durable custom entry was appended to the JSONL file as well.
        let disk = std::fs::read_to_string(&session_file).unwrap();
        assert!(disk.contains("pui.session-origin.v1"));
        assert!(disk.contains("\"platform\":\"android\""));
        assert!(disk.contains("Pixel 9"));

        app.shutdown_agents();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remote_settings_hides_status_and_qr_until_enabled() {
        fn collect_texts(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| collect_texts(s, out)),
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                demo_settings: true,
                ..Default::default()
            },
        );
        app.prefs.remote.enabled = false;
        let off = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(960.0, 900.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.settings_remote(ui));
            },
        );
        let mut off_labels = Vec::new();
        off.shapes
            .iter()
            .for_each(|s| collect_texts(&s.shape, &mut off_labels));
        assert!(off_labels.iter().any(|t| t == "Enable remote access"));
        assert!(
            !off_labels
                .iter()
                .any(|t| t.contains("Scan with Pui Mobile"))
        );
        assert!(!off_labels.iter().any(|t| t.contains("Copy pairing link")));
        assert!(
            !off_labels
                .iter()
                .any(|t| t.contains("Network & host settings"))
        );

        app.prefs.remote.enabled = true;
        app.remote.tailscale_ip = Some("100.108.253.122".into());
        let on = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(960.0, 900.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.settings_remote(ui));
            },
        );
        let mut on_labels = Vec::new();
        on.shapes
            .iter()
            .for_each(|s| collect_texts(&s.shape, &mut on_labels));
        assert!(on_labels.iter().any(|t| t == "Scan with Pui Mobile"));
        assert!(on_labels.iter().any(|t| t == "Copy pairing link"));
        assert!(on_labels.iter().any(|t| t == "Network & host settings"));
    }
}
