//! Explicit current-chat resource reload, compatible with already-running adapters.
use super::*;

mod connector;

#[derive(Default)]
pub(super) struct Reload {
    confirming: bool,
    retiring: Option<Receiver<()>>,
    starting: bool,
    history: bool,
    commands: bool,
    started: Option<std::time::Instant>,
    failed: bool,
    connector_confirm: Option<(String, String)>,
    connector_pending: Option<ConnectorRequest>,
    reconnected: std::collections::BTreeSet<String>,
    connector_error: Option<String>,
}
struct ConnectorRequest {
    id: String,
    name: String,
    started: std::time::Instant,
    confirmed: bool,
    timed_out: bool,
}
impl Reload {
    pub(super) fn connector_pending(&self) -> bool {
        self.connector_pending.is_some()
    }
    pub(super) fn connector_connecting(&self, name: &str) -> bool {
        self.connector_pending
            .as_ref()
            .is_some_and(|r| r.name == name)
    }
    pub(super) fn connector_reconnected(&self, name: &str) -> bool {
        self.reconnected.contains(name)
    }
    pub(super) fn confirming(&self) -> bool {
        self.confirming || self.connector_confirm.is_some()
    }
    pub(super) fn pending(&self) -> bool {
        self.retiring.is_some() || self.starting
    }
}

impl Pui {
    fn resource_reload_blocked(&self) -> Option<&'static str> {
        if self.demo || self.is_mirror() || self.agents.background {
            return Some("Open this chat in its owning Pui window to reload resources.");
        }
        if self.resource_reload.pending() || self.resource_reload.connector_pending() {
            return Some("Resources or a connector are already reconnecting for this chat.");
        }
        if !self.connected || self.client.is_none() || self.state.session_file.is_none() {
            return Some("Connect to a saved chat before reloading resources.");
        }
        if !self.state.ready()
            || self.loading_chat
            || self.history_pending.is_some()
            || self.restoring_code()
            || !self.state.dialogs.is_empty()
            || !self.deliveries.is_empty()
            || self.interrupt.is_some()
            || !self.state.pending_messages.is_empty()
            || self.agents_still_retiring()
            || self.agents.exiting
            || self.deletion.confirming()
            || self.deletion.in_progress()
            || self
                .agents
                .permission_changes
                .contains_key(&self.agents.active)
            || self.agent_mail_pending(&self.agents.active)
            || self.state.session_file.as_ref().is_some_and(|p| {
                self.waiting
                    .get(&PathBuf::from(p))
                    .is_some_and(|q| !q.is_empty())
            })
        {
            return Some(
                "Wait for this chat's run, approvals and queued messages to finish before reloading.",
            );
        }
        if self.agents.nodes.get(&self.agents.active).is_some_and(|n| {
            n.setup_pending() || n.launch.as_ref().is_some_and(|l| l.task_role.is_some())
        }) {
            return Some("Managed workflow runtimes cannot be reloaded here.");
        }
        if self
            .state
            .session_file
            .as_ref()
            .is_some_and(|p| self.sync.enabled() && !self.sync.owns(std::path::Path::new(p)))
        {
            return Some("This window no longer owns the chat. Reopen it in its owning window.");
        }
        None
    }

    pub(super) fn request_resource_reload(&mut self) {
        if self.side_chat.rendering {
            self.state.notice = Some(
                "Expand this side chat before reloading its resources. Your draft was kept.".into(),
            );
            return;
        }
        if let Some(reason) = self.resource_reload_blocked() {
            self.state.error = Some(reason.into());
            return;
        }
        if self.resource_reload.confirming {
            return;
        }
        self.ensure_agent();
        self.resource_reload.confirming = true;
        self.close_model_popovers();
        self.completions.dismiss();
    }

    pub(super) fn resource_reload_dialog(&mut self, ctx: &egui::Context) {
        self.connector_reconnect_dialog(ctx);
        if !self.resource_reload.confirming {
            return;
        }
        let blocked = self.resource_reload_blocked();
        let choice = confirmations::Dialog::new(
            "reload-chat-resources",
            "Reload resources for this chat?",
            "Reconnects this chat to the same saved conversation to load newly enabled skills and refresh slash commands.",
            "Cancel",
            "Reload resources",
        )
        .destructive(false)
        .enabled(blocked.is_none())
        .width(440.0)
        .show(ctx, |ui| {
            confirmations::note(ui, "History, draft and attachments are kept. Nothing is sent or rerun. Other chats are unchanged.");
            confirmations::note(ui, "All configured Pi resources reload, including extensions, MCP servers and context files. Trusted extensions and package dependencies may execute code; servers may reconnect. Session-only extension state and temporary approval grants reset. Existing project trust and this chat's access mode remain enforced.");
            if let Some(reason) = blocked { confirmations::note(ui, reason); }
        });
        match choice {
            Some(confirmations::Choice::Cancel) => self.resource_reload.confirming = false,
            Some(confirmations::Choice::Confirm) => {
                self.resource_reload.confirming = false;
                self.begin_resource_reload();
            }
            None => {}
        }
    }

    fn begin_resource_reload(&mut self) {
        if let Some(reason) = self.resource_reload_blocked() {
            self.state.error = Some(reason.into());
            return;
        }
        let client = self.client.take().unwrap();
        let (tx, rx) = mpsc::channel();
        self.resource_reload = Reload {
            retiring: Some(rx),
            ..Default::default()
        };
        self.loading_chat = true;
        self.connected = false;
        self.ultracode_ready = false;
        if self.draft.trim() == "/reload" {
            self.draft.clear();
            self.recall = Default::default();
        }
        self.state.commands.clear();
        self.completions = Default::default();
        self.state.error = None;
        self.state.notice = Some(
            "Reloading resources for this chat… Your draft is kept; nothing will be sent.".into(),
        );
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            drop(client); // Reap the old writer before opening this session again.
            let _ = tx.send(());
            ctx.request_repaint();
        });
    }

    pub(super) fn poll_resource_reload(&mut self) {
        self.poll_connector_reconnect();
        if self
            .resource_reload
            .retiring
            .as_ref()
            .is_some_and(|rx| !matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)))
        {
            self.resource_reload.retiring = None;
            if self.resource_reload.failed {
                self.resource_reload = Default::default();
                self.loading_chat = false;
                self.state.notice = None;
                self.state.error = Some("Resource reload timed out. Your history and draft are kept; nothing was sent. Reopen this chat to reconnect.".into());
                return;
            }
            if self.agents.exiting {
                self.loading_chat = false;
                return;
            }
            let Some(path) = self.state.session_file.as_ref().map(PathBuf::from) else {
                return;
            };
            if self.sync.enabled() && !self.sync.owns(&path) {
                self.loading_chat = false;
                self.state.error = Some(
                    "Chat ownership changed. Resources were not reloaded; no prompt was sent."
                        .into(),
                );
                return;
            }
            let mut launch = self
                .agents
                .nodes
                .get(&self.agents.active)
                .and_then(|n| n.launch.clone())
                .unwrap_or_default();
            launch.readonly |= self.readonly_worker();
            launch.settings = self.agent_settings(&self.agents.active).payload();
            #[cfg(not(test))]
            {
                self.client = Some(rpc::Client::start_with(
                    &self.prefs.cwd,
                    Some(&path),
                    self.ctx.clone(),
                    launch,
                ));
            }
            #[cfg(test)]
            {
                let _ = (path, launch);
                self.client = Some(rpc::Client::stub());
            }
            self.ultracode = false;
            self.resource_reload.starting = true;
            self.resource_reload.started = Some(std::time::Instant::now());
            for command in [
                "get_state",
                "get_available_models",
                "get_available_thinking_levels",
                "get_entries",
                "get_commands",
                "get_session_stats",
            ] {
                self.send(protocol::command(command));
            }
        }
        let timed_out = self
            .resource_reload
            .started
            .is_some_and(|at| at.elapsed() >= Duration::from_secs(60));
        if timed_out && let Some(client) = self.client.take() {
            let (tx, rx) = mpsc::channel();
            let ctx = self.ctx.clone();
            self.resource_reload = Reload {
                failed: true,
                retiring: Some(rx),
                ..Default::default()
            };
            self.connected = false;
            std::thread::spawn(move || {
                drop(client);
                let _ = tx.send(());
                ctx.request_repaint();
            });
            return;
        }
        if self.resource_reload.starting && self.client.is_none() {
            self.loading_chat = false;
            self.resource_reload = Default::default();
            self.state.notice = None;
            self.state.error = Some("Resource reload did not reconnect. Your history and draft are kept; nothing was sent. Reopen this chat to reconnect.".into());
        } else if self.resource_reload.starting
            && self.resource_reload.history
            && self.resource_reload.commands
            && self.connected
            && self.ultracode_ready
        {
            self.resource_reload = Default::default();
            self.loading_chat = false;
            self.state.notice = Some("Resources reloaded for this chat. Newly enabled skills are available under /skill:… Nothing was sent.".into());
        }
        if let Some(at) = self.resource_reload.started {
            self.ctx
                .request_repaint_after(Duration::from_secs(60).saturating_sub(at.elapsed()));
        }
    }

    pub(super) fn resource_reload_history(&mut self) {
        if self.resource_reload.starting {
            self.resource_reload.history = true;
        }
    }
    pub(super) fn resource_reload_record(&mut self, record: &Value) {
        if self.resource_reload.starting
            && record["type"] == "response"
            && record["command"] == "get_commands"
        {
            self.resource_reload.commands = record["success"] == true;
        }
    }
}

#[cfg(test)]
mod tests;
