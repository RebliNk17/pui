//! Persistent recovery-copy manager UI: list, inspect, restore (with backup), and delete.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(test)]
mod tests;

enum PendingReview {
    Restore(workspace::recovery_manager::RestoreReview),
    Delete(workspace::recovery_manager::DeleteReview),
}

#[derive(Default)]
pub(super) struct RecoveryManager {
    owner: Option<(PathBuf, String)>,
    pub(super) loaded: bool,
    summary: workspace::recovery_manager::Summary,
    manual_paths: BTreeMap<String, String>,
    list_rx:
        Option<workspace::read_only::Request<Result<workspace::recovery_manager::Summary, String>>>,
    review_rx: Option<workspace::read_only::Request<Result<PendingReview, String>>>,
    review: Option<PendingReview>,
    applying: Option<workspace::read_only::Request<Result<String, String>>>,
    notice: Option<String>,
    controls: Vec<egui::Id>,
}

impl Changes {
    pub(super) fn refresh_recovery_copies(&mut self, ctx: &egui::Context, root: PathBuf) {
        let s = &mut self.recovery_manager;
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
            .name("pui-recovery-list".into())
            .spawn(move || {
                let result = workspace::recovery_manager::list(&root, &cancel);
                drop(permit);
                if !cancel.load(Ordering::Acquire) {
                    let _ = tx.send(result);
                    ctx.request_repaint();
                }
            })
            .is_err()
        {
            s.list_rx = None;
            s.notice = Some("Cannot scan recovery copies.".into());
        }
    }

    pub(super) fn recovery_manager_ui(&mut self, ui: &mut egui::Ui, workspace: &Workspace) {
        if !workspace.is_git {
            return;
        }
        if self.repo_sheet == Some(RepoSheet::Recovery)
            && !self.recovery_manager.loaded
            && self.recovery_manager.list_rx.is_none()
        {
            self.refresh_recovery_copies(ui.ctx(), workspace.root.clone());
        }
        let modal_ok = self.repo_sheet.is_some() || ui.memory(|m| m.top_modal_layer().is_none());
        let busy = self.recovery_manager.list_rx.is_some()
            || self.recovery_manager.review_rx.is_some()
            || self.recovery_manager.review.is_some()
            || self.recovery_manager.applying.is_some();
        let enabled =
            self.index_writable && !busy && !egui::Popup::is_any_open(ui.ctx()) && modal_ok;
        let width = ui.available_width();
        self.recovery_manager.controls.clear();

        ui.horizontal_wrapped(|ui| {
            ui.label(theme::small(
                ui,
                format!(
                    "{} retained copies ({} bytes){}",
                    self.recovery_manager.summary.entries.len(),
                    self.recovery_manager.summary.total_bytes,
                    if self.recovery_manager.summary.truncated {
                        " · list truncated at cap"
                    } else {
                        ""
                    }
                ),
            ));
            let refresh_btn = prompt_ui::button(
                ui,
                "Refresh recovery copies",
                prompt_ui::Button::Secondary,
                !busy && modal_ok,
            );
            self.recovery_manager.controls.push(refresh_btn.id);
            if refresh_btn.clicked() {
                self.refresh_recovery_copies(ui.ctx(), workspace.root.clone());
            }
        });
        ui.add_space(6.0);

        if self.recovery_manager.list_rx.is_some() {
            ui.label(theme::small(ui, "Scanning repository recovery copies…"));
        } else if self.recovery_manager.summary.entries.is_empty() {
            ui.label(theme::small(
                ui,
                "No retained discard or conflict recovery copies found in this repository.",
            ));
        } else {
            let colors = theme::palette(ui.ctx());
            let entries = self.recovery_manager.summary.entries.clone();
            let mut restore_req: Option<(PathBuf, String)> = None;
            let mut delete_req: Option<PathBuf> = None;

            egui::ScrollArea::vertical()
                .id_salt("changes-recovery-entries")
                .max_height(240.0)
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
                                        "{} · {} bytes · {}",
                                        entry.kind.label(),
                                        entry.size_bytes,
                                        &entry.file_name[..entry.file_name.len().min(32)]
                                    ))
                                    .size(11.0)
                                    .strong(),
                                );
                                let target_path = if let Some(matched) = &entry.matched_path {
                                    ui.label(
                                        RichText::new(format!("Path: {}", matched.escape_debug()))
                                            .monospace()
                                            .size(11.0),
                                    );
                                    matched.clone()
                                } else {
                                    let draft = self
                                        .recovery_manager
                                        .manual_paths
                                        .entry(entry.file_name.clone())
                                        .or_default();
                                    let r = Self::qm_text_input(
                                        ui,
                                        draft,
                                        "Enter original relative path to verify hash…",
                                        1024,
                                        true,
                                        width - 24.0,
                                    );
                                    self.recovery_manager.controls.push(r.id);
                                    draft.trim().to_owned()
                                };
                                if !entry.preview.is_empty() {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&entry.preview).monospace().size(10.5),
                                        )
                                        .wrap(),
                                    );
                                }
                                ui.horizontal_wrapped(|ui| {
                                    let can_restore =
                                        enabled && workspace::valid_diff_path(&target_path);
                                    let res_btn = prompt_ui::button(
                                        ui,
                                        "Review restore…",
                                        prompt_ui::Button::Secondary,
                                        can_restore,
                                    );
                                    self.recovery_manager.controls.push(res_btn.id);
                                    if res_btn.clicked() {
                                        restore_req =
                                            Some((entry.recovery_path.clone(), target_path));
                                    }
                                    let del_btn = prompt_ui::button(
                                        ui,
                                        "Review delete…",
                                        prompt_ui::Button::Secondary,
                                        enabled,
                                    );
                                    self.recovery_manager.controls.push(del_btn.id);
                                    if del_btn.clicked() {
                                        delete_req = Some(entry.recovery_path.clone());
                                    }
                                });
                            });
                        ui.add_space(4.0);
                    }
                });

            if let Some((recovery_path, target_path)) = restore_req {
                let Some(permit) = branches::permit(ui.ctx()) else {
                    self.recovery_manager.notice = Some("Git busy; retry explicitly.".into());
                    return;
                };
                let root = workspace.root.clone();
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.recovery_manager.owner = Some(self.identity.clone());
                self.recovery_manager.notice = None;
                self.recovery_manager.review_rx =
                    Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ui.ctx().clone();
                if std::thread::Builder::new()
                    .name("pui-recovery-mgr-restore-review".into())
                    .spawn(move || {
                        let result = workspace::recovery_manager::inspect_restore(
                            &root,
                            &recovery_path,
                            &target_path,
                            &cancel,
                        )
                        .map(PendingReview::Restore);
                        drop(permit);
                        if !cancel.load(Ordering::Acquire) {
                            let _ = tx.send(result);
                            ctx.request_repaint();
                        }
                    })
                    .is_err()
                {
                    self.recovery_manager.review_rx = None;
                    self.recovery_manager.notice = Some("Cannot inspect recovery restore.".into());
                }
            } else if let Some(recovery_path) = delete_req {
                let Some(permit) = branches::permit(ui.ctx()) else {
                    self.recovery_manager.notice = Some("Git busy; retry explicitly.".into());
                    return;
                };
                let root = workspace.root.clone();
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.recovery_manager.owner = Some(self.identity.clone());
                self.recovery_manager.notice = None;
                self.recovery_manager.review_rx =
                    Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ui.ctx().clone();
                if std::thread::Builder::new()
                    .name("pui-recovery-mgr-delete-review".into())
                    .spawn(move || {
                        let result = workspace::recovery_manager::inspect_delete(
                            &root,
                            &recovery_path,
                            &cancel,
                        )
                        .map(PendingReview::Delete);
                        drop(permit);
                        if !cancel.load(Ordering::Acquire) {
                            let _ = tx.send(result);
                            ctx.request_repaint();
                        }
                    })
                    .is_err()
                {
                    self.recovery_manager.review_rx = None;
                    self.recovery_manager.notice = Some("Cannot inspect recovery deletion.".into());
                }
            }
        }
        if let Some(notice) = &self.recovery_manager.notice {
            ui.add_space(4.0);
            ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
        }
    }

    pub(super) fn recovery_manager_inputs(&mut self, _ctx: &egui::Context, visible: bool) {
        let s = &mut self.recovery_manager;
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
            Err(_) => Some(Err("Recovery scan ended.".into())),
        }) {
            s.list_rx = None;
            match result {
                Ok(summary) => s.summary = summary,
                Err(e) => s.notice = Some(e),
            }
        }
        if let Some(result) = s.review_rx.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Recovery review ended.".into())),
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
            Err(_) => Some(Err("Recovery operation ended.".into())),
        }) {
            s.applying = None;
            match result {
                Ok(msg) => {
                    s.notice = Some(msg);
                    s.loaded = false;
                }
                Err(e) => s.notice = Some(e),
            }
            self.refresh = true;
            self.invalidate();
        }
    }

    pub(super) fn recovery_manager_dialog(&mut self, ctx: &egui::Context) {
        let Some(review) = &self.recovery_manager.review else {
            return;
        };
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("git-recovery-manager-confirm"),
        );
        if !self.index_writable
            || self.recovery_manager.owner.as_ref() != Some(&self.identity)
            || egui::Popup::is_any_open(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
            || prompt_ui::modal_owns_current_pass(ctx)
        {
            self.recovery_manager.review = None;
            return;
        }
        let (title, desc, confirm_label) = match review {
            PendingReview::Restore(_) => (
                "Restore recovery copy?",
                "Replaces the working file with this cryptographically verified recovery copy. If the working file currently exists, its current bytes are backed up first. The recovery copy itself is retained.",
                "Restore file",
            ),
            PendingReview::Delete(_) => (
                "Delete recovery copy?",
                "Permanently removes only this recovery backup file from the repository's Git directory. Working files and index are untouched.",
                "Delete backup",
            ),
        };
        let choice = confirmations::Dialog::new(
            "git-recovery-manager-confirm",
            title,
            desc,
            "Cancel",
            confirm_label,
        )
        .destructive(true)
        .show(ctx, |ui| match review {
            PendingReview::Restore(r) => {
                ui.add(
                    egui::Label::new(theme::small(
                        ui,
                        format!(
                            "{}
Target: {} ({} · {} bytes)
Source: {}",
                            r.root.display(),
                            r.target_path.escape_debug(),
                            r.kind.label(),
                            r.size_bytes,
                            r.recovery_path.display()
                        ),
                    ))
                    .wrap(),
                );
                egui::ScrollArea::both()
                    .id_salt("recovery-mgr-restore-preview")
                    .max_height(180.0)
                    .show_rows(ui, 16.0, r.preview.lines().count(), |ui, range| {
                        for line in r.preview.lines().skip(range.start).take(range.len()) {
                            ui.add(
                                egui::Label::new(RichText::new(line).monospace().size(10.5))
                                    .extend()
                                    .selectable(false),
                            );
                        }
                    });
            }
            PendingReview::Delete(r) => {
                ui.add(
                    egui::Label::new(theme::small(
                        ui,
                        format!(
                            "{}
File: {} ({} · {} bytes)
Matched path: {}",
                            r.root.display(),
                            r.file_name,
                            r.kind.label(),
                            r.size_bytes,
                            r.matched_path.as_deref().unwrap_or("(unmatched)")
                        ),
                    ))
                    .wrap(),
                );
                egui::ScrollArea::both()
                    .id_salt("recovery-mgr-delete-preview")
                    .max_height(180.0)
                    .show_rows(ui, 16.0, r.preview.lines().count(), |ui, range| {
                        for line in r.preview.lines().skip(range.start).take(range.len()) {
                            ui.add(
                                egui::Label::new(RichText::new(line).monospace().size(10.5))
                                    .extend()
                                    .selectable(false),
                            );
                        }
                    });
            }
        });
        if let Some(choice) = choice {
            let taken = self.recovery_manager.review.take().unwrap();
            if choice == confirmations::Choice::Confirm {
                let Some(permit) = branches::permit(ctx) else {
                    self.recovery_manager.notice = Some("Git busy; retry explicitly.".into());
                    return;
                };
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.recovery_manager.applying =
                    Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-recovery-mgr-apply".into())
                    .spawn(move || {
                        let result = match taken {
                            PendingReview::Restore(r) => {
                                let path = r.target_path.clone();
                                workspace::recovery_manager::apply_restore(r, &cancel).map(|()| {
                                    format!("Restored {path} from recovery copy. Previous working content backed up.")
                                })
                            }
                            PendingReview::Delete(r) => {
                                let name = r.file_name.clone();
                                workspace::recovery_manager::apply_delete(r, &cancel)
                                    .map(|()| format!("Deleted recovery copy {name}."))
                            }
                        };
                        drop(permit);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    })
                    .is_err()
                {
                    self.recovery_manager.applying = None;
                    self.recovery_manager.notice = Some("Cannot start recovery worker.".into());
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
