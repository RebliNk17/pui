//! Bridges Pui Desktop's live app state with the opt-in `crate::remote::RemoteServer`,
//! handles incoming mobile commands, stamps session origins, and renders Settings → Remote & Mobile.
use super::*;
use crate::remote::{
    BindMode, OriginPlatform, RemoteCommand, RemoteSnapshot, SessionDetail, SessionOrigin,
    SessionSummary, extract_artifacts, qr,
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
                self.pending_origins
                    .contains_key(id)
                    .then(|| node.path.clone().map(|p| (id.clone(), p)))
                    .flatten()
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

    fn handle_remote_command(&mut self, cmd: RemoteCommand) {
        match cmd {
            RemoteCommand::CreateSession {
                cwd,
                permission_mode,
                prompt,
                origin,
            } => {
                let perm = permission_mode
                    .as_deref()
                    .and_then(settings::PermissionMode::from_key);
                if cwd.is_dir() {
                    self.start_chat_with_permission(cwd, perm);
                } else {
                    self.start_chat_with_permission(self.prefs.cwd.clone(), perm);
                }
                let active_id = self.agents.active.clone();
                if let Some(path_str) = self.state.session_file.clone() {
                    self.record_session_origin(std::path::Path::new(&path_str), origin);
                } else {
                    self.pending_origins.insert(active_id, origin);
                }
                if let Some(text) = prompt {
                    self.draft = text;
                    let ctx = self.ctx.clone();
                    self.submit_mode(&ctx, self.prefs.settings.delivery);
                }
            }
            RemoteCommand::OpenSession { path } => {
                self.focus_session_path(&path);
            }
            RemoteCommand::SendMessage {
                path,
                text,
                mode,
                images,
                origin,
            } => {
                if let Some(target_path) = path {
                    self.focus_session_path(&target_path);
                }
                if let Some(origin) = origin {
                    if let Some(path_str) = self.state.session_file.clone() {
                        let p = std::path::Path::new(&path_str);
                        if !self.prefs.chats.get(p).is_some_and(|m| m.origin.is_some())
                            && self.state.messages.is_empty()
                        {
                            self.record_session_origin(p, origin);
                        }
                    }
                }
                let delivery = match mode.as_deref() {
                    Some("queue") => settings::DeliveryMode::Queue,
                    Some("steer") => settings::DeliveryMode::Steer,
                    _ => self.prefs.settings.delivery,
                };
                if !images.is_empty() {
                    let mut decoded = Vec::new();
                    for img in images {
                        if let Ok(bytes) =
                            base64::engine::general_purpose::STANDARD.decode(img.data_base64.trim())
                        {
                            if let Ok(att) = crate::media::Attachment::from_bytes(&bytes) {
                                decoded.push(att);
                            }
                        }
                    }
                    if !decoded.is_empty() {
                        self.draft_images.set_images(decoded);
                    }
                }
                self.draft = text;
                let ctx = self.ctx.clone();
                self.submit_mode(&ctx, delivery);
            }
            RemoteCommand::Abort { path } => {
                if let Some(target_path) = path {
                    self.focus_session_path(&target_path);
                }
                self.stop();
            }
            RemoteCommand::AnswerDialog {
                path,
                dialog_id,
                response,
            } => {
                if let Some(target_path) = path {
                    self.focus_session_path(&target_path);
                }
                self.answer_dialog(&dialog_id, response);
            }
            RemoteCommand::UpdateSettings {
                path,
                permission_mode,
                model,
                provider,
                thinking,
            } => {
                if let Some(target_path) = path {
                    self.focus_session_path(&target_path);
                }
                if let Some(key) = permission_mode.as_deref() {
                    if let Some(mode) = settings::PermissionMode::from_key(key) {
                        self.prefs.settings.permission = mode;
                        let payload = self.prefs.settings.payload();
                        self.send(json!({ "type": "set_settings", "settings": payload }));
                    }
                }
                if let Some(model_id) = model {
                    let prov = provider.unwrap_or_else(|| self.state.provider.clone());
                    if !prov.is_empty() {
                        self.send(json!({
                            "type": "set_model",
                            "provider": prov,
                            "modelId": model_id,
                        }));
                    }
                }
                if let Some(level) = thinking {
                    self.send(json!({
                        "type": "set_thinking_level",
                        "level": level,
                    }));
                }
            }
        }
    }

    fn focus_session_path(&mut self, path: &std::path::Path) {
        if self
            .state
            .session_file
            .as_deref()
            .is_some_and(|p| sidebar_tree::same_path(std::path::Path::new(p), path))
        {
            return;
        }
        if let Some(session) = self
            .sessions
            .iter()
            .find(|s| sidebar_tree::same_path(&s.path, path))
            .cloned()
        {
            self.open_chat(session);
        } else if let Some(id) = self.agent_for_path(path) {
            self.view_agent(&id);
        }
    }

    fn publish_remote_snapshot(&mut self) {
        self.remote_snapshot_tick = self.remote_snapshot_tick.wrapping_add(1);
        let active_path = self.state.session_file.clone();
        let active_detail_rev = self
            .state
            .transcript_revision
            .wrapping_mul(31)
            .wrapping_add(self.state.outputs_revision)
            .wrapping_add(u64::from(self.state.busy) << 56)
            .wrapping_add((self.state.dialogs.len() as u64) << 48)
            .wrapping_add((self.state.pending_messages.len() as u64) << 40);

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
                    .unwrap_or_else(|| self.prefs.settings.permission.key().to_owned()),
                cost: self.state.cost,
                context_tokens: self.state.context_tokens,
                context_window: self.state.context_window,
                error: self.state.error.clone(),
                notice: self.state.notice.clone(),
                origin,
                messages: self.state.messages.clone(),
                pending_messages: self.state.pending_messages.clone(),
                dialogs: self.state.dialogs.iter().cloned().collect(),
                commands: self.state.commands.clone(),
                models: self.state.models.clone(),
                artifacts,
                revision: active_detail_rev,
            };
            details.insert(path_str.clone(), detail);
        }

        // Also expose parked live agents so mobile can view running background chats seamlessly.
        for (id, chat) in &self.agents.parked {
            let Some(path_str) = chat.state.session_file.clone() else {
                continue;
            };
            if details.contains_key(&path_str) {
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
            let detail_rev = chat
                .state
                .transcript_revision
                .wrapping_mul(31)
                .wrapping_add(chat.state.outputs_revision)
                .wrapping_add(u64::from(chat.state.busy) << 56)
                .wrapping_add((chat.state.dialogs.len() as u64) << 48);
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
                        .unwrap_or_else(|| self.prefs.settings.permission.key().to_owned()),
                    cost: chat.state.cost,
                    context_tokens: chat.state.context_tokens,
                    context_window: chat.state.context_window,
                    error: chat.state.error.clone(),
                    notice: chat.state.notice.clone(),
                    origin,
                    messages: chat.state.messages.clone(),
                    pending_messages: chat.state.pending_messages.clone(),
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
        });
    }

    pub(super) fn settings_remote(&mut self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        let before = self.prefs.remote.clone();
        self.prefs.remote.ensure_populated();

        settings::page_section(ui, "Mobile companion", "");
        settings::page_card(ui, |ui| {
            settings::page_row(
                ui,
                "Enable remote access",
                "Allow the Pui mobile app (Android now, iOS next) to connect to this computer over Tailscale.",
                |ui| {
                    settings::toggle_switch(
                        ui,
                        &mut self.prefs.remote.enabled,
                        "Enable remote access",
                    );
                },
            );
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
        });

        if self.prefs.remote != before && !self.demo {
            self.remote.apply_config(self.prefs.remote.clone());
        }

        settings::page_section(ui, "Status & pairing", "");
        settings::page_card(ui, |ui| {
            let status_text = if self.demo {
                "Preview mode — remote server is paused in demo.".to_owned()
            } else if let Some(addr) = self.remote.bound_addr {
                let count = self.remote.connected_devices().len();
                if count == 0 {
                    format!("Listening on {addr} · Waiting for phone")
                } else {
                    format!("Listening on {addr} · {count} connected")
                }
            } else if let Some(err) = &self.remote.last_error {
                err.clone()
            } else {
                "Off — enable remote access above to pair your phone.".to_owned()
            };
            let status_color = if self.remote.is_running() {
                colors.green
            } else if self.remote.last_error.is_some() {
                colors.red
            } else {
                colors.muted
            };
            ui.horizontal_wrapped(|ui| {
                icons::inline(ui, Icon::Phone, status_color);
                ui.add(
                    egui::Label::new(RichText::new(status_text).size(12.5).color(status_color))
                        .wrap(),
                );
            });
            ui.add_space(6.0);
            let ts_label = self
                .remote
                .tailscale_ip
                .as_deref()
                .map(|ip| format!("Detected Tailscale IP: {ip}"))
                .unwrap_or_else(|| "Tailscale IP: not detected".to_owned());
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::Label::new(RichText::new(ts_label).size(11.5).color(colors.muted)).wrap(),
                );
                if ui.button("Refresh IP").clicked() && !self.demo {
                    self.remote.refresh_tailscale_ip();
                    if self.prefs.remote.enabled && !self.remote.is_running() {
                        self.remote.start_listener();
                    }
                }
            });

            if let Some(uri) = self.remote.pairing_uri() {
                settings::page_separator(ui);
                ui.add(
                    egui::Label::new(
                        RichText::new("Scan with Pui Mobile to pair over Tailscale")
                            .size(12.5)
                            .strong()
                            .color(colors.text),
                    )
                    .wrap(),
                );
                ui.add_space(8.0);
                if let Some(matrix) = qr::encode(&uri) {
                    paint_qr(ui, &matrix, 168.0_f32.min(ui.available_width()));
                    ui.add_space(8.0);
                }
                if let Some(ws) = self.remote.ws_url() {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("Endpoint: {ws}"))
                                .font(FontId::monospace(11.0))
                                .color(colors.muted),
                        )
                        .wrap()
                        .selectable(true),
                    );
                    ui.add_space(6.0);
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Copy pairing link").clicked() {
                        ui.ctx().copy_text(uri.clone());
                    }
                    if ui.button("Copy token").clicked() {
                        ui.ctx().copy_text(self.prefs.remote.token.clone());
                    }
                    if ui.button("Regenerate token").clicked() {
                        self.prefs.remote.regenerate_token();
                        if !self.demo {
                            self.remote.apply_config(self.prefs.remote.clone());
                        }
                    }
                });
            }

            let devices = self.remote.connected_devices();
            if !devices.is_empty() {
                settings::page_separator(ui);
                ui.add(
                    egui::Label::new(
                        RichText::new("Connected devices")
                            .size(12.0)
                            .strong()
                            .color(colors.text),
                    )
                    .wrap(),
                );
                ui.add_space(4.0);
                for dev in devices {
                    ui.horizontal_wrapped(|ui| {
                        let icon = if dev.platform == OriginPlatform::Web {
                            Icon::Globe
                        } else {
                            Icon::Phone
                        };
                        icons::inline(ui, icon, colors.accent);
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "{} · {} ({})",
                                    dev.device,
                                    dev.platform.label(),
                                    dev.remote_addr
                                ))
                                .size(11.5)
                                .color(colors.muted),
                            )
                            .wrap(),
                        );
                    });
                }
            }
        });
    }
}

fn paint_qr(ui: &mut egui::Ui, matrix: &qr::QrMatrix, target_size: f32) {
    let quiet = 2usize;
    let total = matrix.size + quiet * 2;
    let cell = (target_size / total as f32).floor().max(2.0);
    let side = cell * total as f32;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 6, Color32::WHITE);
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
        assert_eq!(app.pending_origins.get(&app.agents.active), Some(&origin));

        // Simulate Pi assigning the concrete session file path.
        app.state.session_file = Some(session_file.to_string_lossy().into_owned());
        app.flush_pending_origins();
        assert!(app.pending_origins.is_empty());
        assert_eq!(app.session_origin_for(&session_file), origin);

        // Durable custom entry was appended to the JSONL file as well.
        let disk = std::fs::read_to_string(&session_file).unwrap();
        assert!(disk.contains("pui.session-origin.v1"));
        assert!(disk.contains("\"platform\":\"android\""));
        assert!(disk.contains("Pixel 9"));

        app.shutdown_agents();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
