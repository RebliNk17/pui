//! Explicit non-Git project initialization form; no automatic staging.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
pub(super) struct Initialize {
    owner: Option<(PathBuf, String)>,
    branch: String,
    review: Option<workspace::initialize::Review>,
    pending: Option<workspace::read_only::Request<Result<workspace::initialize::Review, String>>>,
    applying: Option<workspace::read_only::Request<Result<(), String>>>,
    notice: Option<String>,
    controls: Vec<egui::Id>,
}
impl Changes {
    pub(super) fn initialize_ui(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        loading: bool,
    ) {
        if workspace.is_git {
            return;
        }
        let s = &mut self.initialize;
        s.controls.clear();
        ui.label(theme::small(
            ui,
            "This project is not a Git repository. Initialization does not stage or publish files.",
        ));
        let enabled = self.index_writable
            && !loading
            && s.pending.is_none()
            && s.applying.is_none()
            && s.review.is_none()
            && !egui::Popup::is_any_open(ui.ctx())
            && (self.repo_sheet.is_some() || ui.memory(|m| m.top_modal_layer().is_none()));
        ui.add_enabled_ui(enabled, |ui| {
            let input = ui.add(
                egui::TextEdit::singleline(&mut s.branch)
                    .hint_text("Initial branch (for example main)")
                    .char_limit(1000)
                    .desired_width(ui.available_width()),
            );
            s.controls.push(input.id);
            let button = prompt_ui::button(
                ui,
                "Review Git initialization…",
                prompt_ui::Button::Secondary,
                workspace::branches::create::valid_name(&s.branch),
            );
            s.controls.push(button.id);
            if button.clicked() {
                let Some(permit) = branches::permit(ui.ctx()) else {
                    s.notice = Some("Git busy; retry explicitly.".into());
                    return;
                };
                let root = self.identity.0.clone();
                let branch = s.branch.clone();
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                s.owner = Some(self.identity.clone());
                s.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ui.ctx().clone();
                if std::thread::Builder::new()
                    .name("pui-init-review".into())
                    .spawn(move || {
                        let result = workspace::initialize::inspect(&root, &branch, &cancel);
                        drop(permit);
                        if !cancel.load(Ordering::Acquire) {
                            let _ = tx.send(result);
                            ctx.request_repaint();
                        }
                    })
                    .is_err()
                {
                    s.pending = None;
                    s.notice = Some("Cannot start initialization review.".into());
                }
            }
        });
        if let Some(notice) = &s.notice {
            ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
        }
    }
    pub(super) fn initialize_inputs(&mut self, ctx: &egui::Context, visible: bool) {
        let s = &mut self.initialize;
        if !visible || !self.index_writable || s.owner.as_ref().is_some_and(|o| o != &self.identity)
        {
            *s = Default::default();
            return;
        }
        if let Some(result) = s.pending.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Initialization review ended.".into())),
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
                "Initialization completion uncertain; inspect folder.".into()
            )),
        }) {
            s.applying = None;
            s.notice = Some(match result {
                Ok(()) => "Repository initialized. No files staged, committed or pushed.".into(),
                Err(e) => e,
            });
            self.refresh = true;
        }
        let focus = ctx.memory(|m| m.focused());
        if s.review.is_some() || focus.is_some_and(|id| s.controls.contains(&id)) {
            ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        }
    }
    pub(super) fn initialize_dialog(&mut self, ctx: &egui::Context) {
        let Some(review) = &self.initialize.review else {
            return;
        };
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("initialize-git-confirm"),
        );
        if !self.index_writable
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.initialize.review = None;
            return;
        }
        let choice=confirmations::Dialog::new("initialize-git-confirm","Initialize Git in this folder?","Creates .git metadata with the selected initial branch and no copied hook templates. Existing project files stay. No staging, commit, remote or network. Interrupted initialization can leave partial metadata; it is never deleted automatically.","Cancel","Initialize Git").destructive(false).show(ctx,|ui|{ui.add(egui::Label::new(theme::small(ui,format!("{}\n{}",review.root.display(),review.branch))).wrap());});
        if let Some(choice) = choice {
            let review = self.initialize.review.take().unwrap();
            if choice == confirmations::Choice::Confirm {
                let Some(permit) = branches::permit(ctx) else {
                    self.initialize.notice = Some("Git busy; review again.".into());
                    return;
                };
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.initialize.applying =
                    Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-init-git".into())
                    .spawn(move || {
                        let result = workspace::initialize::apply(review, &cancel);
                        drop(permit);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    })
                    .is_err()
                {
                    self.initialize.applying = None;
                    self.initialize.notice = Some("Cannot start initialization.".into());
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
