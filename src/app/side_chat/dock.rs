//! Interactive independent chat below the existing right-hand workspace view.
use super::*;

impl Pui {
    pub(in crate::app) fn side_chat_owns_keyboard(&self, ctx: &egui::Context) -> bool {
        if !self.side_chat_visible() {
            return false;
        }
        self.docked_side_chat_target().is_some_and(|target| {
            ctx.memory(|memory| {
                let id = egui::Id::new(("btw-editor", target.as_str()));
                memory.has_focus(id)
                    || memory.had_focus_last_frame(id)
                    || self
                        .side_chat
                        .keyboard_focus
                        .is_some_and(|id| memory.has_focus(id) || memory.had_focus_last_frame(id))
            })
        })
    }

    pub(in crate::app) fn docked_side_chat_target(&self) -> Option<String> {
        if !self.side_chat.docked || !self.side_chat.preview_open {
            return None;
        }
        let root = self
            .side_chat
            .pairs
            .iter()
            .rev()
            .find(|(_, side)| side == &self.agents.active)
            .map(|(main, _)| main)
            .unwrap_or(&self.agents.active);
        self.side_chat.pairs.iter().rev().find_map(|(main, side)| {
            ((main == &self.agents.active || main == root)
                && side != &self.agents.active
                && self.agents.parked.contains_key(side)
                && self.agents.nodes.contains_key(side))
            .then(|| side.clone())
        })
    }

    pub(in crate::app) fn side_chat_workspace(&mut self, ctx: &egui::Context) -> bool {
        let Some(target) = self.docked_side_chat_target() else {
            return false;
        };
        if !self.side_chat_visible() {
            return false;
        }
        let colors = theme::palette(ctx);
        let files = self.files.is_open();
        let available = ctx.available_rect();
        let paint = |ui: &mut egui::Ui, app: &mut Pui| {
            let height = ui.available_height();
            egui::TopBottomPanel::bottom("btw-bottom-chat")
                .default_height((height * 0.55).clamp(200.0, 520.0))
                .height_range(100.0..=(height - 80.0).max(100.0))
                .resizable(true)
                .frame(egui::Frame::new().fill(colors.bg).inner_margin(10))
                .show_inside(ui, |ui| app.docked_chat(ui, &target));
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(colors.bg))
                .show_inside(ui, |ui| {
                    if files {
                        app.prepare_file_block_capture(ctx);
                        let owner = app.review_comment_owner();
                        context_capture::bind_file_owner(ctx, &app.prefs.cwd, &owner);
                        app.files
                            .show_inside(ui, &app.prefs.cwd, &mut app.prefs.table_headers);
                    } else {
                        app.inspector_contents(ui);
                    }
                });
        };
        if available.width() < 700.0 {
            // Preserve a usable main composer on narrow/zoomed windows.
            egui::TopBottomPanel::bottom("btw-workspace-stack")
                .default_height((available.height() * 0.5).max(200.0))
                .height_range(180.0..=(available.height() - 240.0).max(180.0))
                .resizable(true)
                .show(ctx, |ui| paint(ui, self));
        } else {
            egui::SidePanel::right("btw-workspace-dock")
                .default_width(if files {
                    (available.width() * 0.52).clamp(320.0, 760.0)
                } else {
                    360.0
                })
                .width_range(280.0..=(available.width() - 320.0).max(280.0))
                .resizable(true)
                .frame(egui::Frame::new().fill(colors.bg))
                .show(ctx, |ui| paint(ui, self));
        }
        true
    }

    fn docked_chat(&mut self, ui: &mut egui::Ui, target: &str) {
        #[cfg(test)]
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new("btw-chat-rect"), ui.max_rect()));
        let blocked = self.deletion.in_progress()
            || self.deletion.confirming()
            || self.agents.exiting
            || self.deletion.deleting_agent(target)
            || self.agents.permission_changes.contains_key(target)
            || self.agents.nodes[target]
                .path
                .as_deref()
                .is_some_and(|path| self.deletion.deleting_chat(path))
            || egui::Popup::is_any_open(ui.ctx())
            || ui.ctx().memory(|memory| memory.top_modal_layer().is_some());
        let mut open = false;
        let mut dismiss = false;
        ui.horizontal(|ui| {
            icons::inline(ui, Icon::Branch, theme::palette(ui.ctx()).muted);
            ui.label(RichText::new("Side chat").size(12.0).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                dismiss = ui
                    .add_enabled_ui(!blocked, |ui| {
                        icons::button(ui, Icon::Close, "Dismiss side chat")
                    })
                    .inner
                    .on_hover_text("Dismiss companion; keep both chats and runs")
                    .clicked();
                open = ui
                    .add_enabled_ui(!blocked, |ui| {
                        icons::button(ui, Icon::Expand, "Expand side chat")
                    })
                    .inner
                    .on_hover_text("Open the full conversation")
                    .clicked();
            });
        });
        ui.add_space(4.0);
        if dismiss {
            ui.ctx()
                .data_mut(|data| data.remove::<egui::Id>(egui::Id::new("side-chat-paste-editor")));
            self.side_chat.ui = Default::default();
            self.side_chat.pairs.retain(|(_, side)| side != target);
            ui.ctx().input_mut(|input| input.events.clear());
            return;
        }
        if open {
            self.side_chat.ui = Default::default();
            self.side_chat.ui_owner.clear();
            self.side_chat.keyboard_focus = None;
            self.view_agent(target);
            ui.ctx().input_mut(|input| input.events.clear());
            return;
        }
        // Render/send under the side chat's own identity, permission and receipt
        // state, without changing navigation, layouts, or the main chat's draft.
        let Some(chat) = self.agents.parked.remove(target) else {
            return;
        };
        let active = self.agents.active.clone();
        let main = self.take_chat();
        let main_focus = self.focus_composer;
        self.agents.active = target.into();
        self.put_chat(chat);
        if self.side_chat.ui_owner != target {
            self.side_chat.ui = Default::default();
            self.side_chat.ui_owner = target.into();
            self.side_chat.keyboard_focus = None;
        }
        self.swap_side_chat_ui();
        self.side_chat.rendering = true;
        self.focus_composer = std::mem::take(&mut self.side_chat.focus);
        let ctx = ui.ctx().clone();
        let keyboard_rect = ui.max_rect();
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("side-chat-input-rect"), keyboard_rect));
        // Leave captures queued by the main transcript for its own composer.
        // Only captures produced while rendering this transcript belong here.
        let main_capture = context_capture::park_capture(&ctx);
        let mut side_focus_after = None;
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("side-chat-paste-editor"),
                egui::Id::new(("btw-editor", target)),
            )
        });
        let previous_owner = composer_text::set_owner(Some(target.to_owned()));
        ui.push_id(target, |ui| {
            let opened_model = self.model_picker_open || self.effort_picker_open;
            self.model_picker_escape(&ctx);
            let closed_model = opened_model && !self.model_picker_open && !self.effort_picker_open;
            self.context_chip_inputs(&ctx);
            self.poll_image_inputs(&ctx);
            let had_completion = self.completion_active(&ctx);
            let had_question = !self.state.dialogs.is_empty();
            let owns_keys = closed_model
                || self.side_chat_owns_editor(&ctx, target)
                || self.side_chat.keyboard_focus.is_some_and(|id| {
                    ctx.memory(|memory| memory.has_focus(id) || memory.had_focus_last_frame(id))
                })
                || self.model_picker_open
                || self.effort_picker_open
                || ui.input(|input| {
                    input
                        .pointer
                        .interact_pos()
                        .is_some_and(|pos| keyboard_rect.contains(pos))
                        && (input.pointer.any_pressed() || input.pointer.any_released())
                });
            ctx.data_mut(|data| {
                data.insert_temp(egui::Id::new("side-chat-prompt-keyboard"), owns_keys)
            });
            let interaction_blocked = self.deletion.in_progress()
                || self.deletion.confirming()
                || self.agents.exiting
                || self.agents.permission_changes.contains_key(target)
                || self.deletion.deleting_agent(target)
                || self.agents.nodes[target]
                    .path
                    .as_deref()
                    .is_some_and(|path| self.deletion.deleting_chat(path))
                || ctx.memory(|memory| memory.top_modal_layer().is_some());
            let pending_keys = if owns_keys || opened_model || self.completion_active(&ctx) {
                vec![]
            } else {
                ctx.input_mut(|input| {
                    let mut keys = vec![];
                    input.events.retain(|event| {
                        if matches!(
                            event,
                            egui::Event::Key { .. }
                                | egui::Event::Text(_)
                                | egui::Event::Paste(_)
                                | egui::Event::Ime(_)
                        ) {
                            keys.push(event.clone());
                            false
                        } else {
                            true
                        }
                    });
                    keys
                })
            };
            egui::TopBottomPanel::bottom("btw-composer")
                .show_separator_line(false)
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(2, 6)))
                .show_inside(ui, |ui| {
                    // Questions and recovered drafts may be tall: scroll the
                    // composer region without pushing the transcript offscreen.
                    egui::ScrollArea::vertical()
                        .id_salt("btw-composer-scroll")
                        .min_scrolled_height((keyboard_rect.height() * 0.65).clamp(110.0, 360.0))
                        .max_height((keyboard_rect.height() * 0.65).clamp(110.0, 360.0))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.add_enabled_ui(!interaction_blocked, |ui| self.composer(ui, &ctx));
                        });
                });
            if owns_keys
                && !had_completion
                && !had_question
                && !closed_model
                && !self.model_picker_open
                && !self.effort_picker_open
                && !interaction_blocked
                && !egui::Popup::is_any_open(&ctx)
                && !self.state.ready()
                && self.state.dialogs.is_empty()
                && ctx
                    .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                self.stop();
            }
            self.transcript.embedded = true;
            self.transcript.view = self.prefs.transcript_view;
            self.transcript.pending_messages = self.state.pending_messages.clone();
            self.transcript.progress = self.transcript_progress();
            self.transcript.cwd = Some(self.prefs.cwd.clone());
            // History-changing actions require the full conversation's scoped
            // review/confirmation flow. Copy, quote and tool inspection remain.
            self.transcript.message_actions_enabled = false;
            self.transcript.continue_enabled = false;
            self.transcript.show(
                ui,
                &self.state.messages,
                &self.state.model,
                &self.state.thinking,
                !self.state.ready(),
                self.state.session_file.as_deref().unwrap_or(target),
            );
            if self.transcript.message_action.take().is_some() {
                self.state.notice =
                    Some("Expand this chat to edit, rewind or fork its history.".into());
            }
            self.side_transcript_action(&ctx);
            self.collect_queued_context_capture();
            self.model_picker(&ctx);
            side_focus_after = ctx.memory(|memory| memory.focused());
            let pointer_activation =
                ui.input(|input| input.pointer.any_pressed() || input.pointer.any_released());
            let pointer_inside = ui.input(|input| {
                input
                    .pointer
                    .interact_pos()
                    .is_some_and(|pos| keyboard_rect.contains(pos))
            });
            if self.side_chat_owns_editor(&ctx, target)
                || (pointer_activation && pointer_inside)
                || owns_keys
            {
                self.side_chat.keyboard_focus =
                    ctx.memory(|memory| memory.focused()).filter(|id| {
                        *id != egui::Id::new("composer")
                            && ctx.read_response(*id).is_some_and(|response| {
                                keyboard_rect.intersects(response.interact_rect)
                            })
                    });
                ctx.input_mut(|input| {
                    input.events.retain(|event| {
                        !matches!(
                            event,
                            egui::Event::Key { .. }
                                | egui::Event::Text(_)
                                | egui::Event::Paste(_)
                                | egui::Event::Ime(_)
                        )
                    })
                });
            }
            if !pending_keys.is_empty() {
                ctx.input_mut(|input| input.events.extend(pending_keys));
            }
        });
        context_capture::restore_capture(&ctx, main_capture);
        composer_text::set_owner(previous_owner);
        ctx.data_mut(|data| {
            data.remove::<bool>(egui::Id::new("side-chat-prompt-keyboard"));
        });
        self.side_chat.rendering = false;
        self.swap_side_chat_ui();
        self.transcript.embedded = false;
        let side_focus_composer = self.focus_composer;
        let chat = self.take_chat();
        self.agents.parked.insert(target.into(), chat);
        self.agents.active = active;
        self.put_chat(main);
        self.side_chat.focus = side_focus_composer;
        if let Some(focus) = side_focus_after.filter(|_| self.side_chat_owns_keyboard(&ctx)) {
            ctx.memory_mut(|memory| memory.request_focus(focus));
        }
        self.focus_composer = main_focus && !self.side_chat_owns_keyboard(&ctx);
        let owns_keyboard = self.side_chat_owns_keyboard(&ctx);
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("side-chat-main-keyboard-blocked"),
                owns_keyboard,
            )
        });
        // Delivering in the embedded composer has already consumed its Enter.
        // An unfocused main composer must not interpret that activation again.
        if owns_keyboard {
            ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            });
        }
    }
}
