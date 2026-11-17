//! Local create-only branch form; transient intent, no checkout.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
pub(super) struct Create {
    owner: Option<(PathBuf, String)>,
    name: String,
    switch: bool,
    start: String,
    reveal: bool,
    pending:
        Option<workspace::read_only::Request<Result<workspace::branches::create::Review, String>>>,
    review: Option<workspace::branches::create::Review>,
    applying: Option<workspace::read_only::Request<Result<(), String>>>,
    notice: Option<String>,
    controls: Vec<egui::Id>,
}
impl Changes {
    pub(super) fn branch_draft_available(&self) -> bool {
        self.index_writable
            && self.branch_create.pending.is_none()
            && self.branch_create.applying.is_none()
            && self.branch_create.review.is_none()
    }
    pub(super) fn prepare_branch_switch(&mut self, name: String) -> bool {
        if !self.branch_draft_available() || !workspace::branches::create::valid_name(&name) {
            return false;
        }
        self.branch_create=Create{owner:Some(self.identity.clone()),name,switch:true,reveal:true,notice:Some("Highlighted branch copied into draft. Review the switch to inspect clean state and changed paths. Nothing switched.".into()),..Default::default()};
        true
    }
    pub(super) fn prepare_history_branch(&mut self, hash: String) -> bool {
        if !self.branch_draft_available() || !workspace::branches::create::valid_start(&hash) {
            return false;
        }
        self.branch_create=Create{owner:Some(self.identity.clone()),start:hash,reveal:true,notice:Some("History commit copied into draft. Enter a new branch name, then review. No branch created.".into()),..Default::default()};
        true
    }
    pub(super) fn create_branch_ui(&mut self, ui: &mut egui::Ui, workspace: &Workspace) {
        let state = &mut self.branch_create;
        state.controls.clear();
        let open = std::mem::take(&mut state.reveal).then_some(true);
        egui::CollapsingHeader::new("Create or switch local branch…")
            .open(open)
            .show(ui, |ui| {
                let enabled = self.index_writable
                    && workspace.is_git
                    && state.pending.is_none()
                    && state.applying.is_none()
                    && state.review.is_none()
                    && !egui::Popup::is_any_open(ui.ctx())
                    && ui.memory(|m| m.top_modal_layer().is_none());
                ui.add_enabled_ui(enabled, |ui| {
                    let toggle = ui.checkbox(
                        &mut state.switch,
                        "Switch to an existing branch (clean repository only)",
                    );
                    state.controls.push(toggle.id);
                    let input = ui.add(
                        egui::TextEdit::singleline(&mut state.name)
                            .hint_text(if state.switch {
                                "Existing branch name…"
                            } else {
                                "New branch name…"
                            })
                            .char_limit(1000)
                            .desired_width(ui.available_width()),
                    );
                    state.controls.push(input.id);
                    if !state.switch {
                        ui.label(theme::small(
                            ui,
                            "Start commit (optional full ID; blank uses current HEAD)",
                        ));
                        let start = ui.add(
                            egui::TextEdit::singleline(&mut state.start)
                                .char_limit(64)
                                .desired_width(ui.available_width()),
                        );
                        state.controls.push(start.id);
                    }
                    let button = prompt_ui::button(
                        ui,
                        if state.switch {
                            "Review branch switch…"
                        } else {
                            "Review branch creation…"
                        },
                        prompt_ui::Button::Secondary,
                        workspace::branches::create::valid_name(&state.name)
                            && (state.switch
                                || state.start.is_empty()
                                || workspace::branches::create::valid_start(&state.start)),
                    );
                    state.controls.push(button.id);
                    if button.clicked() {
                        let Some(permit) = branches::permit(ui.ctx()) else {
                            state.notice = Some("Git busy; retry explicitly.".into());
                            return;
                        };
                        let root = workspace.root.clone();
                        let name = state.name.clone();
                        let switching = state.switch;
                        let start = state.start.clone();
                        let (tx, rx) = mpsc::channel();
                        let cancel = Arc::new(AtomicBool::new(false));
                        state.owner = Some(self.identity.clone());
                        state.pending =
                            Some(workspace::read_only::Request::new(rx, cancel.clone()));
                        let ctx = ui.ctx().clone();
                        if std::thread::Builder::new()
                            .name("pui-branch-create-review".into())
                            .spawn(move || {
                                let result = if switching {
                                    workspace::branches::create::inspect_action(
                                        &root, &name, true, &cancel,
                                    )
                                } else if !start.is_empty() {
                                    workspace::branches::create::inspect_start(
                                        &root, &name, &start, &cancel,
                                    )
                                } else {
                                    workspace::branches::create::inspect(&root, &name, &cancel)
                                };
                                drop(permit);
                                if !cancel.load(Ordering::Acquire) {
                                    let _ = tx.send(result);
                                    ctx.request_repaint();
                                }
                            })
                            .is_err()
                        {
                            state.pending = None;
                            state.notice = Some("Cannot inspect branch creation.".into());
                        }
                    }
                });
                if let Some(notice) = &state.notice {
                    ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
                }
            });
    }
    pub(super) fn create_branch_inputs(&mut self, ctx: &egui::Context, visible: bool) {
        let s = &mut self.branch_create;
        if !visible || !self.index_writable || s.owner.as_ref().is_some_and(|o| o != &self.identity)
        {
            *s = Default::default();
            return;
        }
        if let Some(result) = s.pending.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Branch inspection ended.".into())),
        }) {
            s.pending = None;
            match result {
                Ok(review) => s.review = Some(review),
                Err(e) => s.notice = Some(e),
            }
        }
        if let Some(result) = s.applying.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(
                "Branch completion uncertain; inspect before retry.".into()
            )),
        }) {
            s.applying = None;
            s.notice=Some(match result{Ok(())=>"Branch operation completed. Refresh/inspect current branch before continuing. No push.".into(),Err(e)=>e});
            self.refresh = true;
            self.inline.clear();
            self.diffs.clear();
            self.git_files.stale = true;
        }
        let focus = ctx.memory(|m| m.focused());
        if s.review.is_some() || focus.is_some_and(|id| s.controls.contains(&id)) {
            ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        }
    }
    pub(super) fn create_branch_dialog(&mut self, ctx: &egui::Context) {
        let Some(review) = &self.branch_create.review else {
            return;
        };
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("create-local-branch"),
        );
        if !self.index_writable
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.branch_create.review = None;
            return;
        }
        let title = if review.switch {
            "Switch local branch?"
        } else {
            "Create local branch?"
        };
        let description = if review.switch {
            "Switches the current branch, index and tracked working files to the reviewed target. Requires completely clean state with no untracked or ignored files. No force, merge, stash or push. Configured checkout filters and submodule trees refuse. Checkout hooks are bypassed. External writes may race; inspect state after any failure."
        } else {
            "Creates only a new local branch at this exact commit. Does not switch branches, modify index/working files, set upstream or push. Reference hooks are bypassed. Existing names are never replaced."
        };
        let choice = confirmations::Dialog::new(
            "create-local-branch",
            title,
            description,
            "Cancel",
            if review.switch {
                "Switch branch"
            } else {
                "Create branch"
            },
        )
        .destructive(false)
        .show(ctx, |ui| {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    format!(
                        "{}\n{}\n{}",
                        review.root.display(),
                        review.name,
                        review.target
                    ),
                ))
                .wrap(),
            );
            if review.switch {
                ui.label(theme::small(
                    ui,
                    format!(
                        "{} changed paths · source → target · renames as delete/add",
                        review.files.len()
                    ),
                ));
                egui::ScrollArea::both()
                    .id_salt("switch-changed-files")
                    .max_height(160.0)
                    .show_rows(ui, 16.0, review.files.len(), |ui, range| {
                        for entry in &review.files[range] {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!(
                                        "{}  {}",
                                        entry.status,
                                        entry.path.escape_debug()
                                    ))
                                    .monospace()
                                    .size(11.0),
                                )
                                .extend(),
                            );
                        }
                    });
                if review.files.is_empty() {
                    ui.label(theme::small(
                        ui,
                        "No tracked content differences between these commits.",
                    ));
                }
            }
        });
        if let Some(choice) = choice {
            let review = self.branch_create.review.take().unwrap();
            if choice == confirmations::Choice::Confirm {
                let Some(permit) = branches::permit(ctx) else {
                    self.branch_create.notice = Some("Git busy; inspect again.".into());
                    return;
                };
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.branch_create.applying =
                    Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-create-branch".into())
                    .spawn(move || {
                        let result = workspace::branches::create::apply(review, &cancel);
                        drop(permit);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    })
                    .is_err()
                {
                    self.branch_create.applying = None;
                    self.branch_create.notice = Some("Cannot start branch creation.".into());
                }
            }
        }
        ctx.input_mut(|i| {
            i.events.retain(|e| {
                !matches!(
                    e,
                    egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Paste(_)
                )
            })
        });
    }
}
#[cfg(test)]
mod tests;
