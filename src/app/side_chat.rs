//! User-created independent conversations, never delegated worker tasks.
use super::*;

#[derive(Default)]
pub(super) struct SideChat {
    // Runtime-only presentation links. Saved chats remain ordinary root conversations.
    pairs: Vec<(String, String)>,
    preview_open: bool,
}
impl Pui {
    /// Static presentation fixture: never connects, sends, or claims real model activity.
    pub(super) fn demo_side_chat(&mut self) {
        let main = self.ensure_agent();
        self.agents.nodes.get_mut(&main).unwrap().status = "Example main · working".into();
        let chat = self.take_chat();
        self.agents.parked.insert(main.clone(), chat);
        self.agents.active.clear();
        self.state=State{session_name:"Side chat · design preview".into(),messages:vec![Message{role:"assistant".into(),text:"## A little room for a tangent\n\nThis is an **independent conversation**. The main chat keeps its own context and model run.\n\nUse **Back to main** to return. Nothing here is sent or merged into the main conversation automatically.\n\n*Static design preview — no model was launched.*".into(),..Default::default()}],..Default::default()};
        self.draft = "What are the tradeoffs of this approach?".into();
        let side = self.ensure_agent();
        self.side_chat.pairs.push((main, side));
        self.side_chat.preview_open = true;
        self.prefs.hide_inspector = true;
    }
    pub(super) fn start_side_chat(&mut self, question: &str) -> bool {
        self.side_chat.pairs.retain(|(a, b)| {
            self.agents.nodes.contains_key(a) && self.agents.nodes.contains_key(b)
        });
        if self.demo
            || self.loading_chat
            || self.history_pending.is_some()
            || self.restoring_code()
            || self
                .agents
                .nodes
                .get(&self.agents.active)
                .is_some_and(|node| node.access != "workspace" || node.parent.is_some())
            || self.is_mirror()
            || self.deletion.in_progress()
            || self.agents.exiting
            || self
                .agents
                .permission_changes
                .contains_key(&self.agents.active)
            || self.agents.nodes.len() >= 128
            || self.side_chat.pairs.len() >= 16
            || !self.prefs.cwd.is_dir()
            || !self.draft_images.is_empty()
        {
            self.state.error=Some("Side chat unavailable: connect to a local writable chat, finish access/deletion changes, remove attachments, or keep within 16 side chats / 128 sessions. Your draft was kept.".into());
            return false;
        }
        let origin = self.ensure_agent();
        let permission = self.active_permission();
        let cwd = self.prefs.cwd.clone();
        // The command itself is not part of either model context.
        self.draft.clear();
        self.context_chips.clear();
        self.completions = Default::default();
        // Make even a pathless empty origin retainable for the explicit return action.
        self.new_chat = false;
        self.start_chat_with_permission(cwd, Some(permission));
        let side = self.ensure_agent();
        if side == origin {
            return false;
        }
        let node = self.agents.nodes.get_mut(&side).unwrap();
        node.name = "Side chat".into();
        node.permission = Some(permission);
        self.state.session_name = "Side chat".into();
        self.draft = question.into();
        let origin = self
            .side_chat
            .pairs
            .iter()
            .rev()
            .find(|(_, child)| child == &origin)
            .map(|(main, _)| main.clone())
            .unwrap_or(origin);
        self.side_chat.pairs.push((origin, side));
        self.side_chat.preview_open = true;
        self.state.notice=Some("Independent side chat · no main-chat context copied. Review and send your question when connected. Nothing is merged back automatically.".into());
        self.focus_composer = true;
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
