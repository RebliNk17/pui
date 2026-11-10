//! Explicit, idle-only navigation. Never auto-send the restored prompt.
use super::*;
use crate::transcript::MessageAction;
#[cfg(test)]
#[path = "rewind_tests.rs"]
mod tests;
#[derive(Clone)]
struct Request {
    path: String,
    point: protocol::HistoryPoint,
    message: Message,
    operation: MessageAction,
    mode: Mode,
}
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Messages,
    Both,
    Code,
}
impl Mode {
    fn key(self) -> &'static str {
        match self {
            Self::Messages => "messages",
            Self::Both => "both",
            Self::Code => "code",
        }
    }
}
pub(super) struct Pending {
    id: String,
    rpc_id: String,
    started: std::time::Instant,
    timed_out: bool,
    request: Request,
}
#[derive(Default)]
pub(super) struct View {
    request: Option<Request>,
    error: Option<String>,
}
impl View {
    pub(super) fn is_open(&self) -> bool {
        self.request.is_some()
    }
}
fn point_for<'a>(
    message: &Message,
    points: &'a [protocol::HistoryPoint],
) -> Option<&'a protocol::HistoryPoint> {
    // Exact timestamp + raw text. Never use text-only/index
    // guesses: duplicate prompts and partial previews must not rewind another turn.
    let mut matches = points.iter().filter(|p| {
        p.timestamp == message.timestamp && p.text == message.text && p.role == message.role
    });
    let point = matches.next()?;
    if matches.next().is_some() {
        None
    } else {
        Some(point)
    }
}
impl Pui {
    pub(super) fn history_ready(&self) -> bool {
        !self.demo
            && !self.is_mirror()
            && self.connected
            && self.client.is_some()
            && self.ultracode_ready
            && self.state.ready()
            && !self.loading_chat
            && self.state.dialogs.is_empty()
            && self.deliveries.is_empty()
            && self.interrupt.is_none()
            && self.state.pending_messages.is_empty()
            && self.history_pending.is_none()
            && !self.restoring_code()
            && self.history_drafts.len() < 8
            && self.state.session_file.is_some()
            && self.state.session_file.as_ref().is_none_or(|p| {
                self.waiting
                    .get(&PathBuf::from(p))
                    .is_none_or(Vec::is_empty)
            })
    }
    pub(super) fn history_action(&mut self) {
        let Some((index, operation)) = self.transcript.message_action.take() else {
            return;
        };
        if !self.history_ready() {
            return;
        }
        let Some(message) = self.state.messages.get(index) else {
            return;
        };
        if operation == MessageAction::Fork
            && self
                .agents
                .nodes
                .get(&self.agents.active)
                .is_some_and(|n| n.access == "read-only")
        {
            self.state.error = Some("Fork from the writable parent chat; read-only worker restrictions cannot be removed by forking.".into());
            return;
        }
        if (operation == MessageAction::Fork) != (message.role == "assistant") {
            return;
        }
        let Some(point) = point_for(message, &self.state.history_points) else {
            self.state.error = Some("No unambiguous Pi entry for this message yet. Wait for the chat to finish loading or reconnect with the current adapter.".into());
            return;
        };
        let request = Request {
            path: self.state.session_file.clone().unwrap(),
            point: point.clone(),
            message: message.clone(),
            operation,
            mode: Mode::Messages,
        };
        // Returning a prompt to the composer is reversible and never sends it.
        // Ask only when there is a real file-restore choice (or a new fork).
        if operation != MessageAction::Fork
            && (request.point.file_changes == Some(0)
                || request.point.code_reason.is_some()
                || !self.workspace_restore_idle())
        {
            self.begin_history(request);
        } else {
            self.history_view = View {
                request: Some(request),
                error: None,
            };
        }
    }
    pub(super) fn history_confirmation(&mut self, ctx: &egui::Context) {
        let Some(mut request) = self.history_view.request.clone() else {
            return;
        };
        if self.state.session_file.as_deref() != Some(&request.path) {
            self.history_view = Default::default();
            return;
        }
        let fork = request.operation == MessageAction::Fork;
        let choice = confirmations::Dialog::new(
            "history-restore-confirmation",
            match request.operation { MessageAction::Edit => "Edit message", MessageAction::Fork => "Fork after this response?", MessageAction::Rewind => "Rewind conversation" },
            if fork { "Start a separate conversation including this response. Nothing will be rerun." } else { "Bring this prompt back to the composer. Choose what happens to files; nothing is sent until you send it." },
            "Cancel",
            match (request.operation, request.mode) { (MessageAction::Fork, _) => "Create fork", (_, Mode::Code) => "Restore files", (MessageAction::Edit, Mode::Messages) => "Edit message", (MessageAction::Edit, Mode::Both) => "Restore files and edit", (_, Mode::Messages) => "Rewind messages", (_, Mode::Both) => "Restore files and rewind" },
        ).width(440.0).destructive(!fork && request.mode != Mode::Messages).enabled(self.history_ready() && (request.mode == Mode::Messages || self.workspace_restore_idle())).show(ctx, |ui| {
                egui::Frame::new().fill(theme::palette(ctx).sidebar).corner_radius(8).inner_margin(12).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(48.0).show(ui, |ui| {
                        ui.add(egui::Label::new(crate::transcript::user_display_text(&request.message)).wrap().selectable(true));
                        if request.operation != MessageAction::Fork && !request.message.images.is_empty() {ui.label(theme::small(ui,format!("{} attached file(s) will return to the draft",request.message.images.len())));}
                    });
                });
                ui.add_space(8.0);
                if request.operation == MessageAction::Fork {
                    ui.add(egui::Label::new("Creates a separate chat. The original chat and all files remain unchanged; both chats still use the same workspace.").wrap());
                } else {
                    ui.add(egui::Label::new(theme::small(ui, match request.point.file_changes {
                        Some(count) => format!("{count} captured file change(s) since this message"),
                        None => "File changes may exist since this message".into(),
                    })).wrap());
                    for (mode,title,description) in [(Mode::Messages,"Keep current files","Edit the conversation only. Your files stay exactly as they are."),(Mode::Both,"Restore files and conversation","Undo captured file changes and return this prompt to the composer."),(Mode::Code,"Restore files only","Undo captured file changes. Keep the conversation and draft unchanged.")] {
                        let available = mode == Mode::Messages || (request.point.code_reason.is_none() && self.workspace_restore_idle());
                        ui.add_enabled_ui(available, |ui| {
                            if prompt_ui::option(ui, title, description, request.mode == mode).clicked() { request.mode = mode; }
                        });
                    }
                    if let Some(reason)=&request.point.code_reason {ui.add(egui::Label::new(theme::small(ui,reason)).wrap());}
                    if !self.workspace_restore_idle() {ui.add(egui::Label::new(theme::small(ui,"File restore requires other agents in this window to finish or stop. Other windows and external writers cannot be locked by Pui; conflicts are checked before restoring.")).wrap());}
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(theme::small(ui,"Later messages remain in history. File restore cannot undo commits, deployments or other external actions.")).wrap());
                }
                if self.history_drafts.len() >= 8 {ui.add(egui::Label::new(theme::small(ui,"Restore or send saved drafts before another rewind; eight are already retained.")).wrap());}
                if request.operation != MessageAction::Fork && (!self.draft.is_empty() || !self.draft_images.is_empty()) {ui.add(egui::Label::new(theme::small(ui,"Your current unsent draft will be saved for manual recovery before replacing it.")).wrap());}
                if let Some(error)=&self.history_view.error {ui.colored_label(theme::palette(ctx).red,error);}
            });
        self.history_view.request = Some(request.clone());
        match choice {
            Some(confirmations::Choice::Cancel) => self.history_view = Default::default(),
            Some(confirmations::Choice::Confirm) => self.begin_history(request),
            None => {}
        }
    }
    pub(super) fn restoring_code(&self) -> bool {
        self.history_pending
            .as_ref()
            .is_some_and(|p| p.request.mode != Mode::Messages)
            || self.agents.parked.values().any(|chat| {
                chat.history_pending
                    .as_ref()
                    .is_some_and(|p| p.request.mode != Mode::Messages)
            })
    }
    fn workspace_restore_idle(&self) -> bool {
        self.agents.parked.values().all(|chat| {
            chat.client.is_none() || (chat.state.ready() && chat.history_pending.is_none())
        }) && self
            .agents
            .nodes
            .values()
            .all(|node| node.stopped || node.path.is_some() || node.id == self.agents.active)
    }
    fn begin_history(&mut self, request: Request) {
        if !self.history_ready() || self.state.session_file.as_deref() != Some(&request.path) {
            return;
        }
        // Resolve the entry again: a dialog must never apply stale restore metadata.
        let Some(point) = point_for(&request.message, &self.state.history_points) else {
            return;
        };
        if point.entry_id != request.point.entry_id {
            return;
        }
        if request.mode != Mode::Messages
            && (point.code_reason.is_some() || !self.workspace_restore_idle())
        {
            return;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let payload = json!({"id":id,"entryId":request.point.entry_id,"operation":match request.operation {MessageAction::Edit=>"edit",MessageAction::Fork=>"fork",MessageAction::Rewind=>"rewind"},"mode":request.mode.key()});
        let record = json!({"type":"prompt","message":format!("/pui-history {payload}")});
        if let Some(client) = self.client.as_mut() {
            match client.enqueue(record) {
                Ok(rpc_id) => {
                    self.history_pending = Some(Pending {
                        id,
                        rpc_id,
                        started: std::time::Instant::now(),
                        timed_out: false,
                        request,
                    });
                    self.history_view = Default::default();
                }
                Err(error) => {
                    self.state.error = Some(error.clone());
                    self.history_view.error = Some(error);
                }
            }
        }
    }
    pub(super) fn history_event(&mut self, record: &Value) -> bool {
        if record["type"] == "response"
            && self
                .history_pending
                .as_ref()
                .is_some_and(|p| record["id"].as_str() == Some(p.rpc_id.as_str()))
        {
            if record["success"] == false {
                self.history_pending = None;
                self.state.error = Some("Pi rejected history navigation. No prompt was resent. Reconnect with the updated Pui adapter.".into());
            }
            return true;
        }
        if record["type"] != "extension_ui_request"
            || record["method"] != "setStatus"
            || record["statusKey"] != "pui-history-result"
        {
            return false;
        }
        let Ok(value) = serde_json::from_str::<Value>(protocol::string(record, "statusText"))
        else {
            return true;
        };
        if self
            .history_pending
            .as_ref()
            .is_none_or(|p| Some(p.id.as_str()) != value["id"].as_str())
        {
            return true;
        }
        let pending = self.history_pending.take().unwrap();
        if value["success"] != true {
            self.state.error = Some(
                value["error"]
                    .as_str()
                    .unwrap_or("History operation failed. Nothing was rerun.")
                    .into(),
            );
            return true;
        }
        let request = pending.request;
        let saved_draft = if request.operation != MessageAction::Fork
            && request.mode != Mode::Code
            && (!self.draft.is_empty() || !self.draft_images.is_empty())
        {
            Some(run_status::WaitingMessage {
                text: self.draft.clone(),
                images: self.draft_images.images(),
                mode: self.prefs.settings.delivery,
            })
        } else {
            None
        };
        if request.operation == MessageAction::Fork {
            let Some(path) = value["path"].as_str() else {
                self.state.error =
                    Some("Fork did not return a session path. No prompt was sent.".into());
                return true;
            };
            if !self.agents.background {
                self.ensure_agent();
                self.publish_live();
                self.park_tree();
                self.connect(&self.ctx.clone(), Some(PathBuf::from(path)));
                self.ensure_agent();
            } else {
                self.state.notice = Some(format!(
                    "Fork created at {path}. Open it from the sidebar; nothing was sent."
                ));
                self.refresh_sessions();
                return true;
            }
        } else {
            if request.mode != Mode::Code {
                self.loading_chat = true;
                self.preview_rx = None;
                self.preview_partial = false;
            }
            self.send(protocol::command("get_state"));
            self.send(protocol::command("get_entries"));
            self.send(protocol::command("get_session_stats"));
            self.refresh_workspace();
        }
        if request.operation != MessageAction::Fork && request.mode != Mode::Code {
            if let Some(saved) = saved_draft {
                self.history_drafts.push(saved);
            }
            self.draft = crate::transcript::user_display_text(&request.message).into();
            self.draft_images.set_images(request.message.images);
            self.recall = Default::default();
            self.focus_composer = true;
        }
        self.state.error = None;
        self.state.notice = Some(match (request.operation, request.mode) {
            (MessageAction::Fork, _) => "Fork created including the selected response. The original chat is unchanged. Send a new message when ready.",
            (_, Mode::Code) => "Captured files restored. Conversation and draft kept; nothing was rerun.",
            (_, Mode::Both) => "Captured files restored and prompt ready to edit. Review the draft, then send when ready.",
            (_, Mode::Messages) => "Prompt ready to edit. Files kept unchanged; nothing was rerun. Review the draft, then send when ready.",
        }.into());
        self.refresh_sessions();
        true
    }
    pub(super) fn poll_history(&mut self) {
        let Some(pending) = &mut self.history_pending else {
            return;
        };
        if pending.timed_out {
            return;
        }
        let deadline = Duration::from_secs(30);
        if pending.started.elapsed() >= deadline {
            pending.timed_out = true;
            self.state.error = Some("History navigation timed out; outcome unknown. Sending remains paused until Pi acknowledges or you explicitly reconnect. Check the chat/files before retrying. No automatic replay.".into());
        } else {
            self.ctx
                .request_repaint_after(deadline.saturating_sub(pending.started.elapsed()));
        }
    }
    pub(super) fn history_draft_recovery(&mut self, ui: &mut egui::Ui) {
        if self.history_pending.is_some() {
            ui.horizontal(|ui| {
                ui.label(theme::small(
                    ui,
                    "Returning to your message… · nothing will be sent",
                ));
                if self.history_pending.as_ref().is_some_and(|p| p.timed_out)
                    && ui.small_button("Reconnect to inspect").clicked()
                {
                    self.history_pending = None;
                    self.connect(
                        &self.ctx.clone(),
                        self.state.session_file.as_ref().map(PathBuf::from),
                    );
                }
            });
        }
        if self.history_drafts.is_empty() {
            return;
        }
        ui.horizontal(|ui| {
            ui.label(theme::small(
                ui,
                format!("{} draft(s) saved before rewind", self.history_drafts.len()),
            ));
            if ui.small_button("Restore saved draft").clicked() {
                if self.draft.is_empty() && self.draft_images.is_empty() {
                    let draft = self.history_drafts.remove(0);
                    self.draft = draft.text;
                    self.draft_images.set_images(draft.images);
                    self.focus_composer = true;
                } else {
                    self.state.error = Some(
                        "Clear or send the current draft first; it will not be overwritten.".into(),
                    );
                }
            }
        });
    }
}
