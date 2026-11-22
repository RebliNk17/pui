//! Two explicit actions: save working text with backup; freshly review/stage as resolved.
#[cfg(test)]
mod tests;
use super::*;
use workspace::index_action::conflict_resolution as backend;
#[derive(Default)]
pub(super) struct Resolution {
    pending: Option<workspace::read_only::Request<Result<backend::Review, String>>>,
    stage_intent: bool,
    editor: Option<(backend::Review, String)>,
    inspected: Option<backend::Review>,
    applying: Option<workspace::read_only::Request<Result<String, String>>>,
    controls: Vec<egui::Id>,
    suppress_release: bool,
}
impl Resolution {
    pub(super) fn begin_rows(&mut self) {
        self.controls.clear();
    }
    pub(super) fn busy(&self) -> bool {
        self.pending.is_some()
            || self.editor.is_some()
            || self.inspected.is_some()
            || self.applying.is_some()
    }
    pub(super) fn row(
        &mut self,
        ui: &mut egui::Ui,
        root: &std::path::Path,
        path: &str,
        enabled: bool,
        notice: &mut Option<String>,
    ) -> bool {
        let mut started = false;
        for (label, stage) in [
            ("Edit working resolution…", false),
            ("Review mark resolved…", true),
        ] {
            let response = prompt_ui::button(
                ui,
                label,
                prompt_ui::Button::Secondary,
                enabled && !self.busy(),
            );
            self.controls.push(response.id);
            #[cfg(test)]
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    egui::Id::new(("conflict-resolution-row", stage, path)),
                    (response.id, response.rect, response.enabled()),
                )
            });
            if response.clicked() {
                let Some(permit) = branches::permit(ui.ctx()) else {
                    *notice = Some("Git busy; retry explicitly.".into());
                    continue;
                };
                let root = root.to_path_buf();
                let path = path.to_owned();
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
                self.stage_intent = stage;
                let ctx = ui.ctx().clone();
                started = true;
                if std::thread::Builder::new()
                    .name("pui-resolution-review".into())
                    .spawn(move || {
                        let result = backend::inspect(&root, &path, &cancel);
                        drop(permit);
                        if !cancel.load(Ordering::Acquire) {
                            let _ = tx.send(result);
                            ctx.request_repaint();
                        }
                    })
                    .is_err()
                {
                    self.pending = None;
                    *notice = Some("Cannot inspect working resolution.".into());
                }
            }
        }
        if self.applying.is_some() {
            ui.label(theme::small(ui, "Applying reviewed resolution…"));
            if prompt_ui::button(
                ui,
                "Cancel resolution",
                prompt_ui::Button::Secondary,
                enabled,
            )
            .clicked()
            {
                self.applying = None;
                *notice=Some("Cancellation requested. File save or staging may already have completed; inspect before retrying.".into());
            }
        }
        if started {
            self.suppress_release = true;
        }
        let focus = ui.memory(|m| m.focused());
        if started || self.controls.iter().any(|id| focus == Some(*id)) {
            Self::suppress(ui.ctx());
        }
        started
    }
    pub(super) fn inputs(&mut self, ctx: &egui::Context, notice: &mut Option<String>) -> bool {
        let mut refresh = false;
        if let Some(result) = self.pending.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Resolution inspection ended.".into())),
        }) {
            self.pending = None;
            match result {
                Ok(review) => {
                    if self.stage_intent {
                        self.inspected = Some(review);
                    } else {
                        let draft = review.text.clone();
                        self.editor = Some((review, draft));
                    }
                }
                Err(e) => *notice = Some(e),
            }
        }
        if let Some(result) = self.applying.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(
                "Resolution completion uncertain; inspect file/index before retrying.".into(),
            )),
        }) {
            self.applying = None;
            *notice = Some(match result {
                Ok(s) => s,
                Err(e) => e,
            });
            refresh = true;
        }
        let focus = ctx.memory(|m| m.focused());
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let owns = self
            .controls
            .iter()
            .any(|id| focus == Some(*id) || escape && ctx.memory(|m| m.focus_lost_by_escape(*id)));
        if owns || self.editor.is_some() || self.inspected.is_some() || self.suppress_release {
            // Suppress global chords before later chat/approval handling, but leave editor text intact.
            ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        }
        if self.suppress_release {
            Self::suppress(ctx);
            if !ctx.input(|i| {
                i.key_down(egui::Key::Enter)
                    || i.key_down(egui::Key::Space)
                    || i.key_down(egui::Key::Escape)
            }) {
                self.suppress_release = false;
            }
        }
        refresh
    }
    fn suppress(ctx: &egui::Context) {
        ctx.input_mut(|i| {
            i.events.retain(|e| {
                !matches!(
                    e,
                    egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Paste(_)
                )
            })
        });
    }
    pub(super) fn dialog(
        &mut self,
        ctx: &egui::Context,
        enabled: bool,
        owner_ok: bool,
        notice: &mut Option<String>,
    ) {
        if self.editor.is_none() && self.inspected.is_none() {
            return;
        }
        let id = if self.editor.is_some() {
            "git-conflict-edit"
        } else {
            "git-conflict-resolve"
        };
        let own = egui::LayerId::new(egui::Order::Foreground, egui::Id::new(id));
        if !enabled
            || !owner_ok
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.editor = None;
            self.inspected = None;
            Self::suppress(ctx);
            self.suppress_release = true;
            return;
        }
        let choice = if let Some((review, draft)) = &mut self.editor {
            let valid = backend::valid_resolution(draft) && *draft != review.text;
            confirmations::Dialog::new(id,"Save working resolution?","Edits only this working file. A recovery copy retains the overwritten text beside the Git index. No staging, commit, merge/rebase continue or abort. Remove marker lines; saving is not proof that the resolution is correct. External writers can race; cancellation has no rollback guarantee.","Cancel","Save working file").enabled(valid).width(520.0).show(ctx,|ui|{
                ui.add(egui::Label::new(theme::small(ui,format!("{}\n{} · mode {}",review.root.display(),review.path.escape_debug(),review.mode))).wrap());
                ui.label(theme::small(ui,"Edit UTF-8 text (64 KiB / 2048 lines). Empty text is an empty file, not deletion."));
                let response=ui.add(egui::TextEdit::multiline(draft).id(egui::Id::new((id,&review.root,&review.path))).code_editor().desired_rows(8).desired_width(ui.available_width()).char_limit(64*1024+1));
                if response.has_focus(){ctx.memory_mut(|m|m.set_focus_lock_filter_current(response.id,egui::EventFilter{escape:true,vertical_arrows:true,horizontal_arrows:true,..Default::default()}));}
                if !backend::valid_resolution(draft){prompt_ui::note(ui,"Marker-like lines, unsupported controls or text bounds prevent saving. No partial save.");}
                #[cfg(test)]ui.ctx().data_mut(|d|d.insert_temp(egui::Id::new("conflict-editor-text"),response.id));
            })
        } else {
            let review = self.inspected.as_ref().unwrap();
            confirmations::Dialog::new(id,"Mark this file resolved?","Replaces this file’s unmerged index stages with exactly the displayed working text and mode. Working file stays unchanged; other conflicts remain. No commit or merge/rebase continue. Marker checks are conservative—not semantic verification. Re-review after any change.","Cancel","Mark resolved").destructive(false).enabled(backend::valid_resolution(&review.text)).width(520.0).show(ctx,|ui|{
                ui.add(egui::Label::new(theme::small(ui,format!("{}\n{} · mode {}\n{} UTF-8 bytes",review.root.display(),review.path.escape_debug(),review.mode,review.text.len()))).wrap());
                ui.label(theme::small(ui,if review.text.is_empty(){"Empty file (zero bytes)."}else if review.text.ends_with('\n'){"Ends with a newline."}else{"No final newline."}));
                if !backend::valid_resolution(&review.text){prompt_ui::note(ui,"Marker-like lines remain. Edit the working file first; marking resolved is disabled.");}
                egui::ScrollArea::both().max_height(180.0).show_rows(ui,16.0,review.text.split('\n').count(),|ui,range|{for line in review.text.split('\n').skip(range.start).take(range.len()){ui.add(egui::Label::new(RichText::new(line).monospace().size(10.5)).extend().selectable(false));}});
            })
        };
        if let Some(choice) = choice {
            let edit = self.editor.take();
            let stage = self.inspected.take();
            self.suppress_release = true;
            if choice == confirmations::Choice::Confirm {
                let Some(permit) = branches::permit(ctx) else {
                    *notice = Some("Git busy; review again.".into());
                    Self::suppress(ctx);
                    return;
                };
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.applying = Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new().name("pui-conflict-resolution".into()).spawn(move||{
                    let result=if let Some((review,draft))=edit{backend::save(review,draft,&cancel).map(|p|format!("Working resolution saved. Recovery copy: {}. Still unresolved in index; review Mark resolved separately.",p.display()))}else{backend::stage(stage.unwrap(),&cancel).map(|()|"File marked resolved in index. No commit or continue performed; inspect remaining conflicts.".into())};drop(permit);let _=tx.send(result);ctx.request_repaint();
                }).is_err(){self.applying=None;*notice=Some("Cannot start resolution action.".into());}
            }
        }
        Self::suppress(ctx);
    }
}
