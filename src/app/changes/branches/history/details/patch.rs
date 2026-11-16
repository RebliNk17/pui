//! One explicit literal patch per selected detail. No persisted intent.
use super::*;
mod viewer;
#[derive(Default)]
pub(super) struct Patch {
    pending: Option<workspace::read_only::Request<Result<String, String>>>,
    result: Option<Result<String, String>>,
    pub(super) control: Option<egui::Id>,
    pub(super) key: Option<egui::Id>,
    path: Option<String>,
    pub(super) viewer: viewer::Viewer,
}
impl Patch {
    pub(super) fn poll(&mut self) {
        let Some(pending) = &self.pending else {
            return;
        };
        let result = match pending.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Commit patch worker ended. Back and select again to retry.".into())
            }
        };
        self.pending = None;
        if let Ok(text) = &result {
            self.viewer = viewer::Viewer::new(text);
        }
        self.result = Some(result);
    }
    fn start(
        &mut self,
        ctx: &egui::Context,
        owner: &Owner,
        detail: &workspace::history::details::Detail,
        demo: bool,
        path: Option<&str>,
        base: usize,
    ) {
        if self.pending.is_some() || self.result.is_some() {
            return;
        }
        self.path = path.map(str::to_owned);
        if demo {
            self.result=Some(Ok("diff --git a/example.rs b/example.rs\n--- a/example.rs\n+++ b/example.rs\n@@ -1 +1 @@\n-old\n+new\n".into()));
            self.viewer = viewer::Viewer::new(self.result.as_ref().unwrap().as_ref().unwrap());
            return;
        }
        let Some(permit) = permit(ctx) else {
            self.result = Some(Err(
                "A local Git inspection is still finishing. Back and select again to retry.".into(),
            ));
            return;
        };
        let root = owner.root.clone();
        let hash = detail.oid.clone();
        let parent = detail.parents.get(base).cloned();
        let path = self.path.clone();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        if std::thread::Builder::new()
            .name("pui-commit-patch".into())
            .spawn(move || {
                let result = if let Some(path) = path {
                    workspace::history::patch::read_path(
                        &root,
                        &hash,
                        parent.as_deref(),
                        Some(&path),
                        &cancel,
                    )
                } else {
                    workspace::history::patch::read(&root, &hash, parent.as_deref(), &cancel)
                };
                drop(permit);
                if !cancel.load(Ordering::Acquire) {
                    let _ = tx.send(result);
                    ctx.request_repaint();
                }
            })
            .is_err()
        {
            self.pending = None;
            self.result = Some(Err(
                "Cannot start commit patch worker. Back and select again.".into(),
            ));
        }
    }
    pub(super) fn show_control(
        &mut self,
        ui: &mut egui::Ui,
        owner: &Owner,
        detail: &workspace::history::details::Detail,
        demo: bool,
        enabled: bool,
        base: usize,
    ) {
        let key = self.key.take();
        self.control = None;
        if self.pending.is_none() && self.result.is_none() {
            let response=ui.push_id(("commit-patch",&detail.oid,detail.parents.get(base)),|ui|ui.add_enabled(enabled,egui::Button::new(theme::small(ui,"Load patch")).frame(false))).inner.on_hover_text("Read-only local patch. Selected parent for merges, empty tree for roots. Up to 32 KiB / 1,024 lines, whole-or-reject. No fetch, checkout or file edits. Binary content is not decoded.");
            self.control = Some(response.id);
            #[cfg(test)]
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    popup_id().with("commit-patch"),
                    (response.id, response.rect),
                )
            });
            if enabled && (response.clicked() || key == Some(response.id)) {
                self.start(ui.ctx(), owner, detail, demo, None, base);
            }
        }
    }
    pub(super) fn select_file(
        &mut self,
        ctx: &egui::Context,
        owner: &Owner,
        detail: &workspace::history::details::Detail,
        path: &str,
        demo: bool,
        base: usize,
    ) {
        if self.pending.is_some() {
            return;
        }
        *self = Self::default();
        self.start(ctx, owner, detail, demo, Some(path), base);
    }
    #[cfg(test)]
    pub(super) fn set_pending_for_test(
        &mut self,
        rx: mpsc::Receiver<Result<String, String>>,
        cancel: Arc<AtomicBool>,
    ) {
        self.pending = Some(workspace::read_only::Request::new(rx, cancel));
    }
    pub(super) fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub(super) fn show_result(&mut self, ui: &mut egui::Ui) {
        if self.pending.is_some() {
            ui.label(theme::small(ui, "Reading local commit patch…"));
        } else if let Some(result) = &self.result {
            match result {
                Ok(text) => {
                    if let Some(path) = &self.path {
                        ui.add(
                            egui::Label::new(theme::small(
                                ui,
                                format!("File patch · {}", path.escape_debug()),
                            ))
                            .wrap(),
                        );
                    }
                    ui.label(theme::small(ui,"Commit patch · selected parent (empty tree for root) · untrusted repository text · read-only"));
                    self.viewer.show(ui, text);
                }
                Err(error) => {
                    ui.add(egui::Label::new(theme::small(ui, error)).wrap());
                }
            }
        }
    }
}
#[cfg(test)]
mod tests;
