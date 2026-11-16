//! Explicit hookless local staged-snapshot commit. Never auto push.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
pub(super) struct Commit {
    owner: Option<(PathBuf, String)>,
    pending: Option<workspace::read_only::Request<Result<workspace::commit::Snapshot, String>>>,
    applying: Option<workspace::read_only::Request<Result<String, String>>>,
    snapshot: Option<workspace::commit::Snapshot>,
    message: String,
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
        if let Some(notice) = &self.commit.notice {
            ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
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
            self.commit = Default::default();
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
                    self.commit.message.clear();
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
                    format!("Committed locally: {oid}. No push. Hooks and signing were not run.")
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
        let choice=confirmations::Dialog::new("commit-staged-confirm",if initial{"Create initial staged commit?"}else{"Commit staged snapshot?"},"Creates a local, unsigned commit without running Git hooks. Uses configured Git author/committer identity. Only the reviewed index snapshot is committed; working files stay unchanged. No push. Use Terminal if hooks or signing are required.","Cancel","Commit without hooks").destructive(false).enabled(workspace::commit::valid_message(&self.commit.message)).show(ctx,|ui|{ui.add(egui::Label::new(theme::small(ui,format!("{}\n{}\nAuthor: {} <{}>\nCommitter: {} <{}>",snapshot.root.display(),snapshot.branch,snapshot.author.name,snapshot.author.email,snapshot.committer.name,snapshot.committer.email))).wrap());let response=ui.add(egui::TextEdit::multiline(&mut self.commit.message).hint_text("Commit message…").char_limit(4096).desired_rows(3).desired_width(ui.available_width()));self.commit.controls.push(response.id);
        ui.label(theme::small(ui,format!("{} staged files · renames shown as delete/add",snapshot.files.len())));
        egui::ScrollArea::both().id_salt("commit-file-list").max_height(72.0).show_rows(ui,16.0,snapshot.files.len(),|ui,range|{for entry in &snapshot.files[range]{ui.add(egui::Label::new(theme::small(ui,format!("{}  {}",entry.status,entry.path.escape_debug()))).extend());}});
        egui::ScrollArea::both().id_salt("commit-staged-patch").max_height(180.0).show_rows(ui,16.0,snapshot.preview.lines().count(),|ui,range|{for line in snapshot.preview.lines().skip(range.start).take(range.len()){ui.add(egui::Label::new(RichText::new(line).monospace().size(10.5)).extend().selectable(false));}});});
        if let Some(choice) = choice {
            let snapshot = self.commit.snapshot.take().unwrap();
            if choice == confirmations::Choice::Confirm {
                let Some(permit) = branches::permit(ctx) else {
                    self.commit.notice = Some("Git busy; inspect again.".into());
                    return;
                };
                let message = std::mem::take(&mut self.commit.message);
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.commit.applying = Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-commit-publish".into())
                    .spawn(move || {
                        let result = workspace::commit::apply(snapshot, &message, &cancel);
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
