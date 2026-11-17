//! Compact catch-up presentation; navigation never answers an agent.
use super::*;

pub(super) fn popup_id() -> egui::Id {
    egui::Id::new("unread-chats")
}

fn dismiss_shield(ctx: &egui::Context) {
    let pass = ctx.cumulative_pass_nr();
    let stamp = egui::Id::new("unread-shield-pass");
    if ctx.data(|d| d.get_temp::<u64>(stamp)) == Some(pass) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(stamp, pass));
    // Transparent, non-focusable interaction layer: an outside click cannot
    // become a press on the approval/composer beneath this dropdown.
    let id = egui::Id::new("unread-chats-dismiss-shield");
    egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(ctx.screen_rect().min)
        .movable(false)
        .constrain(false)
        .sense(Sense::CLICK | Sense::DRAG)
        .show(ctx, |ui| {
            ui.allocate_exact_size(ctx.screen_rect().size(), Sense::CLICK | Sense::DRAG);
        });
    ctx.set_sublayer(
        egui::LayerId::new(egui::Order::Foreground, id),
        egui::LayerId::new(egui::Order::Foreground, popup_id()),
    );
}

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
            || self.changes.review_comments.confirming()
            || self.tasks.attachment_confirming()
            || self.task_operations_confirming()
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
        let geometry = (
            ctx.screen_rect(),
            ctx.zoom_factor(),
            self.prefs.hide_sidebar,
        );
        self.attention.defer_layout = self.attention.geometry != Some(geometry);
        if self.attention_trigger_blocked()
            || ctx.memory(|m| m.top_modal_layer().is_some())
            || (egui::Popup::is_any_open(ctx) && !egui::Popup::is_id_open(ctx, popup_id()))
        {
            self.attention.open = false;
            egui::Popup::close_id(ctx, popup_id());
            return;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            dismiss_shield(ctx);
            self.attention.open = false;
            egui::Popup::close_id(ctx, popup_id());
            self.attention.restore_focus = self.attention.trigger_focus.take();
            swallow_activation(ctx);
            return;
        }
        // Outside press/release dismisses without clicking an approval, send or
        // stop control underneath. The popup itself remains non-modal visually.
        let outside = ctx.input(|input| {
            (input.pointer.any_pressed() || input.pointer.any_released())
                && input.pointer.interact_pos().is_some_and(|pos| {
                    !self.attention.defer_layout
                        && self
                            .attention
                            .popup_rect
                            .is_some_and(|rect| !rect.contains(pos))
                })
        });
        if outside {
            dismiss_shield(ctx);
            self.attention.open = false;
            egui::Popup::close_id(ctx, popup_id());
            self.attention.restore_focus = self.attention.trigger_focus.take();
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::PointerButton { .. } | egui::Event::Touch { .. }
                    )
                })
            });
            swallow_activation(ctx);
            return;
        }
        // Keep Tab inside the small dropdown; no underlying prompt may acquire
        // an Enter/Space activation while it is open.
        let reverse = ctx.input_mut(|input| {
            if input.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab) {
                Some(true)
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::Tab) {
                Some(false)
            } else {
                None
            }
        });
        if let Some(reverse) = reverse {
            let ids = &self.attention.focus_ids;
            if !ids.is_empty() {
                let current = ctx
                    .memory(|m| m.focused())
                    .and_then(|id| ids.iter().position(|i| *i == id));
                let next = match current {
                    Some(i) if reverse => (i + ids.len() - 1) % ids.len(),
                    Some(i) => (i + 1) % ids.len(),
                    None => 0,
                };
                ctx.memory_mut(|m| m.request_focus(ids[next]));
            }
        }
        // Render before composer/approvals. Modified chords must not activate a
        // focused dropdown action as plain Enter.
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
        let response = icons::button(
            ui,
            Icon::Clock,
            &format!(
                "Unread chats · {count} unread · this window\n{}",
                self.attention_dock_hint()
            ),
        );
        self.attention.last_button = Some(response.id);
        self.attention.anchor = Some((response.rect, ui.layer_id()));
        self.attention.geometry = Some((
            ui.ctx().screen_rect(),
            ui.ctx().zoom_factor(),
            self.prefs.hide_sidebar,
        ));
        self.attention.defer_layout = false;
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
                format!("Unread chats · {count} unread"),
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
            // Opening must not activate the safe close control or type the
            // trigger's paired Space into the underlying draft.
            swallow_activation(ui.ctx());
            self.attention.popup_rect = None;
            self.attention.trigger_focus = Some(response.id);
            self.attention.trigger_owner = self.attention_owner();
            self.attention.open = true;
            self.attention.focus_close = true;
            self.focus_composer = false;
            self.close_model_popovers();
            egui::Popup::open_id(ui.ctx(), popup_id());
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
            egui::Popup::close_id(ctx, popup_id());
            return;
        }
        if self.attention.defer_layout {
            dismiss_shield(ctx);
            swallow_activation(ctx);
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
        let Some((anchor, layer)) = self.attention.anchor else {
            // Demo/direct opens wait until the real trigger has been laid out.
            return;
        };
        if !egui::Popup::is_id_open(ctx, popup_id()) {
            egui::Popup::open_id(ctx, popup_id());
        }
        dismiss_shield(ctx);
        let colors = theme::palette(ctx);
        let width = 340.0_f32.min((ctx.screen_rect().width() - 24.0).max(120.0));
        let mut focus_ids = Vec::new();
        let response = egui::Popup::new(popup_id(), ctx.clone(), anchor, layer)
            .open_memory(None)
            .align(egui::RectAlign::BOTTOM_START)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .gap(6.0)
            .width(width)
            .frame(prompt_ui::popup(ctx).inner_margin(12))
            .show(|ui| {
                prompt_ui::style(ui);
                ui.set_width((width - 24.0).max(0.0));
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        prompt_ui::compact_heading(ui, "Unread chats");
                        ui.label(theme::small(
                            ui,
                            format!(
                                "{} unread · this window{}",
                                rows.len(),
                                if self.demo { " · Demo" } else { "" }
                            ),
                        ));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let dismiss =
                            prompt_ui::icon_action(ui, Icon::Close, "Close unread chats", true);
                        focus_ids.push(dismiss.id);
                        #[cfg(test)]
                        ctx.data_mut(|d| {
                            d.insert_temp(
                                egui::Id::new("attention-close"),
                                (dismiss.id, dismiss.rect),
                            )
                        });
                        if self.attention.focus_close && !ui.is_sizing_pass() {
                            dismiss.request_focus();
                            self.attention.focus_close = false;
                        }
                        close |= dismiss.clicked();
                    });
                });
                egui::ScrollArea::vertical()
                    .id_salt("catch-up-list")
                    .max_height((ctx.screen_rect().height() - 200.0).clamp(48.0, 280.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if rows.is_empty() {
                            ui.add_space(10.0);
                            ui.label(theme::small(ui, "You're caught up."));
                            ui.add_space(10.0);
                        }
                        for (index, (entry, (title, project))) in rows.iter().enumerate() {
                            if index > 0 {
                                ui.separator();
                            }
                            let active = self.attention_owner().as_ref() == Some(&entry.owner);
                            ui.push_id(&entry.metadata.revision, |ui| {
                                ui.horizontal(|ui| {
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
                                    let (rect, row) = ui.allocate_exact_size(
                                        Vec2::new((ui.available_width() - 32.0).max(0.0), 42.0),
                                        Sense::click(),
                                    );
                                    focus_ids.push(row.id);
                                    if row.hovered() || row.has_focus() {
                                        ui.painter().rect_filled(rect, 5, colors.hover);
                                    }
                                    if row.has_focus() {
                                        ui.painter().rect_stroke(
                                            rect.shrink(0.5),
                                            5,
                                            Stroke::new(1.0, colors.accent),
                                            egui::StrokeKind::Inside,
                                        );
                                    }
                                    ui.painter().circle_filled(
                                        rect.left_center() + egui::vec2(4.0, 0.0),
                                        2.0,
                                        colors.accent,
                                    );
                                    let left = rect.left() + 14.0;
                                    super::super::new_chat::project_picker::paint_text(
                                        ui,
                                        &title,
                                        egui::Rect::from_min_max(
                                            egui::pos2(left, rect.top() + 2.0),
                                            egui::pos2(rect.right() - 3.0, rect.top() + 23.0),
                                        ),
                                        12.0,
                                        colors.text,
                                    );
                                    super::super::new_chat::project_picker::paint_text(
                                        ui,
                                        &detail,
                                        egui::Rect::from_min_max(
                                            egui::pos2(left, rect.top() + 23.0),
                                            egui::pos2(rect.right() - 3.0, rect.bottom()),
                                        ),
                                        10.0,
                                        colors.dim,
                                    );
                                    row.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            ui.is_enabled(),
                                            format!("Open chat: {title}, {detail}"),
                                        )
                                    });
                                    #[cfg(test)]
                                    ctx.data_mut(|d| {
                                        d.insert_temp(egui::Id::new("attention-first-row"), rect)
                                    });
                                    if row.on_hover_text("Open chat").clicked() {
                                        open = Some(entry.clone());
                                    }
                                    let mark =
                                        prompt_ui::icon_action(ui, Icon::Check, "Mark read", true);
                                    focus_ids.push(mark.id);
                                    #[cfg(test)]
                                    ctx.data_mut(|d| {
                                        d.insert_temp(
                                            egui::Id::new("attention-mark-read"),
                                            mark.rect,
                                        )
                                    });
                                    if mark.clicked() {
                                        read = Some(entry.owner.clone());
                                    }
                                });
                            });
                        }
                    });
                ui.separator();
                prompt_ui::footer(ui, |ui| {
                    ui.label(RichText::new("Esc to close").size(9.0).color(colors.dim));
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        let mark = icons::labeled_button(
                            ui,
                            Icon::Check,
                            "Mark all read",
                            !rows.is_empty(),
                        );
                        if mark.enabled() {
                            focus_ids.push(mark.id);
                        }
                        clear |= mark.clicked();
                    });
                });
            });
        self.attention.popup_rect = response.as_ref().map(|response| response.response.rect);
        self.attention.focus_ids = focus_ids;
        if let Some(id) = ctx
            .memory(|m| m.focused())
            .filter(|id| self.attention.focus_ids.contains(id))
        {
            ctx.memory_mut(|m| {
                m.set_focus_lock_filter_current(
                    id,
                    egui::EventFilter {
                        tab: true,
                        escape: true,
                        ..Default::default()
                    },
                )
            });
        }
        if let Some(owner) = read {
            self.attention.mark_read(&owner);
            self.attention.focus_close = true;
        }
        if clear {
            self.attention.mark_all_read();
            self.attention.focus_close = true;
        }
        if !egui::Popup::is_id_open(ctx, popup_id()) || close {
            self.attention.open = false;
            egui::Popup::close_id(ctx, popup_id());
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
            egui::Popup::close_id(ctx, popup_id());
            self.attention.trigger_focus = None;
            self.open_attention(&entry);
        }
        // A pointer selection plus approval/send/Stop chord is still an inbox
        // interaction for this entire frame, even when it closes the dropdown.
        swallow_activation(ctx);
    }
}
