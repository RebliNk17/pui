//! Explicit reviewed Git stash UI: create (tracked changes only), list, apply and drop.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(test)]
mod tests;

enum PendingReview {
    Push(workspace::stash::PushReview),
    Entry(workspace::stash::EntryReview),
}

#[derive(Default)]
pub(super) struct Stash {
    owner: Option<(PathBuf, String)>,
    message: String,
    pub(super) loaded: bool,
    entries: Vec<workspace::stash::Entry>,
    list_rx: Option<workspace::read_only::Request<Result<Vec<workspace::stash::Entry>, String>>>,
    review_rx: Option<workspace::read_only::Request<Result<PendingReview, String>>>,
    review: Option<PendingReview>,
    applying: Option<workspace::read_only::Request<Result<String, String>>>,
    notice: Option<String>,
    controls: Vec<egui::Id>,
}

impl Changes {
    pub(super) fn refresh_stashes(&mut self, ctx: &egui::Context, root: PathBuf) {
        let s = &mut self.stash;
        let Some(permit) = branches::permit(ctx) else {
            s.notice = Some("Git busy; retry explicitly.".into());
            return;
        };
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        s.owner = Some(self.identity.clone());
        s.loaded = true;
        s.list_rx = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        if std::thread::Builder::new()
            .name("pui-stash-list".into())
            .spawn(move || {
                let result = workspace::stash::list(&root, &cancel);
                drop(permit);
                if !cancel.load(Ordering::Acquire) {
                    let _ = tx.send(result);
                    ctx.request_repaint();
                }
            })
            .is_err()
        {
            s.list_rx = None;
            s.notice = Some("Cannot list Git stashes.".into());
        }
    }

    pub(super) fn stash_ui(&mut self, ui: &mut egui::Ui, workspace: &Workspace) {
        if !workspace.is_git {
            return;
        }
        if self.repo_sheet == Some(RepoSheet::Stash)
            && !self.stash.loaded
            && self.stash.list_rx.is_none()
        {
            self.refresh_stashes(ui.ctx(), workspace.root.clone());
        }
        let modal_ok = self.repo_sheet.is_some() || ui.memory(|m| m.top_modal_layer().is_none());
        let busy = self.stash.list_rx.is_some()
            || self.stash.review_rx.is_some()
            || self.stash.review.is_some()
            || self.stash.applying.is_some();
        let enabled =
            self.index_writable && !busy && !egui::Popup::is_any_open(ui.ctx()) && modal_ok;
        let width = ui.available_width();
        self.stash.controls.clear();

        Self::qm_field_label(
            ui,
            "Stash tracked changes",
            Some("Untracked files stay in place"),
        );
        ui.add_space(4.0);
        let msg_resp = Self::qm_text_input(
            ui,
            &mut self.stash.message,
            "Optional stash note (up to 256 bytes)",
            256,
            false,
            width,
        );
        self.stash.controls.push(msg_resp.id);
        ui.add_space(6.0);
        let valid_msg = workspace::stash::valid_message(&self.stash.message);
        ui.horizontal_wrapped(|ui| {
            let push_btn = prompt_ui::button(
                ui,
                "Review stash push…",
                prompt_ui::Button::Primary,
                enabled && valid_msg,
            );
            self.stash.controls.push(push_btn.id);
            if push_btn.clicked() {
                let Some(permit) = branches::permit(ui.ctx()) else {
                    self.stash.notice = Some("Git busy; retry explicitly.".into());
                    return;
                };
                let root = workspace.root.clone();
                let message = self.stash.message.clone();
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.stash.owner = Some(self.identity.clone());
                self.stash.notice = None;
                self.stash.review_rx = Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ui.ctx().clone();
                if std::thread::Builder::new()
                    .name("pui-stash-push-review".into())
                    .spawn(move || {
                        let result = workspace::stash::inspect_push(&root, &message, &cancel)
                            .map(PendingReview::Push);
                        drop(permit);
                        if !cancel.load(Ordering::Acquire) {
                            let _ = tx.send(result);
                            ctx.request_repaint();
                        }
                    })
                    .is_err()
                {
                    self.stash.review_rx = None;
                    self.stash.notice = Some("Cannot inspect stash push.".into());
                }
            }
            let refresh_btn = prompt_ui::button(
                ui,
                "Refresh stashes",
                prompt_ui::Button::Secondary,
                !busy && modal_ok,
            );
            self.stash.controls.push(refresh_btn.id);
            if refresh_btn.clicked() {
                self.refresh_stashes(ui.ctx(), workspace.root.clone());
            }
        });

        Self::qm_divider(ui);
        Self::qm_field_label(
            ui,
            "Saved stashes",
            Some("Pinned by exact commit ID · Apply requires clean tree"),
        );
        ui.add_space(4.0);
        if self.stash.list_rx.is_some() {
            ui.label(theme::small(ui, "Loading stashes…"));
        } else if self.stash.entries.is_empty() {
            ui.label(theme::small(ui, "No saved stashes in this repository."));
        } else {
            let colors = theme::palette(ui.ctx());
            let entries = self.stash.entries.clone();
            let mut request_entry: Option<(usize, String, workspace::stash::EntryAction)> = None;
            egui::ScrollArea::vertical()
                .id_salt("changes-stash-entries")
                .max_height(200.0)
                .show(ui, |ui| {
                    for entry in entries {
                        egui::Frame::new()
                            .fill(colors.sidebar)
                            .stroke(Stroke::new(1.0, colors.border))
                            .corner_radius(6)
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "{} · {}",
                                        entry.selector,
                                        &entry.oid[..entry.oid.len().min(10)]
                                    ))
                                    .monospace()
                                    .size(11.0)
                                    .strong(),
                                );
                                ui.add(egui::Label::new(theme::small(ui, &entry.summary)).wrap());
                                ui.horizontal_wrapped(|ui| {
                                    let apply_btn = prompt_ui::button(
                                        ui,
                                        "Review apply…",
                                        prompt_ui::Button::Secondary,
                                        enabled,
                                    );
                                    self.stash.controls.push(apply_btn.id);
                                    if apply_btn.clicked() {
                                        request_entry = Some((
                                            entry.index,
                                            entry.oid.clone(),
                                            workspace::stash::EntryAction::Apply,
                                        ));
                                    }
                                    let drop_btn = prompt_ui::button(
                                        ui,
                                        "Review drop…",
                                        prompt_ui::Button::Secondary,
                                        enabled,
                                    );
                                    self.stash.controls.push(drop_btn.id);
                                    if drop_btn.clicked() {
                                        request_entry = Some((
                                            entry.index,
                                            entry.oid.clone(),
                                            workspace::stash::EntryAction::Drop,
                                        ));
                                    }
                                });
                            });
                        ui.add_space(4.0);
                    }
                });
            if let Some((index, oid, action)) = request_entry {
                let Some(permit) = branches::permit(ui.ctx()) else {
                    self.stash.notice = Some("Git busy; retry explicitly.".into());
                    return;
                };
                let root = workspace.root.clone();
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.stash.owner = Some(self.identity.clone());
                self.stash.notice = None;
                self.stash.review_rx = Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ui.ctx().clone();
                if std::thread::Builder::new()
                    .name("pui-stash-entry-review".into())
                    .spawn(move || {
                        let result =
                            workspace::stash::inspect_entry(&root, index, &oid, action, &cancel)
                                .map(PendingReview::Entry);
                        drop(permit);
                        if !cancel.load(Ordering::Acquire) {
                            let _ = tx.send(result);
                            ctx.request_repaint();
                        }
                    })
                    .is_err()
                {
                    self.stash.review_rx = None;
                    self.stash.notice = Some("Cannot inspect stash entry.".into());
                }
            }
        }
        if let Some(notice) = &self.stash.notice {
            ui.add_space(4.0);
            ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
        }
    }

    pub(super) fn stash_inputs(&mut self, _ctx: &egui::Context, visible: bool) {
        let s = &mut self.stash;
        if !visible || !self.index_writable || s.owner.as_ref().is_some_and(|o| o != &self.identity)
        {
            let active = s.applying.is_some();
            if !active {
                *s = Default::default();
            } else {
                s.list_rx = None;
                s.review_rx = None;
                s.review = None;
                s.controls.clear();
            }
            return;
        }
        if let Some(result) = s.list_rx.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Stash list worker ended.".into())),
        }) {
            s.list_rx = None;
            match result {
                Ok(entries) => s.entries = entries,
                Err(e) => s.notice = Some(e),
            }
        }
        if let Some(result) = s.review_rx.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Stash review worker ended.".into())),
        }) {
            s.review_rx = None;
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
                "Stash operation ended; inspect status and stashes.".into()
            )),
        }) {
            s.applying = None;
            match result {
                Ok(msg) => {
                    s.notice = Some(msg);
                    s.message.clear();
                    s.loaded = false;
                }
                Err(e) => s.notice = Some(e),
            }
            self.refresh = true;
            self.invalidate();
        }
    }

    pub(super) fn stash_dialog(&mut self, ctx: &egui::Context) {
        let Some(review) = &self.stash.review else {
            return;
        };
        let own = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("git-stash-confirm"));
        if !self.index_writable
            || self.stash.owner.as_ref() != Some(&self.identity)
            || egui::Popup::is_any_open(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
            || prompt_ui::modal_owns_current_pass(ctx)
        {
            self.stash.review = None;
            return;
        }
        let (title, desc, confirm_label, destructive) = match review {
            PendingReview::Push(_) => (
                "Stash tracked changes?",
                "Saves tracked staged and unstaged changes to a new stash entry and resets tracked files to HEAD. Untracked files remain untouched in the working tree. Hooks are bypassed.",
                "Stash changes",
                true,
            ),
            PendingReview::Entry(entry) => match entry.action {
                workspace::stash::EntryAction::Apply => (
                    "Apply stash entry?",
                    "Applies the pinned stash commit to the clean working tree without dropping the stash entry. Refuses if the working tree or stash commit drifted.",
                    "Apply stash",
                    false,
                ),
                workspace::stash::EntryAction::Drop => (
                    "Drop stash entry?",
                    "Permanently removes this stash entry from refs/stash. Working tree and index are untouched. Refuses if the stash stack shifted since review.",
                    "Drop stash",
                    true,
                ),
            },
        };
        let choice =
            confirmations::Dialog::new("git-stash-confirm", title, desc, "Cancel", confirm_label)
                .destructive(destructive)
                .show(ctx, |ui| match review {
                    PendingReview::Push(r) => {
                        ui.add(
                            egui::Label::new(theme::small(
                                ui,
                                format!(
                                    "{}
Branch: {} · HEAD: {}
Tracked files ({}):",
                                    r.root.display(),
                                    r.branch,
                                    r.head,
                                    r.files.len()
                                ),
                            ))
                            .wrap(),
                        );
                        egui::ScrollArea::vertical()
                            .id_salt("stash-push-files")
                            .max_height(140.0)
                            .show(ui, |ui| {
                                for f in &r.files {
                                    ui.label(
                                        RichText::new(f.escape_debug().to_string())
                                            .monospace()
                                            .size(11.0),
                                    );
                                }
                            });
                    }
                    PendingReview::Entry(r) => {
                        ui.add(
                            egui::Label::new(theme::small(
                                ui,
                                format!(
                                    "{}
{} · {}
Summary: {}",
                                    r.root.display(),
                                    r.selector,
                                    r.expected_oid,
                                    r.summary
                                ),
                            ))
                            .wrap(),
                        );
                        egui::ScrollArea::both()
                            .id_salt("stash-entry-preview")
                            .max_height(180.0)
                            .show_rows(ui, 16.0, r.preview.lines().count(), |ui, range| {
                                for line in r.preview.lines().skip(range.start).take(range.len()) {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(line).monospace().size(10.5),
                                        )
                                        .extend()
                                        .selectable(false),
                                    );
                                }
                            });
                    }
                });
        if let Some(choice) = choice {
            let taken = self.stash.review.take().unwrap();
            if choice == confirmations::Choice::Confirm {
                let Some(permit) = branches::permit(ctx) else {
                    self.stash.notice = Some("Git busy; retry explicitly.".into());
                    return;
                };
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.stash.applying = Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-stash-apply".into())
                    .spawn(move || {
                        let result = match taken {
                            PendingReview::Push(r) => workspace::stash::apply_push(r, &cancel)
                                .map(|oid| format!("Stashed tracked changes ({oid}). Untracked files preserved.")),
                            PendingReview::Entry(r) => {
                                let label = match r.action {
                                    workspace::stash::EntryAction::Apply => {
                                        format!("Applied {} ({}). Stash entry retained.", r.selector, &r.expected_oid[..r.expected_oid.len().min(10)])
                                    }
                                    workspace::stash::EntryAction::Drop => {
                                        format!("Dropped {} ({}).", r.selector, &r.expected_oid[..r.expected_oid.len().min(10)])
                                    }
                                };
                                workspace::stash::apply_entry(r, &cancel).map(|()| label)
                            }
                        };
                        drop(permit);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    })
                    .is_err()
                {
                    self.stash.applying = None;
                    self.stash.notice = Some("Cannot start stash worker.".into());
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
