//! Explicit reviewed push with bounded worker cancellation; never automatic retry.
#[cfg(test)]
mod tests;
use super::*;
mod create;
mod upstream;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
type RemoteResult = Result<(Option<String>, workspace::push::Relation), String>;
#[derive(Default)]
pub(super) struct Push {
    owner: Option<(PathBuf, String)>,
    pending: Option<workspace::read_only::Request<Result<workspace::push::Snapshot, String>>>,
    snapshot: Option<workspace::push::Snapshot>,
    notice: Option<String>,
    confirm: bool,
    hooks: bool,
    diagnostics: bool,
    sign: bool,
    sign_timeout: Option<u64>,
    timeout: Option<u64>,
    upstream: upstream::Upstream,
    create: create::Form,
    create_root: Option<PathBuf>,
    remote_confirm: bool,
    remote_pending: Option<workspace::read_only::Request<RemoteResult>>,
    remote_observed: Option<(workspace::push::Snapshot, RemoteResult)>,
    explicit: bool,
    remote: String,
    destination: String,
    applying: Option<workspace::read_only::Request<Result<workspace::push::Outcome, String>>>,
    controls: Vec<egui::Id>,
}
impl Changes {
    pub(super) fn push_review(&mut self, ui: &mut egui::Ui, workspace: &Workspace, loading: bool) {
        if self
            .push
            .owner
            .as_ref()
            .is_some_and(|o| o != &self.identity)
        {
            self.push = Default::default();
        }
        if let Some(result) = self.push.pending.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Push review worker ended.".into())),
        }) {
            self.push.pending = None;
            match result {
                Ok(s) => {
                    self.push.snapshot = Some(s);
                    self.push.hooks = false;
                    self.push.diagnostics = false;
                    self.push.timeout = None;
                    self.push.sign = false;
                    self.push.sign_timeout = None;
                }
                Err(e) => self.push.notice = Some(e),
            }
        }
        if let Some(result) = self
            .push
            .remote_pending
            .as_ref()
            .and_then(|r| match r.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(_) => Some(Err("Remote inspection ended.".into())),
            })
        {
            self.push.remote_pending = None;
            if let Some(snapshot) = &self.push.snapshot {
                self.push.remote_observed = Some((snapshot.clone(), result));
            }
        }
        self.upstream_poll();
        self.push.controls.clear();
        self.remote_create(ui, workspace);
        let enabled = workspace.is_git
            && self.push.create.pending.is_none()
            && self.push.upstream.pending.is_none()
            && self.push.applying.is_none()
            && self.push.remote_pending.is_none()
            && !loading
            && self.push.pending.is_none()
            && !egui::Popup::is_any_open(ui.ctx())
            && (self.repo_sheet.is_some() || ui.memory(|m| m.top_modal_layer().is_none()));
        ui.add_enabled_ui(enabled, |ui| {
            let toggle = ui.checkbox(
                &mut self.push.explicit,
                "Choose remote and branch (no upstream change)",
            );
            self.push.controls.push(toggle.id);
            if toggle.changed() {
                self.push.snapshot = None;
            }
            if self.push.explicit {
                for (label, value, limit) in [
                    ("Remote name", &mut self.push.remote, 256),
                    ("Branch name", &mut self.push.destination, 1000),
                ] {
                    ui.label(theme::small(ui, label));
                    let input = ui.add(
                        egui::TextEdit::singleline(value)
                            .char_limit(limit)
                            .desired_width(ui.available_width()),
                    );
                    self.push.controls.push(input.id);
                    if input.changed() {
                        self.push.snapshot = None;
                    }
                }
            }
        });
        let inspect = prompt_ui::button(
            ui,
            "Inspect push destination…",
            prompt_ui::Button::Secondary,
            enabled,
        );
        self.push.controls.push(inspect.id);
        if inspect.clicked() {
            self.push.snapshot = None;
            self.push.notice = None;
            let Some(permit) = branches::permit(ui.ctx()) else {
                self.push.notice = Some("Git busy; retry explicitly.".into());
                return;
            };
            let (tx, rx) = mpsc::channel();
            let cancel = Arc::new(AtomicBool::new(false));
            self.push.owner = Some(self.identity.clone());
            self.push.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
            let ctx = ui.ctx().clone();
            let root = workspace.root.clone();
            let target = self.push.explicit.then(|| {
                (
                    self.push.remote.clone(),
                    format!("refs/heads/{}", self.push.destination),
                )
            });
            if std::thread::Builder::new()
                .name("pui-push-review".into())
                .spawn(move || {
                    let result = if let Some((remote, destination)) = &target {
                        workspace::push::prepare_target(&root, Some((remote, destination)), &cancel)
                    } else {
                        workspace::push::prepare(&root, &cancel)
                    };
                    drop(permit);
                    if !cancel.load(Ordering::Acquire) {
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    }
                })
                .is_err()
            {
                self.push.pending = None;
                self.push.notice = Some("Cannot start push review.".into());
            }
        }
        if self.push.remote_pending.is_some() {
            ui.label(theme::small(
                ui,
                "Reading remote branch reference… hide the panel to cancel.",
            ));
        }
        if self.push.pending.is_some() {
            ui.label(theme::small(ui, "Reading local push configuration…"));
        }
        if let Some(error) = &self.push.notice {
            ui.add(egui::Label::new(theme::small(ui, error)).wrap());
        }
        if let Some(snapshot) = &self.push.snapshot {
            ui.add(egui::Label::new(theme::small(ui,format!("Local: {}\nCommit: {}\nRemote: {}\nDestination: {}\nEndpoint: {}\nLocal snapshot only — remote state/authentication not checked. No push performed yet. Authentication/remote state not checked.",snapshot.branch,snapshot.head,snapshot.remote,snapshot.destination,snapshot.endpoint))).wrap());
            if snapshot.explicit_target {
                let response = prompt_ui::button(
                    ui,
                    "Save as upstream…",
                    prompt_ui::Button::Secondary,
                    self.index_writable && enabled,
                );
                self.push.controls.push(response.id);
                if response.clicked() {
                    self.push.upstream.confirm = true;
                }
            }
            let inspect = prompt_ui::button(
                ui,
                "Inspect remote branch…",
                prompt_ui::Button::Secondary,
                self.index_writable && enabled,
            );
            self.push.controls.push(inspect.id);
            if inspect.clicked() {
                self.push.remote_confirm = true;
            }
            if let Some((observed, result)) = &self.push.remote_observed
                && observed == snapshot
            {
                let text = match result {
                    Ok((oid, relation)) => format!(
                        "{}\n{}\nObservation only · remote may change.",
                        oid.as_ref()
                            .map(|oid| format!("Observed remote commit: {oid}"))
                            .unwrap_or_default(),
                        relation.label()
                    ),
                    Err(e) => e.clone(),
                };
                ui.add(egui::Label::new(theme::small(ui, text)).wrap());
            }
            let response = prompt_ui::button(
                ui,
                "Push reviewed commit…",
                prompt_ui::Button::Secondary,
                self.index_writable && enabled,
            );
            self.push.controls.push(response.id);
            if response.clicked() {
                self.push.confirm = true;
            }
        }
        if self.push.applying.is_some() {
            ui.label(theme::small(
                ui,
                "Pushing reviewed commit… no automatic retry.",
            ));
            let response = prompt_ui::button(
                ui,
                "Cancel Push",
                prompt_ui::Button::Secondary,
                self.index_writable
                    && !egui::Popup::is_any_open(ui.ctx())
                    && ui.memory(|m| m.top_modal_layer().is_none())
                    && prompt_ui::keyboard_available(ui),
            );
            self.push.controls.push(response.id);
            #[cfg(test)]
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    egui::Id::new("cancel-push-control"),
                    (response.id, response.rect, response.enabled()),
                )
            });
            if response.clicked() {
                self.cancel_push();
            }
        }
    }
    fn remote_branch_dialog(&mut self, ctx: &egui::Context) {
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("inspect-remote-confirm"),
        );
        if !self.index_writable
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.push.remote_confirm = false;
            return;
        }
        let Some(snapshot) = &self.push.snapshot else {
            self.push.remote_confirm = false;
            return;
        };
        let choice=confirmations::Dialog::new("inspect-remote-confirm","Inspect remote branch?","Contacts this endpoint using existing credentials/SSH agent to read one branch reference. No objects fetched, push or tracking-ref update. Remote state can change immediately afterward.","Cancel","Inspect remote").destructive(false).show(ctx,|ui|{ui.add(egui::Label::new(theme::small(ui,format!("{}\n{}",snapshot.endpoint,snapshot.destination))).wrap());});
        if let Some(choice) = choice {
            self.push.remote_confirm = false;
            if choice == confirmations::Choice::Confirm {
                let snapshot = snapshot.clone();
                let Some(permit) = branches::permit(ctx) else {
                    self.push.notice = Some("Git busy; inspect again.".into());
                    return;
                };
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.push.remote_pending =
                    Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-remote-inspect".into())
                    .spawn(move || {
                        let result =
                            workspace::push::inspect_remote(&snapshot, &cancel).map(|oid| {
                                let relation =
                                    workspace::push::compare(&snapshot, oid.as_deref(), &cancel);
                                (oid, relation)
                            });
                        drop(permit);
                        if !cancel.load(Ordering::Acquire) {
                            let _ = tx.send(result);
                            ctx.request_repaint();
                        }
                    })
                    .is_err()
                {
                    self.push.remote_pending = None;
                    self.push.notice = Some("Cannot start remote inspection.".into());
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
    fn cancel_push(&mut self) {
        if self.push.applying.take().is_some() {
            self.push.snapshot = None;
            self.push.confirm = false;
            self.push.notice=Some("Push cancellation requested. Remote completion may be uncertain; inspect the remote before retrying. No automatic retry.".into());
            self.refresh = true;
        }
    }
    pub(super) fn push_inputs(&mut self, ctx: &egui::Context, visible: bool) {
        if !visible
            || !self.index_writable
            || self
                .push
                .owner
                .as_ref()
                .is_some_and(|o| o != &self.identity)
        {
            self.push.confirm = false;
            self.push.remote_confirm = false;
            self.push.upstream.confirm = false;
            self.push.upstream.pending = None;
            self.push.create.confirm = false;
            self.push.create.pending = None;
            self.push.remote_pending = None;
            self.cancel_push();
        }
        if let Some(result) = self
            .push
            .applying
            .as_ref()
            .and_then(|r| match r.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(_) => Some(Err(
                    "Push completion uncertain; inspect remote before retry.".into(),
                )),
            })
        {
            self.push.applying = None;
            self.push.snapshot = None;
            self.push.notice = Some(match result {
                Ok(workspace::push::Outcome::Updated) => format!(
                    "Push updated the reviewed branch. No tags or force.{} {}",
                    if self.push.sign {
                        "Signed push requested; server verification not established."
                    } else {
                        "No push signing."
                    },
                    if self.push.hooks {
                        "Configured hooks were enabled."
                    } else {
                        "No hooks ran."
                    }
                ),
                Ok(workspace::push::Outcome::UpToDate) => format!(
                    "Reviewed remote branch is already up to date. No tags or force.{}",
                    if self.push.sign {
                        " Signed certificate was requested."
                    } else {
                        " No push signing."
                    }
                ),
                Err(e) => e,
            });
            self.refresh = true;
        }
        let focus = ctx.memory(|m| m.focused());
        let pointer = ctx.input(|i| {
            (i.pointer.primary_pressed() || i.pointer.primary_released())
                .then(|| i.pointer.interact_pos())
                .flatten()
        });
        let pointer_owns = self.push.controls.iter().any(|id| {
            ctx.read_response(*id).is_some_and(|r| {
                r.enabled()
                    && pointer.is_some_and(|p| {
                        r.interact_rect.contains(p) && ctx.layer_id_at(p) == Some(r.layer_id)
                    })
            })
        });
        if self.push.confirm
            || self.push.upstream.confirm
            || self.push.create.confirm
            || self.push.remote_confirm
            || pointer_owns
            || focus.is_some_and(|id| self.push.controls.contains(&id))
        {
            ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        }
    }
    pub(super) fn push_dialog(&mut self, ctx: &egui::Context) {
        if self.push.upstream.confirm {
            self.upstream_dialog(ctx);
            return;
        }
        if self.push.create.confirm {
            self.remote_create_dialog(ctx);
            return;
        }
        if self.push.remote_confirm {
            self.remote_branch_dialog(ctx);
            return;
        }
        if !self.push.confirm {
            return;
        }
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("push-reviewed-confirm"),
        );
        if !self.index_writable
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.push.confirm = false;
            return;
        }
        let Some(snapshot) = &self.push.snapshot else {
            self.push.confirm = false;
            return;
        };
        self.push.controls.truncate(4);
        let description = if self.push.sign {
            if self.push.hooks {
                "Pushes a GPG-signed certificate with configured pre-push hooks. Hooks execute local code; signing failure stops Push with no unsigned fallback. No force/tags. Credentials may be used; remote completion can be uncertain."
            } else {
                "Pushes a GPG-signed certificate without pre-push hooks. Signing failure stops Push with no unsigned fallback. No force/tags. Credentials may be used; remote completion can be uncertain."
            }
        } else if self.push.hooks {
            "Pushes reviewed commit with configured pre-push hooks. Hooks execute local code with your OS permissions; they may modify files/contact services. Hook failure stops Push, no bypass fallback. No force/tags/push signing. Credentials may be used; remote completion can be uncertain."
        } else {
            "Pushes reviewed commit without pre-push hooks or push signing. No force/tags. Existing credentials may be used; interactive login and unknown SSH hosts refuse. Remote completion can be uncertain."
        };
        let choice = confirmations::Dialog::new(
            "push-reviewed-confirm",
            "Push reviewed commit?",
            description,
            "Cancel",
            if self.push.hooks {
                "Push with hooks"
            } else {
                "Push without hooks"
            },
        )
        .show(ctx, |ui| {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    format!(
                        "{}\n{} → {}\n{}",
                        snapshot.endpoint, snapshot.head, snapshot.destination, snapshot.branch
                    ),
                ))
                .wrap(),
            );
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    "Push timeout (seconds, includes hooks and network; cleanup may take longer)",
                ))
                .wrap(),
            );
            let timeout =
                ui.add(egui::DragValue::new(self.push.timeout.get_or_insert(30)).range(1..=300));
            self.push.controls.push(timeout.id);
            let response = ui.checkbox(
                &mut self.push.hooks,
                "Run configured pre-push hooks (local code execution)",
            );
            self.push.controls.push(response.id);
            let details = ui.checkbox(
                &mut self.push.diagnostics,
                "Show failure diagnostics (may expose secrets from remote/hooks)",
            );
            self.push.controls.push(details.id);
            let sign = ui.checkbox(
                &mut self.push.sign,
                "Sign this push (GPG push certificate; no unsigned fallback)",
            );
            self.push.controls.push(sign.id);
            if self.push.sign {
                ui.add(
                    egui::Label::new(theme::small(
                        ui,
                        "Signed-push timeout (seconds; caps whole child including network/signing)",
                    ))
                    .wrap(),
                );
                let sign_timeout = ui.add(
                    egui::DragValue::new(self.push.sign_timeout.get_or_insert(30)).range(1..=300),
                );
                self.push.controls.push(sign_timeout.id);
            }
        });
        if let Some(choice) = choice {
            self.push.confirm = false;
            if choice == confirmations::Choice::Confirm {
                let snapshot = self.push.snapshot.take().unwrap();
                let hooks = self.push.hooks;
                let details = self.push.diagnostics;
                let seconds = self.push.timeout.unwrap_or(30);
                let sign = self.push.sign;
                let sign_timeout = self.push.sign_timeout.unwrap_or(30);
                let Some(permit) = branches::permit(ctx) else {
                    self.push.notice = Some("Git busy; inspect again.".into());
                    return;
                };
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.push.applying = Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-confirmed-push".into())
                    .spawn(move || {
                        let result = workspace::push::apply_signed(
                            snapshot,
                            hooks,
                            details,
                            sign,
                            seconds,
                            sign_timeout,
                            &cancel,
                        );
                        drop(permit);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    })
                    .is_err()
                {
                    self.push.applying = None;
                    self.push.notice = Some("Cannot start push worker.".into());
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
