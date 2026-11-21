//! Explicit reviewed clone form; transient intent, no automatic network.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
pub(super) struct Clone {
    owner: Option<(PathBuf, String)>,
    url: String,
    name: String,
    branch: String,
    timeout: Option<u64>,
    pending: Option<workspace::read_only::Request<Result<workspace::clone::Review, String>>>,
    review: Option<workspace::clone::Review>,
    applying: Option<workspace::read_only::Request<Result<(), String>>>,
    notice: Option<String>,
    controls: Vec<egui::Id>,
}
impl Changes {
    pub(super) fn clone_ui(&mut self, ui: &mut egui::Ui, workspace: &Workspace, loading: bool) {
        let s = &mut self.clone;
        s.controls.clear();
        let enabled = self.index_writable
            && !loading
            && s.pending.is_none()
            && s.applying.is_none()
            && s.review.is_none()
            && !egui::Popup::is_any_open(ui.ctx())
            && (self.repo_sheet.is_some() || ui.memory(|m| m.top_modal_layer().is_none()));
        let sheet_open = self.repo_sheet == Some(RepoSheet::Clone);
        ui.add_enabled_ui(enabled, |ui| {
            egui::CollapsingHeader::new("Clone repository…")
                .default_open(sheet_open)
                .show(ui, |ui| {
                    for (label, value, limit) in [
                        ("Repository URL (no embedded credentials)", &mut s.url, 8192),
                        (
                            "Destination folder name (inside current project)",
                            &mut s.name,
                            255,
                        ),
                    ] {
                        ui.label(theme::small(ui, label));
                        let input = ui.add(
                            egui::TextEdit::singleline(value)
                                .char_limit(limit)
                                .desired_width(ui.available_width()),
                        );
                        s.controls.push(input.id);
                    }
                    ui.label(theme::small(
                        ui,
                        "Start branch (optional; blank uses remote default)",
                    ));
                    let branch = ui.add(
                        egui::TextEdit::singleline(&mut s.branch)
                            .char_limit(1000)
                            .desired_width(ui.available_width()),
                    );
                    s.controls.push(branch.id);
                    let valid = workspace::push::remote::valid_url(&s.url)
                        && workspace::clone::valid_dest_name(&s.name)
                        && (s.branch.is_empty()
                            || workspace::branches::create::valid_name(&s.branch));
                    let response =
                        prompt_ui::button(ui, "Review clone…", prompt_ui::Button::Secondary, valid);
                    s.controls.push(response.id);
                    if response.clicked() {
                        let Some(permit) = branches::permit(ui.ctx()) else {
                            s.notice = Some("Git busy; retry explicitly.".into());
                            return;
                        };
                        let root = workspace.root.clone();
                        let url = s.url.clone();
                        let name = s.name.clone();
                        let branch = if s.branch.trim().is_empty() {
                            None
                        } else {
                            Some(s.branch.trim().to_owned())
                        };
                        let (tx, rx) = mpsc::channel();
                        let cancel = Arc::new(AtomicBool::new(false));
                        s.owner = Some(self.identity.clone());
                        s.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
                        let ctx = ui.ctx().clone();
                        if std::thread::Builder::new()
                            .name("pui-clone-review".into())
                            .spawn(move || {
                                let result = workspace::clone::inspect(
                                    &root,
                                    &name,
                                    &url,
                                    branch.as_deref(),
                                    &cancel,
                                );
                                drop(permit);
                                if !cancel.load(Ordering::Acquire) {
                                    let _ = tx.send(result);
                                    ctx.request_repaint();
                                }
                            })
                            .is_err()
                        {
                            s.pending = None;
                            s.notice = Some("Cannot start clone review.".into());
                        }
                    }
                });
            if let Some(notice) = &s.notice {
                ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
            }
        });
    }
    pub(super) fn clone_inputs(&mut self, ctx: &egui::Context, visible: bool) {
        let s = &mut self.clone;
        if !visible || !self.index_writable || s.owner.as_ref().is_some_and(|o| o != &self.identity)
        {
            *s = Default::default();
            return;
        }
        if let Some(result) = s.pending.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Clone review ended.".into())),
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
                "Clone completion uncertain; inspect the destination folder.".into(),
            )),
        }) {
            s.applying = None;
            s.notice = Some(match result {
                Ok(()) => {
                    "Repository cloned into the destination folder. No push, hooks or force.".into()
                }
                Err(e) => e,
            });
            self.refresh = true;
        }
        let focus = ctx.memory(|m| m.focused());
        if s.review.is_some() || focus.is_some_and(|id| s.controls.contains(&id)) {
            ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        }
    }
    pub(super) fn clone_dialog(&mut self, ctx: &egui::Context) {
        let Some(review) = &self.clone.review else {
            return;
        };
        let own = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("clone-confirm"));
        if !self.index_writable
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.clone.review = None;
            return;
        }
        let choice = confirmations::Dialog::new(
            "clone-confirm",
            "Clone repository?",
            "Clones into an empty destination folder under the current project. Uses conservative SSH settings; interactive login and unknown SSH hosts refuse; credentials may still be used by configured helpers. No hooks, force or push. Remote completion can be uncertain.",
            "Cancel",
            "Clone",
        )
        .destructive(false)
        .show(ctx, |ui| {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    format!(
                        "{}\ninto {}\n{}",
                        review.url,
                        review.parent.join(&review.name).display(),
                        review.branch.as_deref().unwrap_or("remote default branch")
                    ),
                ))
                .wrap(),
            );
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    "Clone timeout (seconds, includes network; cleanup may take longer)",
                ))
                .wrap(),
            );
            let timeout = ui.add(
                egui::DragValue::new(self.clone.timeout.get_or_insert(30)).range(1..=300),
            );
            self.clone.controls.push(timeout.id);
        });
        if let Some(choice) = choice {
            let review = self.clone.review.take().unwrap();
            if choice == confirmations::Choice::Confirm {
                let Some(permit) = branches::permit(ctx) else {
                    self.clone.notice = Some("Git busy; review again.".into());
                    return;
                };
                let seconds = self.clone.timeout.unwrap_or(30);
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.clone.applying = Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-clone".into())
                    .spawn(move || {
                        let result = workspace::clone::apply(review, seconds, &cancel);
                        drop(permit);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    })
                    .is_err()
                {
                    self.clone.applying = None;
                    self.clone.notice = Some("Cannot start clone worker.".into());
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
