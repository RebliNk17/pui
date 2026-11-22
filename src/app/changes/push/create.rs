//! Explicit local remote creation; form intent never persisted.
#[cfg(test)]
mod tests;
use super::*;
#[derive(Default)]
pub(super) struct Form {
    name: String,
    url: String,
    pub(super) confirm: bool,
    pub(super) pending: Option<workspace::read_only::Request<Result<(), String>>>,
}
impl Changes {
    pub(in crate::app::changes) fn remote_create(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
    ) {
        if let Some(result) = self
            .push
            .create
            .pending
            .as_ref()
            .and_then(|r| match r.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(_) => Some(Err(
                    "Remote creation ended; inspect config before retrying.".into(),
                )),
            })
        {
            self.push.create.pending = None;
            self.push.notice = Some(match result {
                Ok(()) => {
                    "Remote created locally. No fetch/push performed. Inspect destination next."
                        .into()
                }
                Err(e) => e,
            });
            self.push.snapshot = None;
        }
        let available = self.index_writable
            && workspace.is_git
            && self.push.applying.is_none()
            && self.push.pending.is_none()
            && self.push.remote_pending.is_none()
            && self.push.create.pending.is_none()
            && !egui::Popup::is_any_open(ui.ctx())
            && (self.repo_sheet.is_some() || ui.memory(|m| m.top_modal_layer().is_none()));
        egui::CollapsingHeader::new("Create remote…")
            .id_salt("create-push-remote")
            .default_open(self.repo_sheet == Some(RepoSheet::CreateRemote))
            .show(ui, |ui| {
                ui.add_enabled_ui(available, |ui| {
                    for (label, value, limit) in [
                        ("New remote name", &mut self.push.create.name, 64),
                        (
                            "Remote URL (no embedded credentials)",
                            &mut self.push.create.url,
                            8192,
                        ),
                    ] {
                        ui.label(theme::small(ui, label));
                        let response = ui.add(
                            egui::TextEdit::singleline(value)
                                .char_limit(limit)
                                .desired_width(ui.available_width()),
                        );
                        self.push.controls.push(response.id);
                    }
                    let valid = workspace::push::remote::valid_name(&self.push.create.name)
                        && workspace::push::remote::valid_url(&self.push.create.url);
                    let response = prompt_ui::button(
                        ui,
                        "Review new remote…",
                        prompt_ui::Button::Secondary,
                        valid,
                    );
                    self.push.controls.push(response.id);
                    if response.clicked() {
                        self.push.owner = Some(self.identity.clone());
                        self.push.create_root = Some(workspace.root.clone());
                        self.push.create.confirm = true;
                    }
                });
            });
    }
    pub(super) fn remote_create_dialog(&mut self, ctx: &egui::Context) {
        if !self.push.create.confirm {
            return;
        }
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("create-remote-confirm"),
        );
        if !self.index_writable
            || self.push.owner.as_ref() != Some(&self.identity)
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.push.create.confirm = false;
            return;
        }
        let choice=confirmations::Dialog::new("create-remote-confirm","Create local Git remote?","Adds one new named remote and its fetch mapping to local Git configuration. Existing names are never replaced. No network contact, fetch, push or upstream change.","Cancel","Create remote").destructive(false).show(ctx,|ui|{ui.add(egui::Label::new(theme::small(ui,format!("{}\n{}",self.push.create.name,self.push.create.url))).wrap());});
        if let Some(choice) = choice {
            self.push.create.confirm = false;
            if choice == confirmations::Choice::Confirm {
                let Some(permit) = branches::permit(ctx) else {
                    self.push.notice = Some("Git busy; retry explicitly.".into());
                    return;
                };
                let root = self.push.create_root.take().unwrap_or_default();
                let name = self.push.create.name.clone();
                let url = self.push.create.url.clone();
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.push.create.pending =
                    Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-create-remote".into())
                    .spawn(move || {
                        let result = workspace::push::remote::create(&root, &name, &url, &cancel);
                        drop(permit);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    })
                    .is_err()
                {
                    self.push.create.pending = None;
                    self.push.notice = Some("Cannot start remote configuration worker.".into());
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
