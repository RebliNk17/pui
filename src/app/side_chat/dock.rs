//! Interactive independent chat below the existing right-hand workspace view.
use super::*;

impl Pui {
    pub(in crate::app) fn side_chat_owns_keyboard(&self, ctx: &egui::Context) -> bool {
        self.docked_side_chat_target().is_some_and(|target| {
            ctx.memory(|memory| {
                let id = egui::Id::new(("btw-editor", target.as_str()));
                memory.has_focus(id) || memory.had_focus_last_frame(id)
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
        if self.tasks.open
            || self.kanban.open
            || self.automation.open
            || self.terminals.is_open()
            || self.inspector_panel
        {
            return false;
        }
        let colors = theme::palette(ctx);
        let files = self.files.is_open();
        let available = ctx.available_rect();
        let paint = |ui: &mut egui::Ui, app: &mut Pui| {
            let height = ui.available_height();
            egui::TopBottomPanel::bottom("btw-bottom-chat")
                .default_height((height * 0.48).clamp(140.0, 400.0))
                .height_range(100.0..=(height - 80.0).max(100.0))
                .resizable(true)
                .frame(egui::Frame::new().fill(colors.bg).inner_margin(10))
                .show_inside(ui, |ui| app.docked_chat(ui, &target));
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(colors.bg))
                .show_inside(ui, |ui| {
                    if files {
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
            ui.label(RichText::new("Side chat").strong())
                .on_hover_text("Independent context · nothing is merged into the main chat");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                dismiss = ui
                    .add_enabled(!blocked, egui::Button::new("×"))
                    .on_hover_text("Dismiss companion; keep both chats and runs")
                    .clicked();
                open = ui
                    .add_enabled(!blocked, egui::Button::new("Open"))
                    .on_hover_text("Open full conversation to review requests or reconnect")
                    .clicked();
            });
        });
        if dismiss {
            self.side_chat.pairs.retain(|(_, side)| side != target);
            ui.ctx().input_mut(|input| input.events.clear());
            return;
        }
        if open {
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
        self.focus_composer = false;
        ui.push_id(target, |ui| {
            if let Some(error) = &self.state.error {
                ui.colored_label(theme::palette(ui.ctx()).red, error);
            }
            if !self.state.dialogs.is_empty() {
                ui.label(theme::small(
                    ui,
                    "Needs input · open this chat to review its request",
                ));
            }
            let ready = !blocked
                && self.connected
                && self.client.is_some()
                && !self.loading_chat
                && self.state.ready()
                && self.ultracode_ready
                && self.state.dialogs.is_empty()
                && !self.resource_reload.pending()
                && !self.resource_reload.confirming();
            egui::TopBottomPanel::bottom("btw-composer")
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 6)))
                .show_inside(ui, |ui| {
                    let editor = egui::ScrollArea::vertical()
                        .id_salt("btw-editor-scroll")
                        .max_height(72.0)
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut self.draft)
                                    .id(egui::Id::new(("btw-editor", target)))
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY)
                                    .hint_text("Ask a side question…")
                                    .return_key(egui::KeyboardShortcut::new(
                                        egui::Modifiers::SHIFT,
                                        egui::Key::Enter,
                                    )),
                            )
                        })
                        .inner;
                    if self.side_chat.focus {
                        editor.request_focus();
                        self.side_chat.focus = false;
                    }
                    let enter = editor.has_focus()
                        && ui.input(|input| {
                            input.key_pressed(egui::Key::Enter) && !input.modifiers.shift
                        });
                    let send_button = ui.add_enabled(
                        ready && !self.draft.trim().is_empty(),
                        egui::Button::new("Send"),
                    );
                    #[cfg(test)]
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(egui::Id::new("btw-send"), send_button.id)
                    });
                    let send = send_button.clicked();
                    if enter || send {
                        // Never let this activation reach a main-chat approval or Send.
                        ui.ctx().input_mut(|input| input.events.clear());
                        if ready && !self.draft.trim().is_empty() {
                            let message = run_status::WaitingMessage {
                                text: self.draft.trim().into(),
                                images: vec![],
                                mode: settings::DeliveryMode::Queue,
                            };
                            if self.deliver_message(message, "prompt") {
                                self.draft.clear();
                                editor.request_focus();
                            }
                        }
                    }
                    if editor.has_focus() || editor.lost_focus() {
                        ui.ctx().input_mut(|input| {
                            input.events.retain(|event| {
                                !matches!(
                                    event,
                                    egui::Event::Key { .. }
                                        | egui::Event::Text(_)
                                        | egui::Event::Paste(_)
                                )
                            })
                        });
                    }
                });
            egui::ScrollArea::vertical()
                .id_salt("btw-messages")
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if self.state.messages.is_empty() {
                        ui.label(theme::small(
                            ui,
                            if self.loading_chat {
                                "Connecting…"
                            } else {
                                "Independent conversation · no main-chat context copied"
                            },
                        ));
                    }
                    if self.state.messages.len() > 32 {
                        ui.label(theme::small(
                            ui,
                            "Showing the latest 32 messages · Open for the full conversation",
                        ));
                    }
                    for message in
                        &self.state.messages[self.state.messages.len().saturating_sub(32)..]
                    {
                        ui.label(
                            RichText::new(if message.role == "assistant" {
                                "Assistant"
                            } else {
                                "You"
                            })
                            .small()
                            .strong(),
                        );
                        let mut end = message.text.len().min(16 * 1024);
                        while !message.text.is_char_boundary(end) {
                            end -= 1;
                        }
                        ui.add(
                            egui::Label::new(&message.text[..end])
                                .wrap()
                                .selectable(true),
                        );
                        if end < message.text.len() {
                            ui.label(theme::small(ui, "Message shortened · Open to read in full"));
                        }
                        ui.add_space(6.0);
                    }
                    if self.state.busy {
                        ui.spinner();
                    }
                });
        });
        let chat = self.take_chat();
        self.agents.parked.insert(target.into(), chat);
        self.agents.active = active;
        self.put_chat(main);
        self.focus_composer = main_focus;
    }
}
