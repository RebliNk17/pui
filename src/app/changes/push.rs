//! Explicit local-only destination review. No executable network intent.
#[cfg(test)]
mod tests;
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
pub(super) struct Push {
    owner: Option<(PathBuf, String)>,
    pending: Option<workspace::read_only::Request<Result<workspace::push::Snapshot, String>>>,
    snapshot: Option<workspace::push::Snapshot>,
    notice: Option<String>,
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
                Ok(s) => self.push.snapshot = Some(s),
                Err(e) => self.push.notice = Some(e),
            }
        }
        let enabled = workspace.is_git
            && !loading
            && self.push.pending.is_none()
            && !egui::Popup::is_any_open(ui.ctx())
            && ui.memory(|m| m.top_modal_layer().is_none());
        if prompt_ui::button(
            ui,
            "Inspect push destination…",
            prompt_ui::Button::Secondary,
            enabled,
        )
        .clicked()
        {
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
            if std::thread::Builder::new()
                .name("pui-push-review".into())
                .spawn(move || {
                    let result = workspace::push::prepare(&root, &cancel);
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
        if self.push.pending.is_some() {
            ui.label(theme::small(ui, "Reading local push configuration…"));
        }
        if let Some(error) = &self.push.notice {
            ui.add(egui::Label::new(theme::small(ui, error)).wrap());
        }
        if let Some(snapshot) = &self.push.snapshot {
            ui.add(egui::Label::new(theme::small(ui,format!("Local: {}\nCommit: {}\nRemote: {}\nDestination: {}\nEndpoint: {}\nLocal snapshot only — remote state/authentication not checked. No push performed. Push execution is not available yet.",snapshot.branch,snapshot.head,snapshot.remote,snapshot.destination,snapshot.endpoint))).wrap());
        }
    }
}
