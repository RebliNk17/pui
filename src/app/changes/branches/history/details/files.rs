//! Explicit changed-file inspection attached to one exact detail lifetime.
use super::*;
#[derive(Default)]
pub(super) struct Files {
    pending: Option<
        workspace::read_only::Request<Result<Vec<workspace::history::files::Entry>, String>>,
    >,
    pub(super) result: Option<Result<Vec<workspace::history::files::Entry>, String>>,
    pub(super) control: Option<egui::Id>,
    pub(super) key: Option<egui::Id>,
    pub(super) rows: Vec<egui::Id>,
    pub(super) row_key: Option<egui::Id>,
}
impl Files {
    pub(super) fn poll(&mut self) {
        let Some(pending) = &self.pending else {
            return;
        };
        let result = match pending.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Commit file worker ended. Back and select again to retry.".into())
            }
        };
        self.pending = None;
        self.result = Some(result);
    }
    fn start(
        &mut self,
        ctx: &egui::Context,
        owner: &Owner,
        detail: &workspace::history::details::Detail,
        demo: bool,
        base: usize,
    ) {
        if self.pending.is_some() || self.result.is_some() {
            return;
        }
        if demo {
            self.result = Some(Ok(vec![workspace::history::files::Entry {
                status: 'M',
                path: "src/example.rs".into(),
            }]));
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
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        if std::thread::Builder::new()
            .name("pui-commit-files".into())
            .spawn(move || {
                let result =
                    workspace::history::files::read(&root, &hash, parent.as_deref(), &cancel);
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
                "Cannot start commit file worker. Back and select again.".into(),
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
            let response=ui.push_id(("commit-files",&detail.oid,detail.parents.get(base)),|ui|ui.add_enabled(enabled,egui::Button::new(theme::small(ui,"Load changed files")).frame(false))).inner.on_hover_text("Read-only local tree comparison. Uses the explicitly selected parent; root commits compare with empty tree. Renames appear as delete/add. Up to 256 files / 128 KiB; no fetch or checkout.");
            self.control = Some(response.id);
            #[cfg(test)]
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    popup_id().with("commit-files"),
                    (response.id, response.rect),
                )
            });
            if enabled && (response.clicked() || key == Some(response.id)) {
                self.start(ui.ctx(), owner, detail, demo, base);
            }
        }
    }
    pub(super) fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub(super) fn show_result(
        &mut self,
        ui: &mut egui::Ui,
        hash: &str,
        enabled: bool,
    ) -> Option<String> {
        let key = self.row_key.take();
        self.rows.clear();
        let mut selected = None;
        if self.pending.is_some() {
            ui.label(theme::small(ui, "Reading local commit file list…"));
        } else if let Some(result) = &self.result {
            match result {
                Ok(entries) => {
                    ui.label(theme::small(
                        ui,
                        format!(
                            "{} files · selected-parent comparison (empty tree for root) · renames as delete/add · read-only",
                            entries.len()
                        ),
                    ));
                    for entry in entries {
                        let response=ui.push_id(("commit-file-patch",hash,&entry.path),|ui|ui.add_enabled(enabled,
                            egui::Button::new(
                                RichText::new(format!(
                                    "{}  {}",
                                    entry.status,
                                    entry.path.escape_debug()
                                ))
                                .monospace()
                                .size(10.5),
                            )
                            .wrap().frame(false),
                        )).inner.on_hover_text("Load a bounded patch for this exact literal path. No checkout or file write.");
                        self.rows.push(response.id);
                        if enabled && (response.clicked() || key == Some(response.id)) {
                            selected = Some(entry.path.clone());
                        }
                        #[cfg(test)]
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(
                                popup_id().with(("commit-file-patch", &entry.path)),
                                (response.id, response.rect),
                            )
                        });
                    }
                }
                Err(error) => {
                    ui.add(egui::Label::new(theme::small(ui, error)).wrap());
                }
            }
        }
        selected
    }
}
#[cfg(test)]
mod tests;
