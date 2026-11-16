//! Quiet inline review chrome; all mutations are explicit and local.
use super::*;

pub(super) fn keyboard_event(event: &egui::Event) -> bool {
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
        if self.confirming() {
            return true;
        }
        if self.marker_inputs(ctx, document) {
            return true;
        }
        let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let pointer = ctx.input(|input| {
            (input.pointer.button_pressed(egui::PointerButton::Primary)
                || input.pointer.button_released(egui::PointerButton::Primary))
            .then(|| input.pointer.interact_pos())
            .flatten()
        });
        let pointer_layer = pointer.and_then(|pos| ctx.layer_id_at(pos));
        let editor_pointer = self.editor_controls.iter().flatten().any(|id| {
            ctx.read_response(*id).is_some_and(|response| {
                response.enabled()
                    && pointer.is_some_and(|pos| {
                        response.interact_rect.contains(pos)
                            && pointer_layer == Some(response.layer_id)
                    })
            })
        });
        if editor_pointer {
            ctx.input_mut(|input| {
                input.events.retain(|event| !matches!(event,
                    egui::Event::Key { key:egui::Key::Enter|egui::Key::Backspace,modifiers,..} if modifiers.command || modifiers.ctrl
                ) && !matches!(event,
                    egui::Event::Key {key:egui::Key::Enter|egui::Key::Space,..} | egui::Event::Text(_) | egui::Event::Paste(_)
                ));
            });
            self.control_keyboard_pass = Some(ctx.cumulative_pass_nr());
        }
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
                                && memory.focus_lost_by_escape(*id)
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
                        || memory.focused().is_none() && escape && memory.focus_lost_by_escape(*id)
                })
        });
        // Interaction is computed from the previous live widget geometry before
        // globals run. Scope chord suppression to actual primary activation,
        // including pointer release while the composer still owns keyboard focus.
        let (primary_activation, pointer_pos) = ctx.input(|input| {
            (
                input.pointer.button_pressed(egui::PointerButton::Primary)
                    || input.pointer.button_released(egui::PointerButton::Primary),
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
                            || primary_activation
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
                if self
                    .editor
                    .as_ref()
                    .is_some_and(|editor| Some(&editor.anchor.owner) == self.owner.as_ref())
                {
                    // Reverse Tab from the editor may land on a preceding local
                    // control. Escape dismisses that visible unfinished edit,
                    // never a guessed departure widget or the underlying run.
                    self.editor = None;
                    self.editor_controls = [None; 3];
                } else {
                    self.expanded = false;
                }
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
        let enabled = !self.confirming()
            && prompt_ui::keyboard_available(ui)
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
        if let Some(notice) = self.storage_notice {
            ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
        }
        if self.expanded {
            prompt_ui::inline(&ctx).inner_margin(10).show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                ui.add(egui::Label::new(theme::small(ui, "Local checkpoint · saved chats only · restored anchors stay stale.")).wrap()).on_hover_text("Saved notes use this device’s normal app storage at the next app save/clean exit. Unfinished edits and pathless-chat notes are excluded. No automatic file reopening or reattachment; other windows are not synchronized.");
                ui.horizontal_wrapped(|ui| {
                    for (only,label) in [(false,"All"),(true,"Unresolved")] {
                        let response=ui.add_enabled(enabled,egui::Button::selectable(self.unresolved_only==only,theme::small(ui,label)));
                        self.controls.push(response.id);
                        if response.clicked(){self.unresolved_only=only;}
                        #[cfg(test)]ctx.data_mut(|data|data.insert_temp(egui::Id::new(("review-note-filter",only)),(response.id,response.rect)));
                    }
                    let unresolved=self.items.iter().filter(|item|Some(&item.anchor.owner)==self.owner.as_ref() && !item.resolved).count();
                    ui.label(theme::small(ui,format!("{unresolved} unresolved · {owned} total")));
                });
                if owned>0 {
                    let search=ui.add_enabled(enabled,egui::TextEdit::singleline(&mut self.query).id(navigation.focus_id().with("local-note-search")).char_limit(128).desired_width(ui.available_width()).hint_text("Filter note text or original path…"));
                    self.controls.push(search.id);
                    if search.changed(){self.query=self.query.chars().take(128).collect();self.query.shrink_to_fit();}
                    #[cfg(test)]ctx.data_mut(|data|data.insert_temp(egui::Id::new("review-note-search"),(search.id,search.rect)));
                }
                let editor_visible = self.editor.as_ref().is_some_and(|editor| Some(&editor.anchor.owner) == self.owner.as_ref());
                if editor_visible { ui.add_enabled_ui(enabled, |ui| self.editor_ui(ui, navigation)); }
                else if self.editor.is_some() {
                    ui.add(egui::Label::new(theme::small(ui, "An unfinished note belongs to another repository/session. Return there to finish it, or explicitly Clear all.")).wrap());
                }
                let mut delete = None;
                let mut edit = None;
                let mut jump = None;
                let reveal_note = if enabled {
                    self.marker_reveal(navigation,path,recorded,ctx.cumulative_pass_nr())
                } else {
                    self.reveal_note=None;
                    None
                };
                if reveal_note.is_some_and(|id| self.items.iter().any(|item|item.id==id && item.resolved)){self.unresolved_only=false;}
                if reveal_note.is_some(){self.query.clear();}
                let unresolved_only=self.unresolved_only;
                let query=self.query.trim();
                let mut visible=0usize;
                let source_root = self.source_root.as_ref();
                let mut open_recorded = None;
                let mut add_note=None;
                egui::ScrollArea::vertical().id_salt("local-review-notes").max_height(140.0).show(ui, |ui| {
                    for (_index, item) in self.items.iter_mut().enumerate().filter(|(_, item)| Some(&item.anchor.owner) == self.owner.as_ref() && (!unresolved_only || !item.resolved) && (query.is_empty() || item.text.contains(query) || item.anchor.path.contains(query))) {
                        visible+=1;
                        let note_row=ui.push_id(item.id, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                let current = item.anchor.path == path && item.anchor.recorded == recorded && Some(item.anchor.digest) == digest;
                                let label = format!("{}{}{}", if current { String::new() } else { format!("{} · ", item.anchor.path) }, item.anchor.selection.label(), if item.stale { " · STALE" } else { "" });
                                let can_jump = !item.stale && current && source_root == Some(&item.anchor.root);
                                let reveal = ui.add_enabled(enabled && can_jump, egui::Button::new(RichText::new(label).size(11.0).color(if can_jump {theme::palette(&ctx).accent} else {theme::palette(&ctx).dim})).frame(false).wrap())
                                    .on_hover_text(format!("Jump to start of original range · {}\n{}\nExact open diff only; does not select text or change the note.",item.anchor.source(),item.anchor.root.display()))
                                    .on_disabled_hover_text("Open the matching original diff snapshot. Stale notes never jump or reattach to similar text.");
                                self.controls.push(reveal.id);
                                if reveal.clicked() { jump = Some(item.id); }
                                if self.recorded_targets.iter().any(|(note,_)|*note==item.id) {
                                    let open=ui.push_id(("recorded-note-open",navigation.focus_id(),item.stale),|ui|ui.add_enabled(enabled,egui::Button::new(theme::small(ui,if item.stale{"Inspect original"}else{"Open"})).frame(false))).inner
                                        .on_hover_text("Inspect this note’s exact recorded snapshot and original range. Stale/restored notes stay STALE until separate Re-review confirmation. No disk read, fuzzy reattachment or send.");
                                    self.controls.push(open.id);
                                    if open.clicked() {open_recorded=Some(item.id);}
                                    #[cfg(test)]
                                    ctx.data_mut(|data|data.insert_temp(egui::Id::new(("review-note-open-recorded",item.id)),(open.id,open.rect)));
                                }
                                if reveal_note==Some(item.id) {ctx.memory_mut(|memory|memory.request_focus_with_filter(reveal.id,egui::EventFilter{escape:true,..Default::default()}));}
                                #[cfg(test)]
                                ctx.data_mut(|data| data.insert_temp(egui::Id::new(("review-comment-jump", item.id)), (reveal.id,reveal.rect)));
                                let action = ui.add_enabled(enabled, egui::Button::new(theme::small(ui, if item.resolved { "Reopen" } else { "Resolve" })).frame(false));
                                self.controls.push(action.id);
                                if self.after_editor.is_none() && action.enabled() { self.after_editor = Some(action.id); }
                                if action.clicked() { item.resolved = !item.resolved; self.storage_dirty = true; }
                                let quote=ui.push_id(("add-single-note",navigation.focus_id()),|ui|ui.add_enabled(enabled,egui::Button::new(theme::small(ui,"Add to draft")).frame(false))).inner.on_hover_text("Append only this committed note with original snapshot provenance and stale/resolved status. Never sends or changes the note; unfinished edits are excluded.");
                                self.controls.push(quote.id);
                                if quote.clicked(){add_note=Some(item.id);}
                                #[cfg(test)]ctx.data_mut(|data|data.insert_temp(egui::Id::new(("review-note-add-draft",item.id)),(quote.id,quote.rect)));
                                if item.stale&&recorded&&current&&source_root==Some(&item.anchor.root){
                                    let recheck=ui.push_id(("recheck-original",navigation.focus_id()),|ui|ui.add_enabled(enabled&&self.editor.is_none(),egui::Button::new(theme::small(ui,"Re-review…")).frame(false))).inner.on_hover_text("Explicitly re-review this exact original recorded snapshot after confirmation. No fuzzy reattachment or working-tree verification.");
                                    self.controls.push(recheck.id);
                                    if recheck.clicked(){self.recheck_requested=Some((item.id,navigation.focus_id()));}
                                    #[cfg(test)]ctx.data_mut(|d|d.insert_temp(egui::Id::new(("review-note-recheck",item.id)),(recheck.id,recheck.rect)));
                                }
                                let modify = ui.add_enabled(enabled && self.editor.is_none(), egui::Button::new(theme::small(ui,"Edit")).frame(false))
                                    .on_hover_text("Edit this local note's text only. Original anchor and status stay unchanged until Save.");
                                self.controls.push(modify.id);
                                if modify.clicked() {edit=Some(item.id);}
                                #[cfg(test)]
                                ctx.data_mut(|data|data.insert_temp(egui::Id::new(("review-comment-edit",_index)),modify.rect));
                                let remove = ui.add_enabled(enabled, egui::Button::new(theme::small(ui,"Delete")).frame(false))
                                    .on_hover_text("Delete only this local note after confirmation. Draft context is unchanged.");
                                self.controls.push(remove.id);
                                if remove.clicked() {delete=Some(item.id);}
                                #[cfg(test)]
                                ctx.data_mut(|data|data.insert_temp(egui::Id::new(("review-comment-delete",_index)),remove.rect));
                                #[cfg(test)]
                                ctx.data_mut(|data| data.insert_temp(egui::Id::new(("review-comment-resolve", _index)), action.rect));
                            });
                            let text=ui.add(egui::Label::new(RichText::new(&item.text).size(12.0).color(if item.resolved { theme::palette(&ctx).dim } else { theme::palette(&ctx).text })).wrap());
                            #[cfg(test)]
                            ctx.data_mut(|data|data.insert_temp(egui::Id::new(("review-note-body",item.id)),(text.rect,ui.clip_rect())));
                            let _=text;
                        });
                        if reveal_note==Some(item.id) {
                            note_row.response.scroll_to_me(Some(Align::Center));
                        }
                    }
                });
                if !query.is_empty(){ui.label(theme::small(ui,format!("{visible} matching notes · literal, case-sensitive")));}
                if self.unresolved_only && !self.items.iter().any(|item|Some(&item.anchor.owner)==self.owner.as_ref() && !item.resolved) {
                    ui.label(theme::small(ui,"No unresolved notes. Resolved notes remain in All."));
                }
                if let Some(id)=add_note {
                    match self.one_note(id){
                        Ok(text)=>if let Some(owner)=&self.owner {context_capture::queue_review(&ctx,&owner.root,&owner.session,"Local review note (original snapshot)",&text);},
                        Err(error)=>self.error=Some(error),
                    }
                }
                if let Some(id)=open_recorded {self.request_recorded_open(id,navigation.focus_id(),ctx.cumulative_pass_nr());}
                if let Some(id)=delete {self.request_delete(id);}
                if let Some(id)=edit {self.begin_edit(id);}
                if let Some(id)=jump {self.jump(id,&ctx,path,diff,recorded,navigation);}
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
                    let clear = ui.add_enabled(enabled && !self.confirming() && (!self.items.is_empty() || self.editor.is_some()), egui::Button::new(theme::small(ui, "Clear all")).frame(false));
                    self.controls.push(clear.id);
                    if self.after_editor.is_none() && clear.enabled() { self.after_editor = Some(clear.id); }
                    if clear.clicked() && !self.confirming() { self.clear_requested = true; }
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
        self.recheck_dialog(&ctx, path, diff, recorded, navigation);
        self.deletion_dialog(&ctx, navigation);
        if self.clear_requested {
            match confirmations::Dialog::new("clear-local-review-comments", "Clear local review comments?", "Removes all local notes and unfinished comment text in this window, including other sessions. The next normal app save updates the local app checkpoint. Other windows are not synchronized. Draft context already added is unchanged.", "Cancel", "Clear all").show(&ctx, |_| {}) {
                Some(confirmations::Choice::Confirm) => { self.items.clear(); self.editor = None; self.editor_controls = [None;3]; self.error = None; self.clear_requested = false; self.storage_dirty = true; }
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
            if editor.editing.is_some() {
                "Edit local comment"
            } else {
                "Local comment"
            },
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
        if editor.text.capacity() > MAX_TEXT {
            editor.text.shrink_to_fit();
        }
        prompt_ui::footer(ui, |ui| {
            let add = prompt_ui::button(
                ui,
                if editor.editing.is_some() {
                    "Save changes"
                } else {
                    "Add comment"
                },
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
                memory.set_focus_lock_filter_current(
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
