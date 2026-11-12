//! Compact catch-up presentation; navigation never answers an agent.
use super::*;

fn swallow_activation(ctx: &egui::Context) {
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            !matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::Enter
                        | egui::Key::Escape
                        | egui::Key::Backspace
                        | egui::Key::Space,
                    ..
                } | egui::Event::Text(_)
                    | egui::Event::Paste(_)
            )
        })
    });
}

impl Pui {
    fn attention_trigger_blocked(&self) -> bool {
        self.settings_open
            || self.question_designs.is_some()
            || self.search_open
            || self.agents.close_prompt
            || self.tasks.attachment_confirming()
            || self.section_view.confirming()
            || self.terminals.confirming_stop()
            || self.history_view.is_open()
            || self.deletion.confirming()
            || self.deletion.in_progress()
            || self.automation.confirming()
            || self.menu.about
            || self.updates.open
            || self.transcript.gallery.is_open()
            || self.draft_images.gallery.is_open()
    }
    pub(in crate::app) fn attention_inputs(&mut self, ctx: &egui::Context) {
        if !self.attention.open {
            if self.attention_trigger_blocked()
                || egui::Popup::is_any_open(ctx)
                || ctx.memory(|m| m.top_modal_layer().is_some())
            {
                return;
            }
            let current_hit = self.attention.button_hit.filter(|(_, _, screen, frame)| {
                *screen == ctx.screen_rect() && frame.saturating_add(1) >= ctx.cumulative_frame_nr()
            });
            if current_hit.is_none() {
                return;
            }
            let reverse_focus = self.attention.reverse_focus.take();
            if let Some(from) = self.attention.reverse_from
                && ctx.memory(|m| m.has_focus(from))
                && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::Tab, pressed: true, modifiers, .. } if *modifiers == egui::Modifiers::SHIFT))) {
                self.attention.reverse_focus = self.attention.last_button;
            }
            // Native Tab assigns focus at end-pass, after the button rendered.
            // Escape can clear that focus at begin-pass before the button installs
            // its filter. Scope the repair to the actual previous trigger owner.
            if let Some(id) = self.attention.last_button
                && ctx.memory(|m| {
                    m.had_focus_last_frame(id)
                        || (m.focused().is_none() && reverse_focus == Some(id))
                })
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                ctx.memory_mut(|m| {
                    m.request_focus_with_filter(
                        id,
                        egui::EventFilter {
                            escape: true,
                            ..Default::default()
                        },
                    )
                });
            }
            // Reserve actual press and release interactions on the last live,
            // enabled, clipped trigger before global Stop/inline approval keys.
            let activation = self
                .attention
                .last_button
                .is_some_and(|id| ctx.memory(|m| m.has_focus(id)))
                && ctx
                    .input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space));
            let pointer = ctx.input(|i| {
                (i.pointer.button_pressed(egui::PointerButton::Primary)
                    || i.pointer.button_released(egui::PointerButton::Primary))
                .then(|| i.pointer.interact_pos())
                .flatten()
            });
            let pointer_activation = current_hit.is_some_and(|(rect, layer, _, _)| {
                pointer.is_some_and(|pos| rect.contains(pos) && ctx.layer_id_at(pos) == Some(layer))
            });
            if pointer_activation {
                swallow_activation(ctx);
            } else if activation {
                // Preserve the trigger key itself so egui still activates it.
                ctx.input_mut(|i| {
                    i.events.retain(|event| {
                        !matches!(event, egui::Event::Text(_) | egui::Event::Paste(_))
                    })
                });
            }
            return;
        }
        // The custom prompt renders before the composer/inline approvals. Keep
        // modified chords from activating its focused Done button as plain Enter.
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                !matches!(event,
                    egui::Event::Key { key: egui::Key::Enter | egui::Key::Backspace, modifiers, .. }
                        if !modifiers.is_none()
                )
            })
        });
    }
    pub(in crate::app) fn attention_reverse_from(&mut self, id: egui::Id) {
        self.attention.reverse_from = Some(id);
        #[cfg(test)]
        self.ctx
            .data_mut(|d| d.insert_temp(egui::Id::new("attention-next-button"), id));
    }
    pub(in crate::app) fn attention_button(&mut self, ui: &mut egui::Ui) {
        let count = self.attention.unread();
        let response = icons::button(ui, Icon::Clock, &format!("Catch up · {count} unread"));
        self.attention.last_button = Some(response.id);
        if self.attention.restore_focus == Some(response.id) {
            ui.ctx().memory_mut(|m| {
                m.request_focus_with_filter(
                    response.id,
                    egui::EventFilter {
                        escape: true,
                        ..Default::default()
                    },
                )
            });
            self.attention.restore_focus = None;
        }
        self.attention.button_hit = (response.enabled() && response.sense.senses_click())
            .then_some((
                response.rect.intersect(ui.clip_rect()),
                ui.layer_id(),
                ui.ctx().screen_rect(),
                ui.ctx().cumulative_frame_nr(),
            ));
        if response.has_focus() {
            ui.painter().rect_stroke(
                response.rect,
                6,
                Stroke::new(1.0, theme::palette(ui.ctx()).accent),
                egui::StrokeKind::Inside,
            );
        }
        #[cfg(test)]
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("attention-button-id"), response.id));
        if response.has_focus() {
            ui.ctx().memory_mut(|m| {
                m.request_focus_with_filter(
                    response.id,
                    egui::EventFilter {
                        escape: true,
                        ..Default::default()
                    },
                )
            });
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Catch up · {count} unread"),
            )
        });
        if count > 0 {
            ui.painter().circle_filled(
                response.rect.right_top() + egui::vec2(-5.0, 5.0),
                3.0,
                theme::palette(ui.ctx()).accent,
            );
        }
        #[cfg(test)]
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("attention-button"), response.rect));
        let blocked = self.attention_trigger_blocked() || egui::Popup::is_any_open(ui.ctx());
        self.attention.reverse_from = None;
        #[cfg(test)]
        ui.ctx()
            .data_mut(|d| d.remove::<egui::Id>(egui::Id::new("attention-next-button")));
        if response.clicked() && !blocked {
            // A keyboard-triggered open must not activate the modal's safe Done
            // focus or type its paired Space into the underlying draft.
            swallow_activation(ui.ctx());
            self.attention.trigger_focus = Some(response.id);
            self.attention.trigger_owner = self.attention_owner();
            self.attention.open = true;
            self.attention.focus_close = true;
            self.focus_composer = false;
            self.close_model_popovers();
            egui::Popup::close_all(ui.ctx());
        }
    }
    fn attention_target(&self, entry: &Entry) -> Option<(String, String)> {
        match &entry.owner {
            Owner::Runtime(id) => self.agents.nodes.get(id).map(|n| {
                (
                    n.name
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(100)
                        .collect(),
                    sessions::project_name(&n.cwd),
                )
            }),
            Owner::Saved(path) => {
                let title = self
                    .prefs
                    .chats
                    .get(path)
                    .and_then(|p| p.title.clone())
                    .or_else(|| {
                        self.agents
                            .nodes
                            .values()
                            .find(|n| {
                                n.path
                                    .as_ref()
                                    .is_some_and(|p| self.sync.attention_identity(p) == *path)
                            })
                            .map(|n| n.name.clone())
                            .or_else(|| {
                                self.sessions
                                    .iter()
                                    .find(|s| self.sync.attention_identity(&s.path) == *path)
                                    .map(|s| s.title.clone())
                            })
                            .or_else(|| {
                                Some(
                                    path.file_stem()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .into_owned(),
                                )
                            })
                    })
                    .unwrap_or_else(|| "Chat".into());
                Some((title, sessions::project_name(&entry.metadata.cwd)))
            }
        }
    }
    pub(super) fn open_attention(&mut self, entry: &Entry) {
        if self.deletion.in_progress() {
            return;
        }
        match &entry.owner {
            Owner::Runtime(id) => {
                if self.agents.nodes.contains_key(id) {
                    self.view_agent(id);
                }
            }
            Owner::Saved(path) => {
                if !path.is_file() {
                    self.attention.forget_path(path);
                    self.state.notice = Some(
                        "This chat is no longer available; its unread update was cleared.".into(),
                    );
                    return;
                }
                if self.deletion.deleting_chat(path) {
                    return;
                }
                if let Some(id) = self
                    .agents
                    .nodes
                    .values()
                    .find(|n| {
                        n.path
                            .as_ref()
                            .is_some_and(|p| self.sync.attention_identity(p) == *path)
                    })
                    .map(|n| n.id.clone())
                {
                    self.view_agent(&id);
                } else {
                    // Reuse established chat navigation, but retain even an idle
                    // pathless/source-only draft rather than replacing its context.
                    self.ensure_agent();
                    self.park_tree();
                    self.open_chat(Session {
                        path: path.clone(),
                        cwd: entry.metadata.cwd.clone(),
                        project: entry.metadata.cwd.clone(),
                        title: "Opening chat…".into(),
                        created: None,
                        modified: std::time::SystemTime::UNIX_EPOCH,
                    });
                }
            }
        }
        // Navigation itself is not a read. The actual rendered owner marks read.
    }
    pub(in crate::app) fn attention_panel(&mut self, ctx: &egui::Context) {
        if !self.attention.open {
            return;
        }
        // No continuous timer, transcript scan, or model polling. Validate only
        // when opening/rendering this bounded list (also on restore/save/delete).
        self.attention.entries.retain(|e| {
            e.metadata.at <= now() && now().saturating_sub(e.metadata.at) <= MAX_AGE_SECONDS
        });
        let rows: Vec<_> = self
            .attention
            .entries
            .iter()
            .rev()
            .filter(|e| !e.metadata.read)
            .filter_map(|e| self.attention_target(e).map(|target| (e.clone(), target)))
            .collect();
        let valid: Vec<_> = rows.iter().map(|(e, _)| e.owner.clone()).collect();
        self.attention
            .entries
            .retain(|e| e.metadata.read || valid.contains(&e.owner));
        let mut close = false;
        let mut open = None;
        let mut read = None;
        let mut clear = false;
        let response = prompt_ui::modal(ctx, egui::Id::new("catch-up"), 400.0, |ui| {
            close |= prompt_ui::close(ui);
            prompt_ui::heading(
                ui,
                "Catch up",
                if self.demo {
                    "Synthetic demo only · no agent work was run. Open chat never answers or approves."
                } else {
                    "Observed updates only. Open chat never answers or approves."
                },
            );
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .id_salt("catch-up-list")
                .max_height((ctx.screen_rect().height() - 250.0).clamp(60.0, 310.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    if rows.is_empty() {
                        ui.label(theme::small(
                            ui,
                            "You're caught up. Saved history is not counted as unread.",
                        ));
                    }
                    for (entry, (title, project)) in &rows {
                        let active = self.attention_owner().as_ref() == Some(&entry.owner);
                        ui.push_id(&entry.metadata.revision, |ui| {
                            let title: String = title
                                .chars()
                                .filter(|c| !c.is_control())
                                .take(100)
                                .collect();
                            let detail = format!(
                                "{} · {}{}",
                                entry.metadata.kind.label(),
                                project,
                                if active { " · Active chat" } else { "" }
                            );
                            let response = prompt_ui::option(ui, &title, &detail, active)
                                .on_hover_text("Open chat");
                            #[cfg(test)]
                            ctx.data_mut(|d| {
                                d.insert_temp(egui::Id::new("attention-first-row"), response.rect)
                            });
                            if response.clicked() {
                                open = Some(entry.clone());
                            }
                            if prompt_ui::button(
                                ui,
                                "Mark read",
                                prompt_ui::Button::Secondary,
                                true,
                            )
                            .clicked()
                            {
                                read = Some(entry.owner.clone());
                            }
                        });
                    }
                });
            ui.add_space(8.0);
            prompt_ui::footer(ui, |ui| {
                let done = prompt_ui::button(ui, "Done", prompt_ui::Button::Secondary, true);
                #[cfg(test)]
                ctx.data_mut(|d| {
                    d.insert_temp(egui::Id::new("attention-done"), (done.id, done.rect))
                });
                if self.attention.focus_close || done.has_focus() {
                    ctx.memory_mut(|m| {
                        m.request_focus_with_filter(
                            done.id,
                            egui::EventFilter {
                                escape: true,
                                ..Default::default()
                            },
                        )
                    });
                    self.attention.focus_close = false;
                }
                close |= done.clicked();
                clear |= prompt_ui::button(
                    ui,
                    "Clear unread",
                    prompt_ui::Button::Secondary,
                    !rows.is_empty(),
                )
                .clicked();
            });
        });
        if let Some(owner) = read {
            self.attention.mark_read(&owner);
        }
        if clear {
            self.attention.mark_all_read();
        }
        if response.should_close() || close {
            self.attention.open = false;
            if self.attention.trigger_owner == self.attention_owner() {
                if let Some(trigger) = self.attention.trigger_focus.take() {
                    self.attention.restore_focus = Some(trigger);
                } else {
                    self.focus_composer = true;
                }
            }
        }
        if let Some(entry) = open {
            self.attention.open = false;
            self.attention.trigger_focus = None;
            self.open_attention(&entry);
        }
        // A pointer selection plus approval/send/Stop chord is still an inbox
        // interaction for this entire frame, even when it closes the modal.
        swallow_activation(ctx);
    }
}
