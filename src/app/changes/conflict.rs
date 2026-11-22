//! Explicit conflict inspection, working-text edit and separately reviewed index resolution.
use super::*;
mod resolution;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
pub(super) struct Conflict {
    owner: Option<(PathBuf, String)>,
    pending: Option<workspace::read_only::Request<Result<workspace::conflict::Review, String>>>,
    review: Option<workspace::conflict::Review>,
    notice: Option<String>,
    side: usize,
    resolution: resolution::Resolution,
}
impl Conflict {
    pub(super) fn begin_rows(&mut self) {
        self.resolution.begin_rows();
    }
}
impl Changes {
    pub(super) fn conflict_row(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        file: &workspace::FileChange,
        loading: bool,
    ) {
        if !matches!(
            file.status.as_str(),
            "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU"
        ) {
            return;
        }
        let s = &mut self.conflict;
        let enabled = self.index_writable
            && !loading
            && workspace.is_git
            && workspace.error.is_none()
            && s.pending.is_none()
            && s.review.is_none()
            && !s.resolution.busy()
            && !egui::Popup::is_any_open(ui.ctx())
            && ui.memory(|m| m.top_modal_layer().is_none());
        if prompt_ui::button(
            ui,
            "Inspect conflict sides…",
            prompt_ui::Button::Secondary,
            enabled,
        )
        .clicked()
        {
            let Some(permit) = branches::permit(ui.ctx()) else {
                s.notice = Some("Git busy; retry.".into());
                return;
            };
            let root = workspace.root.clone();
            let path = file.path.clone();
            s.owner = Some(self.identity.clone());
            let (tx, rx) = mpsc::channel();
            let cancel = Arc::new(AtomicBool::new(false));
            s.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
            let ctx = ui.ctx().clone();
            if std::thread::Builder::new()
                .name("pui-conflict-read".into())
                .spawn(move || {
                    let result = workspace::conflict::inspect(&root, &path, &cancel);
                    drop(permit);
                    if !cancel.load(Ordering::Acquire) {
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    }
                })
                .is_err()
            {
                s.pending = None;
                s.notice = Some("Cannot inspect conflict.".into());
            }
        }
        let resolution_enabled = self.index_writable
            && !loading
            && workspace.is_git
            && workspace.error.is_none()
            && s.pending.is_none()
            && s.review.is_none()
            && !egui::Popup::is_any_open(ui.ctx())
            && ui.memory(|m| m.top_modal_layer().is_none());
        ui.push_id(
            (
                &self.identity,
                &workspace.root,
                &file.path,
                "conflict-resolution",
            ),
            |ui| {
                if s.resolution.row(
                    ui,
                    &workspace.root,
                    &file.path,
                    resolution_enabled,
                    &mut s.notice,
                ) {
                    s.owner = Some(self.identity.clone());
                }
            },
        );
        if let Some(notice) = &s.notice {
            ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
        }
    }
    pub(super) fn conflict_inputs(&mut self, ctx: &egui::Context, visible: bool) {
        let s = &mut self.conflict;
        if !visible || !self.index_writable || s.owner.as_ref().is_some_and(|o| o != &self.identity)
        {
            *s = Default::default();
            return;
        }
        if s.resolution.inputs(ctx, &mut s.notice) {
            self.refresh = true;
        }
        if let Some(result) = s.pending.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Conflict worker ended.".into())),
        }) {
            s.pending = None;
            match result {
                Ok(r) => {
                    s.review = Some(r);
                    s.side = 0;
                }
                Err(e) => s.notice = Some(e),
            }
        }
    }
    pub(super) fn conflict_dialog(&mut self, ctx: &egui::Context) {
        self.conflict.resolution.dialog(
            ctx,
            self.index_writable,
            self.conflict.owner.as_ref() == Some(&self.identity),
            &mut self.conflict.notice,
        );
        let Some(review) = &self.conflict.review else {
            return;
        };
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("git-conflict-inspection"),
        );
        if !self.index_writable
            || self.conflict.owner.as_ref() != Some(&self.identity)
            || egui::Popup::is_any_open(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
            || prompt_ui::modal_owns_current_pass(ctx)
        {
            self.conflict.review = None;
            return;
        }
        let choice=confirmations::Dialog::new("git-conflict-inspection","Index conflict sides","Read-only frozen index stages. Base=1, ours=2, theirs=3. During rebase, ours is the rebased-onto side and theirs is the replayed commit—not necessarily your original branch. No resolution, staging, file write, continue or abort is performed.","Close","Close inspector").destructive(false).show(ctx,|ui|{ui.add(egui::Label::new(theme::small(ui,format!("{}\n{}",review.root.display(),review.path.escape_debug()))).wrap());ui.horizontal_wrapped(|ui|{for(i,label)in ["Base (1)","Ours (2)","Theirs (3)"].iter().enumerate(){ui.selectable_value(&mut self.conflict.side,i,*label);}});if let Some(side)=&review.sides[self.conflict.side]{ui.add(egui::Label::new(theme::small(ui,format!("{} · {}",side.mode,side.oid))).wrap());egui::ScrollArea::both().max_height(220.0).show_rows(ui,16.0,side.text.lines().count(),|ui,range|{for line in side.text.lines().skip(range.start).take(range.len()){ui.add(egui::Label::new(RichText::new(line).monospace().size(10.5)).extend().selectable(false));}});}else{ui.label(theme::small(ui,"This side is absent (for example an add/delete conflict)."));}});
        if choice.is_some() {
            self.conflict.review = None;
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
