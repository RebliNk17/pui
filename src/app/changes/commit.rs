//! Explicit hookless local staged-snapshot commit. Never auto push.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
fn ui_signing_text(sign: bool) -> &'static str {
    if sign {
        "Creates a signed local commit using the configured Git signer/key. Signer programs and agent/pinentry may run with OS permissions. Failure never falls back to unsigned. Hooks bypassed; working files unchanged; no push."
    } else {
        "Creates an unsigned local commit. Hooks bypassed; working files unchanged; no push. Opt in to signing below if an available Git signer/key is configured."
    }
}
#[derive(Default)]
pub(super) struct Commit {
    owner: Option<(PathBuf, String)>,
    pending: Option<workspace::read_only::Request<Result<workspace::commit::Snapshot, String>>>,
    applying: Option<workspace::read_only::Request<Result<String, String>>>,
    snapshot: Option<workspace::commit::Snapshot>,
    message: String,
    retained_message: String,
    sign: bool,
    pre_commit: bool,
    commit_msg: bool,
    hook_timeout: u64,
    applied_signed: bool,
    notice: Option<String>,
    controls: Vec<egui::Id>,
}
impl Changes {
    pub(super) fn commit_control(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        loading: bool,
    ) {
        self.commit.controls.clear();
        let enabled = self.index_writable
            && workspace.is_git
            && !loading
            && workspace.error.is_none()
            && self.commit.pending.is_none()
            && self.commit.applying.is_none()
            && self.commit.snapshot.is_none()
            && !egui::Popup::is_any_open(ui.ctx())
            && ui.memory(|m| m.top_modal_layer().is_none());
        let response = ui
            .push_id(
                ("inspect-staged-commit", &self.identity, &workspace.root),
                |ui| {
                    prompt_ui::button(
                        ui,
                        "Review staged commit…",
                        prompt_ui::Button::Secondary,
                        enabled,
                    )
                },
            )
            .inner;
        self.commit.controls.push(response.id);
        if response.clicked() {
            self.start_commit_inspection(ui.ctx(), workspace.root.clone());
        }
        if self.commit.applying.is_some() {
            ui.label(theme::small(
                ui,
                "Committing locally… hook/signer may still be running.",
            ));
            let cancel = prompt_ui::button(
                ui,
                "Cancel Commit",
                prompt_ui::Button::Secondary,
                self.index_writable
                    && !egui::Popup::is_any_open(ui.ctx())
                    && ui.memory(|m| m.top_modal_layer().is_none())
                    && prompt_ui::keyboard_available(ui),
            );
            self.commit.controls.push(cancel.id);
            #[cfg(test)]
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    egui::Id::new("cancel-commit-control"),
                    (cancel.id, cancel.rect, cancel.enabled()),
                )
            });
            if cancel.clicked() {
                self.cancel_commit();
            }
        }
        if !self.commit.retained_message.is_empty() && self.commit.applying.is_none() {
            ui.label(theme::small(
                ui,
                "Commit message retained locally. Fresh staged review is required; options reset.",
            ));
            let clear = prompt_ui::button(
                ui,
                "Discard retained message",
                prompt_ui::Button::Secondary,
                !egui::Popup::is_any_open(ui.ctx()) && ui.memory(|m| m.top_modal_layer().is_none()),
            );
            self.commit.controls.push(clear.id);
            if clear.clicked() {
                self.commit.retained_message.clear();
            }
        }
        if let Some(notice) = &self.commit.notice {
            ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
        }
    }
    fn cancel_commit(&mut self) {
        if self.commit.applying.take().is_some() {
            self.commit.snapshot = None;
            self.commit.message.clear();
            self.commit.notice=Some("Commit cancellation requested. Branch publication may already have completed; inspect History and staged changes before retrying. No automatic retry.".into());
            self.refresh = true;
            self.invalidate();
        }
    }
    fn start_commit_inspection(&mut self, ctx: &egui::Context, root: PathBuf) {
        let Some(permit) = branches::permit(ctx) else {
            self.commit.notice = Some("Git busy; retry explicitly.".into());
            return;
        };
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.commit.owner = Some(self.identity.clone());
        self.commit.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        if std::thread::Builder::new()
            .name("pui-commit-review".into())
            .spawn(move || {
                let result = workspace::commit::prepare(&root, &cancel);
                drop(permit);
                if !cancel.load(Ordering::Acquire) {
                    let _ = tx.send(result);
                    ctx.request_repaint();
                }
            })
            .is_err()
        {
            self.commit.pending = None;
            self.commit.notice = Some("Cannot start staged inspection.".into());
        }
    }
    pub(super) fn commit_inputs(&mut self, ctx: &egui::Context, visible: bool) {
        if !visible
            || !self.index_writable
            || self
                .commit
                .owner
                .as_ref()
                .is_some_and(|o| o != &self.identity)
        {
            let active = self.commit.applying.is_some();
            self.cancel_commit();
            if !active {
                self.commit = Default::default();
            } else {
                self.commit.pending = None;
                self.commit.controls.clear();
            }
            return;
        }
        if let Some(result) = self
            .commit
            .pending
            .as_ref()
            .and_then(|r| match r.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(_) => Some(Err("Staged inspection ended.".into())),
            })
        {
            self.commit.pending = None;
            match result {
                Ok(s) => {
                    self.commit.snapshot = Some(s);
                    self.commit.message = self.commit.retained_message.clone();
                    self.commit.sign = false;
                    self.commit.pre_commit = false;
                    self.commit.commit_msg = false;
                    self.commit.hook_timeout = 30;
                }
                Err(e) => self.commit.notice = Some(e),
            }
        }
        if let Some(result) = self
            .commit
            .applying
            .as_ref()
            .and_then(|r| match r.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(_) => Some(Err(
                    "Commit completion uncertain; inspect History before retrying.".into(),
                )),
            })
        {
            self.commit.applying = None;
            self.commit.notice = Some(match result {
                Ok(oid) => {
                    self.commit.retained_message.clear();
                    format!(
                        "Committed locally: {oid}. No push. Hook/signing options explicitly selected. {}",
                        if self.commit.applied_signed {
                            "Signing requested from configured Git signer; signer trust not verified by Pui."
                        } else {
                            "Unsigned commit."
                        }
                    )
                }
                Err(e) => e,
            });
            self.refresh = true;
            self.invalidate();
        }
        let focus = ctx.memory(|m| m.focused());
        let pointer = ctx.input(|i| {
            (i.pointer.primary_pressed() || i.pointer.primary_released())
                .then(|| i.pointer.interact_pos())
                .flatten()
        });
        let pointer_owns = self.commit.controls.iter().any(|id| {
            ctx.read_response(*id).is_some_and(|r| {
                r.enabled()
                    && pointer.is_some_and(|p| {
                        r.interact_rect.contains(p) && ctx.layer_id_at(p) == Some(r.layer_id)
                    })
            })
        });
        if self.commit.snapshot.is_some()
            || pointer_owns
            || focus.is_some_and(|id| self.commit.controls.contains(&id))
        {
            ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        }
    }
    pub(super) fn commit_dialog(&mut self, ctx: &egui::Context) {
        let Some(snapshot) = &self.commit.snapshot else {
            return;
        };
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("commit-staged-confirm"),
        );
        if !self.index_writable
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.commit.snapshot = None;
            return;
        }
        let initial = snapshot.head.bytes().all(|b| b == b'0');
        self.commit.controls.truncate(1);
        ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        let signing = if self.commit.pre_commit || self.commit.commit_msg {
            "Runs selected pre-commit/commit-msg hooks with OS permissions/private index. Failure or changed reviewed message/snapshot stops commit. Hooks can modify files/contact services. Unselected and other commit hooks bypassed. Signing optional below; no push."
        } else {
            ui_signing_text(self.commit.sign)
        };
        let choice = confirmations::Dialog::new(
            "commit-staged-confirm",
            if initial {
                "Create initial staged commit?"
            } else {
                "Commit staged snapshot?"
            },
            signing,
            "Cancel",
            if self.commit.pre_commit || self.commit.commit_msg {
                "Check and commit"
            } else if self.commit.sign {
                "Sign and commit without hooks"
            } else {
                "Commit without hooks"
            },
        )
        .destructive(false)
        .enabled(workspace::commit::valid_message(&self.commit.message))
        .show(ctx, |ui| {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    format!(
                        "{}\n{}\nAuthor: {} <{}>\nCommitter: {} <{}>",
                        snapshot.root.display(),
                        snapshot.branch,
                        snapshot.author.name,
                        snapshot.author.email,
                        snapshot.committer.name,
                        snapshot.committer.email
                    ),
                ))
                .wrap(),
            );
            let response = ui.add(
                egui::TextEdit::multiline(&mut self.commit.message)
                    .hint_text("Commit message…")
                    .char_limit(4096)
                    .desired_rows(3)
                    .desired_width(ui.available_width()),
            );
            self.commit.controls.push(response.id);
            let signing = ui.checkbox(
                &mut self.commit.sign,
                "Sign with configured Git signer (may use agent/pinentry)",
            );
            self.commit.controls.push(signing.id);
            let check = ui.checkbox(
                &mut self.commit.pre_commit,
                "Run configured pre-commit check (local code execution)",
            );
            self.commit.controls.push(check.id);
            let message_check = ui.checkbox(
                &mut self.commit.commit_msg,
                "Run configured commit-msg check (message rewrites refuse)",
            );
            self.commit.controls.push(message_check.id);
            if self.commit.pre_commit || self.commit.commit_msg {
                ui.label(theme::small(
                    ui,
                    "Timeout per selected hook (seconds, 1–300)",
                ));
                let timeout =
                    ui.add(egui::DragValue::new(&mut self.commit.hook_timeout).range(1..=300));
                self.commit.controls.push(timeout.id);
            }
            ui.label(theme::small(
                ui,
                format!(
                    "{} staged files · renames shown as delete/add",
                    snapshot.files.len()
                ),
            ));
            egui::ScrollArea::both()
                .id_salt("commit-file-list")
                .max_height(72.0)
                .show_rows(ui, 16.0, snapshot.files.len(), |ui, range| {
                    for entry in &snapshot.files[range] {
                        ui.add(
                            egui::Label::new(theme::small(
                                ui,
                                format!("{}  {}", entry.status, entry.path.escape_debug()),
                            ))
                            .extend(),
                        );
                    }
                });
            egui::ScrollArea::both()
                .id_salt("commit-staged-patch")
                .max_height(180.0)
                .show_rows(ui, 16.0, snapshot.preview.lines().count(), |ui, range| {
                    for line in snapshot.preview.lines().skip(range.start).take(range.len()) {
                        ui.add(
                            egui::Label::new(RichText::new(line).monospace().size(10.5))
                                .extend()
                                .selectable(false),
                        );
                    }
                });
        });
        if let Some(choice) = choice {
            let snapshot = self.commit.snapshot.take().unwrap();
            if choice == confirmations::Choice::Confirm {
                let Some(permit) = branches::permit(ctx) else {
                    self.commit.notice = Some("Git busy; inspect again.".into());
                    return;
                };
                let message = std::mem::take(&mut self.commit.message);
                self.commit.retained_message = message.clone();
                let sign = self.commit.sign;
                let pre_commit = self.commit.pre_commit;
                let commit_msg = self.commit.commit_msg;
                let hook_timeout = self.commit.hook_timeout.clamp(1, 300);
                self.commit.applied_signed = sign;
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.commit.applying = Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-commit-publish".into())
                    .spawn(move || {
                        let result = if commit_msg || pre_commit {
                            workspace::commit::apply_timed_checks(
                                snapshot,
                                &message,
                                sign,
                                pre_commit,
                                commit_msg,
                                hook_timeout,
                                &cancel,
                            )
                        } else if sign {
                            workspace::commit::apply_with_signing(snapshot, &message, true, &cancel)
                        } else {
                            workspace::commit::apply(snapshot, &message, &cancel)
                        };
                        drop(permit);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    })
                    .is_err()
                {
                    self.commit.applying = None;
                    self.commit.notice = Some("Cannot start commit worker.".into());
                }
            }
            self.commit.message.clear();
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
