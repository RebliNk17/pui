//! Restart checkpoints are local recovery hints, never executable RPC state.
use super::*;

#[cfg(test)]
mod tests;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Checkpoint {
    path: Option<PathBuf>,
    cwd: PathBuf,
    draft: String,
    notice: String,
}

impl Checkpoint {
    fn capture(
        path: Option<PathBuf>,
        cwd: PathBuf,
        state: &State,
        draft: &str,
        owner: bool,
    ) -> Self {
        let notice = if !owner {
            String::new()
        } else if !state.dialogs.is_empty() {
            let question = state.dialogs.front().and_then(questions::Request::parse);
            match question {
                Some(q) => format!(
                    "Interrupted while waiting for your answer: {}",
                    q.summary().chars().take(240).collect::<String>()
                ),
                None => "Interrupted while waiting for input or approval.".into(),
            }
        } else if !state.ready()
            || state.pending_messages.iter().any(|m| {
                matches!(
                    m.status,
                    protocol::DeliveryStatus::Sending
                        | protocol::DeliveryStatus::Interrupting
                        | protocol::DeliveryStatus::AwaitingAgent
                        | protocol::DeliveryStatus::Uncertain
                )
            })
        {
            "This chat was interrupted before its work finished.".into()
        } else {
            state.restart_notice.clone()
        };
        Self {
            path,
            cwd,
            draft: draft.chars().take(256 * 1024).collect(),
            notice,
        }
    }

    fn apply(&self, state: &mut State, draft: &mut String) {
        if draft.is_empty() {
            draft.clone_from(&self.draft);
        }
        if state.restart_notice.is_empty() {
            state.restart_notice.clone_from(&self.notice);
        }
    }
}

impl Pui {
    pub(super) fn checkpoint_chats(&mut self) {
        if self.demo || self.screenshot.is_some() || self.agents.exiting {
            return;
        }
        let mut checkpoints = vec![Checkpoint::capture(
            self.pending_session.clone().or_else(|| {
                (!self.new_chat)
                    .then(|| self.state.session_file.as_ref().map(PathBuf::from))
                    .flatten()
            }),
            self.prefs.cwd.clone(),
            &self.state,
            &self.outgoing_text(),
            !self.is_mirror(),
        )];
        for (id, chat) in &self.agents.parked {
            if let Some(node) = self.agents.nodes.get(id) {
                // Only the visible blank chat has an unambiguous startup identity.
                if chat.new_chat {
                    continue;
                }
                // Unopened saved placeholders must not erase their checkpoint.
                if chat.client.is_none()
                    && chat.state.messages.is_empty()
                    && chat.draft.is_empty()
                    && !chat.context_chips.has_quotes()
                    && chat.state.restart_notice.is_empty()
                {
                    continue;
                }
                // Mirror drafts are local; their owner's activity is not.
                let mirror = chat.client.is_none() && chat.connected;
                checkpoints.push(Checkpoint::capture(
                    chat.pending.clone().or_else(|| {
                        (!chat.new_chat)
                            .then(|| chat.state.session_file.as_ref().map(PathBuf::from))
                            .flatten()
                    }),
                    node.cwd.clone(),
                    &chat.state,
                    &chat.context_chips.compose(&chat.draft),
                    !mirror,
                ));
            }
        }
        for checkpoint in checkpoints {
            self.prefs.restart_checkpoints.retain(|c| {
                !(c.path == checkpoint.path && (c.path.is_some() || c.cwd == checkpoint.cwd))
            });
            if !checkpoint.draft.is_empty() || !checkpoint.notice.is_empty() {
                self.prefs.restart_checkpoints.insert(0, checkpoint);
            }
        }
        self.prefs.restart_checkpoints.truncate(128);
    }

    pub(super) fn forget_restart_checkpoint(&mut self, path: &std::path::Path) {
        let keep = |c: &Checkpoint| {
            c.path
                .as_deref()
                .is_none_or(|p| sync::identity(p) != sync::identity(path))
        };
        self.prefs.restart_checkpoints.retain(keep);
        self.restart_pending.retain(keep);
    }

    pub(super) fn restore_restart_checkpoint(&mut self) {
        let path = self
            .pending_session
            .clone()
            .or_else(|| self.state.session_file.as_ref().map(PathBuf::from));
        // Blank drafts are restored only at startup, not for every New chat.
        if path.is_some() {
            self.restore_checkpoint_for(path);
        }
    }

    pub(super) fn restore_blank_checkpoint(&mut self) {
        self.restore_checkpoint_for(None);
    }

    fn restore_checkpoint_for(&mut self, path: Option<PathBuf>) {
        if let Some(index) = self
            .restart_pending
            .iter()
            .position(|c| c.path == path && (path.is_some() || c.cwd == self.prefs.cwd))
        {
            let checkpoint = self.restart_pending.remove(index);
            let adopt = self.draft.is_empty() && !self.context_chips.has_quotes();
            checkpoint.apply(&mut self.state, &mut self.draft);
            if adopt {
                // Captured context was checkpointed as blocks; show it as cards again.
                let text = std::mem::take(&mut self.draft);
                self.draft = self.context_chips.adopt_quotes(&text);
            }
        }
    }

    pub(super) fn restart_recovery(&mut self, ui: &mut egui::Ui) {
        if self.is_mirror() || self.state.restart_notice.is_empty() {
            return;
        }
        let colors = theme::palette(ui.ctx());
        ui.label(
            RichText::new(&self.state.restart_notice)
                .size(11.0)
                .color(colors.accent),
        );
        ui.label(theme::small(ui, "Review the transcript, then send a message to continue. Questions and approvals must be asked again. Nothing was replayed."));
        if prompt_ui::button(ui, "Dismiss", prompt_ui::Button::Secondary, true).clicked() {
            self.state.restart_notice.clear();
        }
        ui.add_space(6.0);
    }
}
