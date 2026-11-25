//! Restart checkpoints contain unsent local composers, never executable RPC state.
//! Composer payloads are exact and unbounded by the old notice-only retention cap.
use super::*;
mod recovery;
pub(super) use recovery::Recovery;

#[cfg(test)]
mod tests;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Checkpoint {
    path: Option<PathBuf>,
    cwd: PathBuf,
    draft: String,
    notice: String,
    images: Vec<crate::media::Attachment>,
    history_drafts: Vec<run_status::WaitingMessage>,
    agent_id: Option<String>,
    name: String,
    blank: bool,
    parked: bool,
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
            blank: path.is_none(),
            path,
            cwd,
            draft: draft.to_owned(),
            notice,
            ..Default::default()
        }
    }

    fn has_composer(&self) -> bool {
        !self.draft.is_empty() || !self.images.is_empty() || !self.history_drafts.is_empty()
    }

    fn same_chat(&self, other: &Self) -> bool {
        if self.path.is_some() || other.path.is_some() {
            return self.path == other.path;
        }
        match (&self.agent_id, &other.agent_id) {
            (Some(a), Some(b)) if a == b => true,
            // The startup blank may not have received an agent identity yet.
            _ => self.cwd == other.cwd && !self.parked && !other.parked,
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
        let mut active = Checkpoint::capture(
            self.pending_session.clone().or_else(|| {
                (!self.new_chat)
                    .then(|| self.state.session_file.as_ref().map(PathBuf::from))
                    .flatten()
            }),
            self.prefs.cwd.clone(),
            &self.state,
            &self.draft,
            !self.is_mirror(),
        );
        active.blank = active.path.is_none();
        active.agent_id = (!self.agents.active.is_empty()).then(|| self.agents.active.clone());
        active.name = self
            .agents
            .nodes
            .get(&self.agents.active)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| self.state.chat_title().unwrap_or_default());
        active.images = self.draft_images.images();
        active.history_drafts = self.history_drafts.clone();
        let mut checkpoints = Vec::new();
        // A save before startup hydration must not replace a retained attachment
        // or recovery shelf with an empty placeholder.
        if !(self.draft.is_empty()
            && self.draft_images.is_empty()
            && self.history_drafts.is_empty()
            && self.restart_pending.iter().any(|c| c.same_chat(&active)))
        {
            checkpoints.push(active);
        }
        for (id, chat) in &self.agents.parked {
            if let Some(node) = self.agents.nodes.get(id) {
                let mut checkpoint = Checkpoint::capture(
                    chat.pending.clone().or_else(|| {
                        (!chat.new_chat)
                            .then(|| chat.state.session_file.as_ref().map(PathBuf::from))
                            .flatten()
                    }),
                    node.cwd.clone(),
                    &chat.state,
                    &chat.draft,
                    // Mirror drafts are local; their owner's activity is not.
                    !(chat.client.is_none() && chat.connected),
                );
                checkpoint.agent_id = Some(id.clone());
                checkpoint.name = node.name.clone();
                checkpoint.blank = checkpoint.path.is_none();
                checkpoint.parked = true;
                (checkpoint.images, checkpoint.history_drafts) = chat.composer_snapshot();
                // Unopened saved placeholders must not erase their checkpoint.
                if chat.composer_is_empty()
                    && self
                        .restart_pending
                        .iter()
                        .any(|c| c.same_chat(&checkpoint))
                {
                    continue;
                }
                checkpoints.push(checkpoint);
            }
        }
        for checkpoint in checkpoints {
            self.prefs
                .restart_checkpoints
                .retain(|c| !c.same_chat(&checkpoint));
            if checkpoint.has_composer() || !checkpoint.notice.is_empty() || checkpoint.blank {
                self.prefs.restart_checkpoints.insert(0, checkpoint);
            }
        }
        // No record cap: even the 129th live/parked composer must survive.
    }

    pub(super) fn forget_composer_agent_checkpoints(
        &mut self,
        ids: &std::collections::BTreeSet<String>,
    ) {
        let keep = |c: &Checkpoint| c.agent_id.as_ref().is_none_or(|id| !ids.contains(id));
        self.prefs.restart_checkpoints.retain(keep);
        self.restart_pending.retain(keep);
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

    /// Startup only, after restore_agents and only without explicit cwd/session.
    /// Saved side chats recover as ordinary chats; no presentation links, clients,
    /// context associations, queue entries, permissions or work are reconstructed.
    pub(super) fn restore_parked_composers(&mut self) {
        let ids: Vec<_> = self
            .restart_pending
            .iter()
            .filter_map(|c| c.agent_id.clone())
            .collect();
        self.reserve_composer_agent_ids(&ids);
        let pending = std::mem::take(&mut self.restart_pending);
        for checkpoint in pending {
            // Keep the selected blank for the existing startup-only blank path.
            if checkpoint.path.is_none() && !checkpoint.parked {
                self.restart_pending.push(checkpoint);
                continue;
            }
            if !checkpoint.has_composer() && !checkpoint.blank && checkpoint.notice.is_empty() {
                continue;
            }
            let id = self.restore_composer_agent(
                checkpoint.agent_id.as_deref(),
                &checkpoint.name,
                &checkpoint.cwd,
                checkpoint.path.as_deref(),
                checkpoint.blank || checkpoint.path.is_none(),
            );
            if let Some(chat) = self.agents.parked.get_mut(&id) {
                if chat.composer_is_empty() {
                    chat.restore_composer(
                        &checkpoint.draft,
                        &checkpoint.images,
                        &checkpoint.history_drafts,
                    );
                    if chat.state.restart_notice.is_empty() {
                        chat.state.restart_notice.clone_from(&checkpoint.notice);
                    }
                    self.prefs
                        .restart_checkpoints
                        .retain(|c| !c.same_chat(&checkpoint));
                    continue;
                }
            }
            self.restart_pending.push(checkpoint);
        }
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
        if !self.draft.is_empty()
            || !self.draft_images.is_empty()
            || !self.history_drafts.is_empty()
            || self.image_picker_rx.is_some()
        {
            return;
        }
        if let Some(index) = self.restart_pending.iter().position(|c| {
            c.path == path && (path.is_some() || (!c.parked && c.cwd == self.prefs.cwd))
        }) {
            let checkpoint = self.restart_pending.remove(index);
            self.prefs
                .restart_checkpoints
                .retain(|c| !c.same_chat(&checkpoint));
            self.context_chips.clear();
            checkpoint.apply(&mut self.state, &mut self.draft);
            self.draft_images.set_images(checkpoint.images);
            self.history_drafts = checkpoint.history_drafts;
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
