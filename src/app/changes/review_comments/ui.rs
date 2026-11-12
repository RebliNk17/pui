//! Quiet inline review chrome; all mutations are explicit and local.
use super::*;

fn keyboard_event(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Key { .. }
            | egui::Event::Text(_)
            | egui::Event::Paste(_)
            | egui::Event::Copy
            | egui::Event::Cut
            | egui::Event::Ime(_)
    )
}
fn ctx_control_owns(ctx: &egui::Context, id: egui::Id) -> bool {
    ctx.read_response(id).is_some_and(|response| {
        response.enabled()
            && (response.has_focus()
                || response.is_pointer_button_down_on()
                || response.clicked_by(egui::PointerButton::Primary))
    })
}

impl Comments {
    /// Runs before composer/global shortcuts, only for this review's actual focus owner.
    pub(in crate::app) fn inputs(
        &mut self,
        ctx: &egui::Context,
        document: &diff_viewer::Document,
    ) -> bool {
        if ctx.memory(|memory| memory.top_modal_layer().is_some()) || egui::Popup::is_any_open(ctx)
        {
            return false;
        }
        let reverse_focus = self.reverse_focus.take();
        let focused = ctx.memory(|memory| memory.focused());
        if focused == self.after_editor
            && focused.is_some()
            && ctx.input(|input| {
                input.events.iter().any(|event| {
                    matches!(event,
                egui::Event::Key {key:egui::Key::Tab,pressed:true,modifiers,..}
                    if modifiers.shift && !modifiers.ctrl && !modifiers.command && !modifiers.alt)
                })
            })
        {
            self.reverse_focus = self.editor_controls[2];
        }
        let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let owns_editor = self
            .editor
            .as_ref()
            .is_some_and(|editor| Some(&editor.anchor.owner) == self.owner.as_ref())
            && self.editor_controls.iter().flatten().any(|id| {
                ctx.read_response(*id)
                    .is_some_and(|response| response.enabled())
                    && ctx.memory(|memory| {
                        memory.has_focus(*id)
                            || memory.focused().is_none()
                                && escape
                                && (memory.had_focus_last_frame(*id) || reverse_focus == Some(*id))
                    })
            });
        if owns_editor {
            let cancel = ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
            });
            if cancel {
                self.editor = None;
                self.editor_controls = [None; 3];
                document.2.focus(ctx);
            }
            return true;
        }
        let owns_controls = self.controls.iter().any(|id| {
            ctx.read_response(*id)
                .is_some_and(|response| response.enabled())
                && ctx.memory(|memory| {
                    memory.has_focus(*id)
                        || memory.focused().is_none() && escape && memory.had_focus_last_frame(*id)
                })
        });
        // Interaction is computed from the previous live widget geometry before
        // globals run. Scope chord suppression to actual primary activation,
        // including pointer release while the composer still owns keyboard focus.
        let (primary_pressed, pointer_pos) = ctx.input(|input| {
            (
                input.pointer.button_pressed(egui::PointerButton::Primary),
                input.pointer.interact_pos(),
            )
        });
        let pointer_layer = pointer_pos.and_then(|pos| ctx.layer_id_at(pos));
        let pointer_action = self
            .controls
            .iter()
            .chain(self.editor_controls.iter().flatten())
            .any(|id| {
                ctx.read_response(*id).is_some_and(|response| {
                    response.enabled()
                        && response.sense.senses_click()
                        && (response.is_pointer_button_down_on()
                            || response.clicked_by(egui::PointerButton::Primary)
                            || primary_pressed
                                && pointer_pos.is_some_and(|pos| {
                                    response.interact_rect.contains(pos)
                                        && pointer_layer == Some(response.layer_id)
                                }))
                })
            });
        if owns_controls || pointer_action {
            let close = ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
                owns_controls && input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
            });
            if close {
                self.expanded = false;
                document.2.focus(ctx);
                return true;
            }
        }
        if (document.2.owns_focus(ctx) || owns_controls)
            && ctx.input_mut(|input| {
                input.consume_key(
                    egui::Modifiers {
                        alt: true,
                        ..egui::Modifiers::COMMAND
                    },
                    egui::Key::M,
                )
            })
        {
            self.request_add = true;
        }
        false
    }

    pub(in crate::app) fn show(
        &mut self,
        ui: &mut egui::Ui,
        path: &str,
        diff: &str,
        recorded: bool,
        navigation: &mut diff_viewer::Navigation,
    ) {
        let old_control_owner = self
            .controls
            .iter()
            .any(|id| ctx_control_owns(ui.ctx(), *id));
        self.controls.clear();
        self.after_editor = None;
        let digest = self
            .source_root
            .as_ref()
            .filter(|root| root.as_os_str().len() <= 4096)
            .and_then(|_| navigation.comment_digest());
        self.bind_document(path, recorded, navigation);
        let ctx = ui.ctx().clone();
        let enabled = prompt_ui::keyboard_available(ui)
            && !egui::Popup::is_any_open(&ctx)
            && ctx.memory(|memory| memory.top_modal_layer().is_none());
        let owned = self
            .items
            .iter()
            .filter(|item| Some(&item.anchor.owner) == self.owner.as_ref())
            .count();
        let others = self.items.len() - owned;
        ui.horizontal_wrapped(|ui| {
            let label = if self.items.is_empty() { "Comments".into() } else { format!("Comments {owned}{}", if others > 0 { format!(" (+{others} other session)") } else { String::new() }) };
            let toggle = ui.add_enabled(enabled, egui::Button::new(theme::small(ui, label)).frame(false).selected(self.expanded));
            navigation.register(9, toggle.id);
            self.controls.push(toggle.id);
            if toggle.clicked() { self.expanded = !self.expanded; }
            let add = ui.add_enabled(enabled && digest.is_some() && self.editor.is_none() && self.owner.is_some(),
                egui::Button::new(theme::small(ui, "Comment")).frame(false))
                .on_hover_text(if digest.is_none() { "Comments unavailable for this snapshot. Reopen after refresh. Requires ≤2 MiB and the exact session repository root." } else { "Select text in one hunk, then Comment · ⌘/Ctrl+Alt+M. Local to this window; never sent automatically." });
            navigation.register(8, add.id);
            self.controls.push(add.id);
            // Freeze only the small character range on pointer-down. A later button click
            // must not borrow a different label selection or rely on text matching.
            let key = navigation.focus_id().with("review-comment-range");
            // Loading label selection state can clone egui's selection buffer; do
            // that only for an explicit operation, never while the pane is idle.
            let selection = (add.is_pointer_button_down_on() || add.clicked() || self.request_add)
                .then(|| navigation.selection_response(&ctx).and_then(|response|
                    egui::text_selection::LabelSelectionState::load(&ctx).range_for_response(&response))
                    .map(|range| range.as_sorted_char_range())).flatten();
            if add.is_pointer_button_down_on() {
                ctx.data_mut(|data| data.insert_temp(key, selection.clone()));
            }
            if enabled && (add.clicked() || std::mem::take(&mut self.request_add)) {
                let range = if add.clicked() { ctx.data_mut(|data| data.remove_temp::<Option<std::ops::Range<usize>>>(key)).flatten().or(selection.clone()) } else { selection.clone() };
                if let Some(range) = range {
                    self.begin(path, diff, recorded, digest, range);
                } else {
                    self.error = Some("Select source text in this diff first (one hunk only).");
                }
            }
            #[cfg(test)]
            ctx.data_mut(|data| {
                data.insert_temp(egui::Id::new("review-comment-add"), add.rect);
                data.insert_temp(egui::Id::new("review-comment-toggle"), toggle.rect);
            });
        });
        if let Some(error) = self.error {
            ui.add(egui::Label::new(theme::small(ui, error)).wrap());
        }
        if self.expanded {
            prompt_ui::inline(&ctx).inner_margin(10).show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                ui.add(egui::Label::new(theme::small(ui, "Local review · this window only · closing the app loses notes. Other sessions remain separate. No automatic reattachment.")).wrap());
                let editor_visible = self.editor.as_ref().is_some_and(|editor| Some(&editor.anchor.owner) == self.owner.as_ref());
                if editor_visible { self.editor_ui(ui, navigation); }
                else if self.editor.is_some() {
                    ui.add(egui::Label::new(theme::small(ui, "An unfinished note belongs to another repository/session. Return there to finish it, or explicitly Clear all.")).wrap());
                }
                egui::ScrollArea::vertical().id_salt("local-review-notes").max_height(140.0).show(ui, |ui| {
                    for (index, item) in self.items.iter_mut().enumerate().filter(|(_, item)| Some(&item.anchor.owner) == self.owner.as_ref()) {
                        ui.push_id(index, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                let current = item.anchor.path == path && item.anchor.recorded == recorded && Some(item.anchor.digest) == digest;
                                let label = format!("{}{}{}", if current { String::new() } else { format!("{} · ", item.anchor.path) }, item.anchor.selection.label(), if item.stale { " · STALE" } else { "" });
                                ui.add(egui::Label::new(theme::small(ui, label)).wrap()).on_hover_text(format!("{}\n{}\nOriginal immutable snapshot; never retargeted.", item.anchor.source(), item.anchor.root.display()));
                                let action = ui.add_enabled(enabled, egui::Button::new(theme::small(ui, if item.resolved { "Reopen" } else { "Resolve" })).frame(false));
                                self.controls.push(action.id);
                                if self.after_editor.is_none() && action.enabled() { self.after_editor = Some(action.id); }
                                if action.clicked() { item.resolved = !item.resolved; }
                                #[cfg(test)]
                                ctx.data_mut(|data| data.insert_temp(egui::Id::new(("review-comment-resolve", index)), action.rect));
                            });
                            ui.add(egui::Label::new(RichText::new(&item.text).size(12.0).color(if item.resolved { theme::palette(&ctx).dim } else { theme::palette(&ctx).text })).wrap());
                        });
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    let available = self.items.iter().any(|item| !item.resolved && Some(&item.anchor.owner) == self.owner.as_ref());
                    let add = prompt_ui::button(ui, "Add unresolved to draft", prompt_ui::Button::Secondary, enabled && available)
                        .on_hover_text("Adds a complete unresolved batch for this repository/session, up to 24 KiB, to the draft only. Never sends. Notes remain here.");
                    self.controls.push(add.id);
                    if self.after_editor.is_none() && add.enabled() { self.after_editor = Some(add.id); }
                    // Formatting/digest hex/text copying only on explicit batch action.
                    if add.clicked() {
                        match self.batch() {
                            Ok(batch) => if let Some(owner) = &self.owner {
                                context_capture::queue_review(&ctx, &owner.root, &owner.session, "Local review comments (original snapshots)", &batch);
                            },
                            Err(error) => self.error = Some(error),
                        }
                    }
                    let clear = ui.add_enabled(enabled && (!self.items.is_empty() || self.editor.is_some()), egui::Button::new(theme::small(ui, "Clear all")).frame(false));
                    self.controls.push(clear.id);
                    if self.after_editor.is_none() && clear.enabled() { self.after_editor = Some(clear.id); }
                    if clear.clicked() { self.clear_requested = true; }
                    #[cfg(test)]
                    ctx.data_mut(|data| data.insert_temp(egui::Id::new("review-comment-clear"), clear.rect));
                    #[cfg(test)]
                    ctx.data_mut(|data| data.insert_temp(egui::Id::new("review-comment-batch"), add.rect));
                });
            });
        }
        let controls_owned =
            old_control_owner || self.controls.iter().any(|id| ctx_control_owns(&ctx, *id));
        if enabled && controls_owned {
            self.control_keyboard_pass = Some(ctx.cumulative_pass_nr());
            ui.input_mut(|input| input.events.retain(|event| !keyboard_event(event)));
        }
        if self.clear_requested {
            match confirmations::Dialog::new("clear-local-review-comments", "Clear local review comments?", "Removes all local notes and unfinished comment text in this window, including notes owned by other sessions. Draft context already added is unchanged.", "Cancel", "Clear all").show(&ctx, |_| {}) {
                Some(confirmations::Choice::Confirm) => { self.items.clear(); self.editor = None; self.editor_controls = [None;3]; self.error = None; self.clear_requested = false; }
                Some(confirmations::Choice::Cancel) => self.clear_requested = false,
                None => {}
            }
        }
    }
    fn editor_ui(&mut self, ui: &mut egui::Ui, navigation: &diff_viewer::Navigation) {
        let mut save = false;
        let mut cancel = false;
        let editor = self.editor.as_mut().unwrap();
        prompt_ui::style(ui);
        prompt_ui::heading(
            ui,
            "Local comment",
            &format!(
                "{} · {}{}",
                editor.anchor.source(),
                editor.anchor.selection.label(),
                if editor.stale {
                    " · STALE original snapshot"
                } else {
                    ""
                }
            ),
        );
        let text = ui.add(
            egui::TextEdit::multiline(&mut editor.text)
                .id(egui::Id::new("local-review-comment-text"))
                .desired_rows(2)
                .desired_width(ui.available_width())
                .char_limit(MAX_TEXT)
                .hint_text("Review note (up to 2 KiB UTF-8)…"),
        );
        if editor.first {
            text.request_focus();
            editor.first = false;
        }
        if editor.text.len() > MAX_TEXT {
            let mut end = MAX_TEXT;
            while !editor.text.is_char_boundary(end) {
                end -= 1;
            }
            editor.text.truncate(end);
            editor.text.shrink_to_fit();
            self.error =
                Some("Comment text limited to 2 KiB UTF-8; excess input was not retained.");
        }
        prompt_ui::footer(ui, |ui| {
            let add = prompt_ui::button(
                ui,
                "Add comment",
                prompt_ui::Button::Primary,
                !editor.text.trim().is_empty(),
            );
            save = add.clicked();
            let dismiss = prompt_ui::button(ui, "Cancel", prompt_ui::Button::Secondary, true);
            cancel = dismiss.clicked();
            self.editor_controls = [Some(text.id), Some(add.id), Some(dismiss.id)];
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("review-comment-save"), add.rect);
                data.insert_temp(egui::Id::new("review-comment-cancel"), dismiss.rect);
            });
        });
        // Lock Escape on the same pass focus is acquired (including native Tab),
        // before egui's next begin_pass can clear focus and leak Escape globally.
        if let Some(id) = ui.ctx().memory(|memory| memory.focused())
            && self.editor_controls.contains(&Some(id))
        {
            ui.ctx().memory_mut(|memory| {
                memory.request_focus_with_filter(
                    id,
                    egui::EventFilter {
                        escape: true,
                        vertical_arrows: true,
                        horizontal_arrows: true,
                        ..Default::default()
                    },
                )
            });
        }
        if self.editor_owns_keyboard(ui.ctx()) {
            // TextEdit has processed its events, but raw input is still visible to
            // later inline questions. Never reuse comment typing as an answer.
            self.control_keyboard_pass = Some(ui.ctx().cumulative_pass_nr());
            ui.input_mut(|input| input.events.retain(|event| !keyboard_event(event)));
        }
        if save && self.save() || cancel {
            self.editor = None;
            self.editor_controls = [None; 3];
            navigation.focus(ui.ctx());
        }
    }
}
