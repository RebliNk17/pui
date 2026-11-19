//! Repository-local identity creation; transient review, no global config.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
pub(super) struct Setup {
    owner: Option<(PathBuf, String)>,
    name: String,
    email: String,
    override_inherited: bool,
    edit_local: bool,
    loading: Option<workspace::read_only::Request<Result<(String, String), String>>>,
    pending: Option<
        workspace::read_only::Request<Result<workspace::commit::identity_setup::Review, String>>,
    >,
    review: Option<workspace::commit::identity_setup::Review>,
    applying: Option<workspace::read_only::Request<Result<(), String>>>,
    notice: Option<String>,
    controls: Vec<egui::Id>,
}
impl Changes {
    pub(super) fn identity_setup_ui(&mut self, ui: &mut egui::Ui, workspace: &Workspace) {
        if !workspace.is_git {
            return;
        }
        let sheet_open = self.repo_sheet == Some(RepoSheet::Identity);
        let modal_ok = self.repo_sheet.is_some() || ui.memory(|m| m.top_modal_layer().is_none());
        let s = &mut self.identity_setup;
        s.controls.clear();
        egui::CollapsingHeader::new("Set repository commit identity…")
            .default_open(sheet_open)
            .show(ui, |ui| {
            let enabled = self.index_writable
                && s.loading.is_none()
                && s.pending.is_none()
                && s.applying.is_none()
                && s.review.is_none()
                && !egui::Popup::is_any_open(ui.ctx())
                && modal_ok;
            ui.add_enabled_ui(enabled, |ui| {
                let load = prompt_ui::button(
                    ui,
                    "Load current local identity",
                    prompt_ui::Button::Secondary,
                    true,
                );
                s.controls.push(load.id);
                if load.clicked() {
                    let Some(permit) = branches::permit(ui.ctx()) else {
                        s.notice = Some("Git busy; retry explicitly.".into());
                        return;
                    };
                    let root = workspace.root.clone();
                    let (tx, rx) = mpsc::channel();
                    let cancel = Arc::new(AtomicBool::new(false));
                    s.owner = Some(self.identity.clone());
                    s.loading = Some(workspace::read_only::Request::new(rx, cancel.clone()));
                    let ctx = ui.ctx().clone();
                    if std::thread::Builder::new()
                        .name("pui-identity-load".into())
                        .spawn(move || {
                            let result =
                                workspace::commit::identity_setup::load_local(&root, &cancel);
                            drop(permit);
                            if !cancel.load(Ordering::Acquire) {
                                let _ = tx.send(result);
                                ctx.request_repaint();
                            }
                        })
                        .is_err()
                    {
                        s.loading = None;
                        s.notice = Some("Cannot load local identity.".into());
                    }
                    return;
                }
                for (label, value) in [("Name", &mut s.name), ("Email", &mut s.email)] {
                    ui.label(theme::small(ui, label));
                    let input = ui.add(
                        egui::TextEdit::singleline(value)
                            .char_limit(512)
                            .desired_width(ui.available_width()),
                    );
                    s.controls.push(input.id);
                }
                let edit = ui.checkbox(
                    &mut s.edit_local,
                    "Edit existing direct-local identity (review old → new)",
                );
                s.controls.push(edit.id);
                let toggle = ui.checkbox(
                    &mut s.override_inherited,
                    "Override inherited identity locally (never replace local values)",
                );
                s.controls.push(toggle.id);
                let response = prompt_ui::button(
                    ui,
                    "Review local identity…",
                    prompt_ui::Button::Secondary,
                    workspace::commit::identity_setup::valid(&s.name, &s.email),
                );
                s.controls.push(response.id);
                if response.clicked() {
                    let Some(permit) = branches::permit(ui.ctx()) else {
                        s.notice = Some("Git busy; retry explicitly.".into());
                        return;
                    };
                    let root = workspace.root.clone();
                    let name = s.name.clone();
                    let email = s.email.clone();
                    let allow = s.override_inherited;
                    let edit = s.edit_local;
                    let (tx, rx) = mpsc::channel();
                    let cancel = Arc::new(AtomicBool::new(false));
                    s.owner = Some(self.identity.clone());
                    s.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
                    let ctx = ui.ctx().clone();
                    if std::thread::Builder::new()
                        .name("pui-identity-review".into())
                        .spawn(move || {
                            let result = if edit {
                                workspace::commit::identity_setup::inspect_mode(
                                    &root, &name, &email, false, true, &cancel,
                                )
                            } else if allow {
                                workspace::commit::identity_setup::inspect_override(
                                    &root, &name, &email, true, &cancel,
                                )
                            } else {
                                workspace::commit::identity_setup::inspect(
                                    &root, &name, &email, &cancel,
                                )
                            };
                            drop(permit);
                            if !cancel.load(Ordering::Acquire) {
                                let _ = tx.send(result);
                                ctx.request_repaint();
                            }
                        })
                        .is_err()
                    {
                        s.pending = None;
                        s.notice = Some("Cannot inspect identity setup.".into());
                    }
                }
            });
            if let Some(notice) = &s.notice {
                ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
            }
        });
    }
    pub(super) fn identity_setup_inputs(&mut self, ctx: &egui::Context, visible: bool) {
        let s = &mut self.identity_setup;
        if !visible || !self.index_writable || s.owner.as_ref().is_some_and(|o| o != &self.identity)
        {
            *s = Default::default();
            return;
        }
        if let Some(result) = s.loading.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Local identity load ended; retry explicitly.".into())),
        }) {
            s.loading = None;
            match result {
                Ok((name, email)) => {
                    s.name = name;
                    s.email = email;
                    s.edit_local = true;
                    s.override_inherited = false;
                    s.notice = Some(
                        "Loaded local values into draft only. Edit, then review before saving."
                            .into(),
                    );
                }
                Err(e) => s.notice = Some(e),
            }
        }
        if let Some(result) = s.pending.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Identity review ended.".into())),
        }) {
            s.pending = None;
            match result {
                Ok(r) => s.review = Some(r),
                Err(e) => s.notice = Some(e),
            }
        }
        if let Some(result) = s.applying.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(
                "Identity write uncertain; inspect configuration before retry.".into(),
            )),
        }) {
            s.applying = None;
            s.notice=Some(match result{Ok(())=>"Repository-local name/email saved. Review staged commit again. No commit or network.".into(),Err(e)=>e});
        }
        let focus = ctx.memory(|m| m.focused());
        if s.review.is_some() || focus.is_some_and(|id| s.controls.contains(&id)) {
            ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        }
    }
    pub(super) fn identity_setup_dialog(&mut self, ctx: &egui::Context) {
        let Some(review) = &self.identity_setup.review else {
            return;
        };
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("local-identity-confirm"),
        );
        if !self.index_writable
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.identity_setup.review = None;
            return;
        }
        let choice=confirmations::Dialog::new("local-identity-confirm","Save repository commit identity?","Writes user.name and user.email only to this repository’s shared local Git config. Existing local values are replaced only in explicit Edit mode, shown below. Explicit inherited overrides add local values; inherited/global files stay unchanged. Future commits may publicly expose this identity. No global settings, credentials, commit or network.","Cancel","Save local identity").destructive(false).show(ctx,|ui|{ui.add(egui::Label::new(theme::small(ui,format!("{}\n{} <{}>",review.root.display(),review.name,review.email))).wrap());for(key,value)in &review.inherited{ui.add(egui::Label::new(theme::small(ui,format!("{} {key}: {value} → {}",if review.edit_local{"Previous local"}else{"Inherited"},if key=="user.name"{&review.name}else{&review.email}))).wrap());}});
        if let Some(choice) = choice {
            let review = self.identity_setup.review.take().unwrap();
            if choice == confirmations::Choice::Confirm {
                let Some(permit) = branches::permit(ctx) else {
                    self.identity_setup.notice = Some("Git busy; review again.".into());
                    return;
                };
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.identity_setup.applying =
                    Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-local-identity".into())
                    .spawn(move || {
                        let result = workspace::commit::identity_setup::apply(review, &cancel);
                        drop(permit);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    })
                    .is_err()
                {
                    self.identity_setup.applying = None;
                    self.identity_setup.notice = Some("Cannot start identity write.".into());
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
