//! Explicit reviewed abort of an in-progress Git merge, rebase, cherry-pick, or revert.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Operation {
    owner: Option<(PathBuf, String)>,
    pending: Option<workspace::read_only::Request<Result<workspace::operation::Review, String>>>,
    review: Option<workspace::operation::Review>,
    applying: Option<workspace::read_only::Request<Result<(), String>>>,
    notice: Option<String>,
    controls: Vec<egui::Id>,
}

impl Changes {
    pub(super) fn operation_has_status(&self, workspace: &Workspace) -> bool {
        let s = &self.operation;
        s.pending.is_some()
            || s.review.is_some()
            || s.applying.is_some()
            || s.notice.is_some()
            || workspace.files.iter().any(|f| {
                matches!(
                    f.status.as_str(),
                    "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU"
                )
            })
    }

    pub(super) fn trigger_operation_abort_review(
        &mut self,
        ctx: &egui::Context,
        workspace: &Workspace,
    ) {
        if !self.index_writable || !workspace.is_git {
            return;
        }
        let s = &mut self.operation;
        if s.pending.is_some() || s.applying.is_some() || s.review.is_some() {
            return;
        }
        let Some(permit) = branches::permit(ctx) else {
            s.notice = Some("Git busy; retry explicitly.".into());
            return;
        };
        let root = workspace.root.clone();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        s.owner = Some(self.identity.clone());
        s.notice = None;
        s.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        if std::thread::Builder::new()
            .name("pui-operation-abort-review".into())
            .spawn(move || {
                let result = workspace::operation::inspect(&root, &cancel);
                drop(permit);
                if !cancel.load(Ordering::Acquire) {
                    let _ = tx.send(result);
                    ctx.request_repaint();
                }
            })
            .is_err()
        {
            s.pending = None;
            s.notice = Some("Cannot inspect Git operation.".into());
        }
    }

    pub(super) fn operation_bar(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        loading: bool,
    ) {
        let s = &mut self.operation;
        s.controls.clear();
        let enabled = self.index_writable
            && workspace.is_git
            && !loading
            && workspace.error.is_none()
            && s.pending.is_none()
            && s.applying.is_none()
            && s.review.is_none()
            && !egui::Popup::is_any_open(ui.ctx())
            && ui.memory(|m| m.top_modal_layer().is_none());
        let mut clicked = false;
        ui.horizontal_wrapped(|ui| {
            let btn = prompt_ui::button(
                ui,
                "Review abort operation…",
                prompt_ui::Button::Secondary,
                enabled,
            );
            s.controls.push(btn.id);
            #[cfg(test)]
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    egui::Id::new("operation-abort-review-control"),
                    (btn.id, btn.rect, btn.enabled()),
                )
            });
            clicked = btn.clicked();
            if s.pending.is_some() {
                ui.label(theme::small(ui, "Inspecting active Git operation…"));
            }
            if s.applying.is_some() {
                ui.label(theme::small(ui, "Aborting operation…"));
            }
        });
        if clicked {
            self.trigger_operation_abort_review(ui.ctx(), workspace);
        }
        if let Some(notice) = &self.operation.notice {
            ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
        }
    }

    pub(super) fn operation_inputs(&mut self, _ctx: &egui::Context, visible: bool) {
        let s = &mut self.operation;
        if !visible || !self.index_writable || s.owner.as_ref().is_some_and(|o| o != &self.identity)
        {
            let active = s.applying.is_some();
            if !active {
                *s = Default::default();
            } else {
                s.pending = None;
                s.review = None;
                s.controls.clear();
            }
            return;
        }
        if let Some(result) = s.pending.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Operation inspection ended.".into())),
        }) {
            s.pending = None;
            match result {
                Ok(review) => {
                    s.review = Some(review);
                    s.notice = None;
                }
                Err(e) => s.notice = Some(e),
            }
        }
        if let Some(result) = s.applying.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(
                "Operation abort ended; inspect repository state.".into()
            )),
        }) {
            s.applying = None;
            match result {
                Ok(()) => {
                    s.notice = Some(
                        "Operation aborted. Working tree and index restored to pre-operation state."
                            .into(),
                    );
                }
                Err(e) => s.notice = Some(e),
            }
            self.refresh = true;
            self.invalidate();
        }
    }

    pub(super) fn operation_dialog(&mut self, ctx: &egui::Context) {
        let Some(review) = self.operation.review.clone() else {
            return;
        };
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("git-operation-abort-confirm"),
        );
        if !self.index_writable
            || self.operation.owner.as_ref() != Some(&self.identity)
            || egui::Popup::is_any_open(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
            || prompt_ui::modal_owns_current_pass(ctx)
        {
            self.operation.review = None;
            return;
        }
        let choice = confirmations::Dialog::new(
            "git-operation-abort-confirm",
            review.kind.abort_title(),
            "Aborts the in-progress Git operation and restores HEAD, index and tracked working files to their pre-operation state. Conflicted or staged changes from this operation are discarded. Hooks are bypassed.",
            "Cancel",
            "Abort operation",
        )
        .destructive(true)
        .show(ctx, |ui| {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    format!(
                        "{}
Operation: {} · Branch: {}
HEAD: {}",
                        review.root.display(),
                        review.kind.label(),
                        review.branch,
                        review.head
                    ),
                ))
                .wrap(),
            );
            if let Some(target) = &review.target {
                ui.add(
                    egui::Label::new(theme::small(ui, format!("Target: {target}"))).wrap(),
                );
            }
            if !review.unmerged_files.is_empty() {
                ui.label(theme::small(
                    ui,
                    format!("Unmerged files ({}):", review.unmerged_files.len()),
                ));
                egui::ScrollArea::vertical()
                    .id_salt("operation-abort-unmerged")
                    .max_height(120.0)
                    .show(ui, |ui| {
                        for path in &review.unmerged_files {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(path.escape_debug().to_string())
                                        .monospace()
                                        .size(11.0),
                                )
                                .wrap(),
                            );
                        }
                    });
            }
        });
        if let Some(choice) = choice {
            self.operation.review = None;
            if choice == confirmations::Choice::Confirm {
                let Some(permit) = branches::permit(ctx) else {
                    self.operation.notice = Some("Git busy; retry explicitly.".into());
                    return;
                };
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.operation.applying =
                    Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-operation-abort-apply".into())
                    .spawn(move || {
                        let result = workspace::operation::abort(review, &cancel);
                        drop(permit);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    })
                    .is_err()
                {
                    self.operation.applying = None;
                    self.operation.notice = Some("Cannot start abort worker.".into());
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
