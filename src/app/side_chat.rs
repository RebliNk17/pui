//! User-created independent conversations, never delegated worker tasks.
use super::*;

#[derive(Default)]
pub(super) struct SideChat {
    // Runtime-only presentation links. Saved chats remain ordinary root conversations.
    pairs: Vec<(String, String)>,
    preview_open: bool,
    docked: bool,
    focus: bool,
    pub(super) rendering: bool,
    ui: ui_state::ChatUi,
    ui_owner: String,
    keyboard_focus: Option<egui::Id>,
}
mod actions;
#[cfg(test)]
mod control_tests;
mod dock;
#[cfg(test)]
mod dock_safety_tests;
#[cfg(test)]
mod dock_tests;
#[cfg(test)]
mod interaction_tests;
#[cfg(test)]
mod ownership_tests;
#[cfg(test)]
mod redesign_tests;
#[cfg(test)]
mod routing_tests;
mod ui_state;
impl Pui {
    /// Static presentation fixture: never connects, sends, or claims real model activity.
    pub(super) fn demo_side_chat(&mut self) {
        let main = self.ensure_agent();
        self.agents.nodes.get_mut(&main).unwrap().status = "Example main · working".into();
        let chat = self.take_chat();
        self.agents.parked.insert(main.clone(), chat);
        self.agents.active.clear();
        self.state=State{session_name:"Side chat · design preview".into(),messages:vec![Message{role:"assistant".into(),text:"## A little room for a tangent\n\nThis is an **independent conversation**. The main chat keeps its own context and model run.\n\nUse **Expand** for full-size history controls. Nothing here is sent or merged into the main conversation automatically.\n\n*Static design preview — no model was launched.*".into(),..Default::default()}],..Default::default()};
        self.state.model = "gpt-6.1-sol".into();
        self.state.model_name = "GPT-6.1 Sol".into();
        self.state.provider = "litellm".into();
        self.state.thinking = "high".into();
        self.draft = "What are the tradeoffs of this approach?".into();
        let side = self.ensure_agent();
        let side_chat = self.take_chat();
        self.agents.parked.insert(side.clone(), side_chat);
        let main_chat = self.agents.parked.remove(&main).unwrap();
        self.put_chat(main_chat);
        self.agents.active = main.clone();
        self.side_chat.pairs.push((main, side));
        self.side_chat.preview_open = true;
        self.side_chat.docked = true;
        self.focus_composer = false;
        self.prefs.hide_inspector = false;
    }
    pub(super) fn is_side_chat_companion(&self, id: &str) -> bool {
        self.side_chat
            .pairs
            .iter()
            .any(|(main, side)| main == id || side == id)
    }
    fn live_side_chat_connections(&self) -> usize {
        usize::from(self.client.is_some())
            + self
                .agents
                .parked
                .values()
                .filter(|chat| chat.client.is_some())
                .count()
            + self
                .agents
                .nodes
                .values()
                .filter(|node| node.setup_pending())
                .count()
    }
    pub(super) fn start_docked_side_chat(&mut self, question: &str) -> bool {
        // Creating the independent RPC chat must not navigate the workspace:
        // keep the selected files/browser, main transcript, and main run in place.
        if !self.side_chat_creation_allowed() {
            return false;
        }
        let origin = self.ensure_agent();
        let permission = match self.active_permission() {
            settings::PermissionMode::Bypass => match self.prefs.settings.permission {
                settings::PermissionMode::Bypass => settings::PermissionMode::Auto,
                mode => mode,
            },
            mode => mode,
        };
        let cwd = self.prefs.cwd.clone();
        self.draft.clear();
        self.context_chips.clear();
        self.context_chips.clear_quotes();
        self.completions = Default::default();
        self.state.error = None;
        self.new_chat = false;
        let main = self.take_chat();
        self.agents.active.clear();
        let background = self.agents.background;
        self.agents.background = true;
        let side = self.ensure_agent();
        self.agents.background = background;
        self.agents.nodes.get_mut(&side).unwrap().permission = Some(permission);
        self.new_chat = true;
        self.loading_chat = true;
        self.seed_model_catalog();
        #[cfg(not(test))]
        {
            self.client = Some(rpc::Client::start_with(
                &cwd,
                None,
                self.ctx.clone(),
                rpc::Launch {
                    settings: self.agent_settings(&side).payload(),
                    ..Default::default()
                },
            ));
        }
        #[cfg(test)]
        {
            self.client = Some(rpc::Client::stub());
        }
        for kind in [
            "get_state",
            "get_available_models",
            "get_available_thinking_levels",
            "get_entries",
            "get_commands",
            "get_session_stats",
        ] {
            self.send(protocol::command(kind));
        }
        self.state.session_name = "Side chat".into();
        self.agents.nodes.get_mut(&side).unwrap().name = "Side chat".into();
        self.draft = question.into();
        let chat = self.take_chat();
        self.agents.parked.insert(side.clone(), chat);
        self.agents.active = origin.clone();
        self.prefs.cwd = cwd;
        self.put_chat(main);
        let origin = self
            .side_chat
            .pairs
            .iter()
            .rev()
            .find(|(_, child)| child == &origin)
            .map(|(main, _)| main.clone())
            .unwrap_or(origin);
        self.side_chat.pairs.push((origin, side));
        self.side_chat.docked = true;
        self.side_chat.preview_open = true;
        self.side_chat.focus = true;
        self.focus_composer = false;
        true
    }
    fn side_chat_creation_allowed(&mut self) -> bool {
        self.side_chat.pairs.retain(|(a, b)| {
            self.agents.nodes.contains_key(a) && self.agents.nodes.contains_key(b)
        });
        let reason = if self.demo || self.is_mirror() {
            Some("select a local writable conversation")
        } else if self.loading_chat || self.history_pending.is_some() || self.restoring_code() {
            Some("wait for the current chat operation to finish")
        } else if self
            .agents
            .nodes
            .get(&self.agents.active)
            .is_some_and(|node| node.access != "workspace" || node.parent.is_some())
        {
            Some("select a writable main conversation, not a delegated worker")
        } else if self.deletion.in_progress()
            || self.deletion.confirming()
            || self.agents.exiting
            || self
                .agents
                .permission_changes
                .contains_key(&self.agents.active)
        {
            Some("finish the current access, deletion, or exit request")
        } else if self.live_side_chat_connections() >= 128 {
            Some(
                "128 live connections reached; close an unused connection (saved history does not count)",
            )
        } else if self.side_chat.pairs.len() >= 16 {
            Some("16 side chats reached; dismiss an unused companion")
        } else if !self.prefs.cwd.is_dir() {
            Some("choose an available project folder")
        } else if !self.draft_images.is_empty() {
            Some("remove attachments before opening a side chat")
        } else {
            None
        };
        if let Some(reason) = reason {
            self.state.error = Some(format!(
                "Side chat unavailable: {reason}. Your draft was kept."
            ));
            false
        } else {
            true
        }
    }
    // Full-size fixture also exercises the explicit open/return navigation path.
    #[cfg(test)]
    pub(super) fn start_side_chat(&mut self, question: &str) -> bool {
        if !self.start_docked_side_chat(question) {
            return false;
        }
        let side = self.side_chat.pairs.last().unwrap().1.clone();
        self.side_chat.docked = false;
        self.new_chat = false;
        self.view_agent(&side);
        true
    }
    pub(super) fn return_side_chat(&mut self) -> bool {
        let target = self
            .side_chat
            .pairs
            .iter()
            .rev()
            .find(|(_, side)| side == &self.agents.active)
            .map(|(main, _)| main.clone());
        let Some(target) = target.filter(|target| {
            self.agents.nodes.contains_key(target) && self.agents.parked.contains_key(target)
        }) else {
            self.state.error = Some(
                "No originating conversation is available for /main. Your draft was kept.".into(),
            );
            return false;
        };
        let blocked = self.deletion.in_progress()
            || self.deletion.confirming()
            || self.agents.exiting
            || self.history_pending.is_some()
            || self.restoring_code()
            || !self.state.dialogs.is_empty()
            || egui::Popup::is_any_open(&self.ctx)
            || self.ctx.memory(|memory| memory.top_modal_layer().is_some())
            || self.deletion.deleting_agent(&target)
            || self.agents.nodes[&target]
                .path
                .as_deref()
                .is_some_and(|path| self.deletion.deleting_chat(path));
        if blocked {
            self.state.error=Some("Finish or dismiss the current request before returning to main. Your draft was kept.".into());
            return false;
        }
        self.draft.clear();
        self.context_chips.clear();
        self.completions = Default::default();
        self.recall = Default::default();
        context_capture::clear_source(&self.ctx);
        self.view_agent(&target);
        self.ctx.input_mut(|input| input.events.clear());
        true
    }
    pub(super) fn side_chat_bar(&mut self, ctx: &egui::Context) {
        if self.docked_side_chat_target().is_some() {
            return;
        }
        let active = self.agents.active.clone();
        let Some((origin, side)) = self
            .side_chat
            .pairs
            .iter()
            .rev()
            .find(|(a, b)| a == &active || b == &active)
        else {
            return;
        };
        let target = if origin == &active {
            side.clone()
        } else {
            origin.clone()
        };
        let is_side = side == &active;
        let Some(node) = self.agents.nodes.get(&target) else {
            return;
        };
        if !self.agents.parked.contains_key(&target) {
            return;
        }
        let chat = &self.agents.parked[&target];
        let status = preview::observed_status(chat, node).to_owned();
        let waiting = !chat.state.dialogs.is_empty();
        let colors = theme::palette(ctx);
        let blocked = egui::Popup::is_any_open(ctx)
            || ctx.memory(|memory| memory.top_modal_layer().is_some())
            || self.deletion.deleting_agent(&target)
            || node
                .path
                .as_deref()
                .is_some_and(|path| self.deletion.deleting_chat(path));
        let mut switch = false;
        let mut dismiss = false;
        egui::TopBottomPanel::bottom("independent-side-chat-rail").show(ctx, |ui| {
            let compact=ui.available_width()<640.0;
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(if is_side { "Side chat" } else { "Main chat" }).strong()).on_hover_text("Independent context · no automatic merge");
                if !compact{ui.label(theme::small(ui, "Independent context · no automatic merge"));}
                let response = ui
                    .push_id((&active, &target), |ui| {
                        ui.add_enabled(
                            !blocked
                                && !self.deletion.in_progress()
                                && !self.deletion.confirming()
                                && !self.agents.exiting,
                            egui::Button::new(if is_side {
                                if compact{"Main"}else{"Back to main"}
                            } else if compact{"Side chat"}else{"Open side chat"}),
                        )
                    })
                    .inner;
                #[cfg(test)]
                ctx.data_mut(|data| {
                    data.insert_temp(egui::Id::new("side-chat-switch"), response.id)
                });
                switch = response.clicked();
                let preview = ui.add_enabled(
                    !blocked,
                    egui::Button::new(if self.side_chat.preview_open {
                        if compact{"Hide"}else{"Hide preview"}
                    } else {
                        "Preview"
                    }),
                );
                #[cfg(test)]
                ctx.data_mut(|data| {
                    data.insert_temp(egui::Id::new("side-chat-preview-toggle"), preview.id)
                });
                if preview.clicked() {
                    self.side_chat.preview_open = !self.side_chat.preview_open;
                    ctx.input_mut(|input| input.events.clear());
                }
                let dismiss_button=ui.push_id(("dismiss-companion",&active,&target),|ui|ui.add_enabled(!blocked && !self.deletion.in_progress() && !self.agents.exiting,egui::Button::new(if compact{"Dismiss"}else{"Dismiss companion"}))).inner.on_hover_text("Remove this temporary pairing only. Both chats and their runs remain available in the sidebar; /main will no longer use this link.");
                #[cfg(test)]ctx.data_mut(|data|data.insert_temp(egui::Id::new("side-chat-dismiss"),dismiss_button.id));
                dismiss=dismiss_button.clicked();
                ui.label(
                    RichText::new(if waiting {
                        "Other chat needs input"
                    } else if compact {preview::compact_status(chat)}else{&status})
                    .size(11.0)
                    .color(if waiting { colors.amber } else { colors.muted }),
                ).on_hover_text(&status);
            });
        });
        if dismiss {
            self.side_chat.pairs.retain(|(main, side)| {
                !((main == &active && side == &target) || (side == &active && main == &target))
            });
            ctx.input_mut(|input| input.events.clear());
            return;
        }
        // Yield space to explicit workspace tools without overwriting the user's
        // preview preference. A hidden preview never becomes context or navigation.
        let conversation_visible = !self.files.is_open()
            && !self.inspector_panel
            && !self.terminals.is_open()
            && !self.tasks.open
            && !self.settings_open
            && !self.kanban.open
            && !self.automation.open;
        if self.side_chat.preview_open && !switch && conversation_visible {
            self.side_chat_preview(ctx, &target, is_side);
        }
        if switch {
            context_capture::clear_source(ctx);
            self.view_agent(&target);
            ctx.input_mut(|input| input.events.clear());
        }
    }
}
#[cfg(test)]
mod dismiss_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod main_tests;
#[cfg(test)]
mod navigation_tests;
mod preview;
#[cfg(test)]
mod preview_tests;
#[cfg(all(test, unix))]
mod process_tests;
#[cfg(test)]
mod safety_tests;
#[cfg(test)]
mod tests;
