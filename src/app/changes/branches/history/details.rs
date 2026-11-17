//! One exact selected commit; no diff or execution actions, no persisted intent.
use super::*;
mod comparison;
mod files;
mod patch;
#[derive(Default)]
pub(super) struct Details {
    selected: Option<String>,
    pending:
        Option<workspace::read_only::Request<Result<workspace::history::details::Detail, String>>>,
    result: Option<Result<workspace::history::details::Detail, String>>,
    copy_controls: [Option<egui::Id>; 2],
    copy_key: Option<egui::Id>,
    branch_control: Option<egui::Id>,
    branch_key: Option<egui::Id>,
    parent_controls: Vec<(egui::Id, usize)>,
    parent_key: Option<egui::Id>,
    parent_hops: u8,
    list_origin: Option<String>,
    files: files::Files,
    patch: patch::Patch,
    comparison: comparison::Comparison,
    #[cfg(test)]
    requests: usize,
}
impl Details {
    fn clear(&mut self) {
        *self = Self::default();
    }
    pub(super) fn poll(&mut self) {
        self.files.poll();
        self.patch.poll();
        let result = self
            .pending
            .as_ref()
            .and_then(|pending| match pending.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Commit detail worker ended without a result. Back and select again to retry."
                        .into(),
                )),
            });
        if let Some(result) = result {
            self.pending = None;
            self.result = Some(result.and_then(|detail| {
                if self.selected.as_deref() == Some(&detail.oid) {
                    Ok(detail)
                } else {
                    Err("Commit detail identity changed. Back and select again.".into())
                }
            }));
        }
    }
    fn start(&mut self, ctx: &egui::Context, owner: &Owner, hash: &str, demo: bool) {
        self.clear();
        self.selected = Some(hash.into());
        if demo {
            self.result = Some(Ok(workspace::history::details::Detail {oid:hash.into(),time:1_700_000_000,parents:if hash=="a".repeat(40){vec!["b".repeat(40),"c".repeat(40)]}else{vec![]},
                message:"Synthetic read-only history preview\n\nInspect the original local commit message.\nNo checkout, patch, rebase, context capture or send.\n".into()}));
            return;
        }
        #[cfg(test)]
        {
            self.requests += 1;
        }
        let Some(permit) = permit(ctx) else {
            self.result = Some(Err(
                "A local Git inspection is still finishing. Back and select again to retry.".into(),
            ));
            return;
        };
        let hash = hash.to_owned();
        let root = owner.root.clone();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        if std::thread::Builder::new()
            .name("pui-commit-detail".into())
            .spawn(move || {
                let result = workspace::history::details::read(&root, &hash, &cancel);
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
                "Cannot start commit detail worker. Back and select again.".into(),
            ));
        }
    }
}
impl History {
    pub(in crate::app::changes::branches) fn has_detail(&self) -> bool {
        self.detail.selected.is_some()
    }
    pub(in crate::app::changes::branches) fn detail_inputs(
        &mut self,
        focused: Option<egui::Id>,
        key: egui::Key,
    ) {
        if !self.active {
            return;
        }
        if focused.is_some() && focused == self.mode_control && !self.has_detail() {
            self.mode_key = true;
            return;
        }
        if self.has_detail()
            && focused.is_some()
            && (self.detail.patch.viewer.controls.contains(&focused)
                || self
                    .detail
                    .patch
                    .viewer
                    .fold_controls
                    .iter()
                    .any(|id| Some(*id) == focused))
        {
            self.detail.patch.viewer.key = focused;
            return;
        }
        if self.has_detail()
            && focused.is_some()
            && self
                .detail
                .comparison
                .controls
                .iter()
                .any(|id| Some(*id) == focused)
        {
            self.detail.comparison.key = focused;
            return;
        }
        if self.has_detail()
            && focused.is_some()
            && self.detail.files.rows.iter().any(|id| Some(*id) == focused)
        {
            self.detail.files.row_key = focused;
            return;
        }
        if self.has_detail() && focused.is_some() && focused == self.detail.patch.control {
            self.detail.patch.key = focused;
            return;
        }
        if self.has_detail() && focused.is_some() && focused == self.detail.files.control {
            self.detail.files.key = focused;
            return;
        }
        if self.has_detail()
            && focused.is_some()
            && self
                .detail
                .parent_controls
                .iter()
                .any(|(id, _)| Some(*id) == focused)
        {
            self.detail.parent_key = focused;
            return;
        }
        if self.has_detail() && focused.is_some() && focused == self.detail.branch_control {
            self.detail.branch_key = focused;
            return;
        }
        if self.has_detail() && focused.is_some() && self.detail.copy_controls.contains(&focused) {
            self.detail.copy_key = focused;
            return;
        }
        if self.detail_back == focused && focused.is_some() {
            self.detail_back_key = true;
            return;
        }
        if matches!(key, egui::Key::Enter | egui::Key::Space)
            && let Some((_, index)) = self
                .commit_controls
                .iter()
                .find(|(id, _)| Some(*id) == focused)
        {
            self.detail_key = self
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.commits.get(*index))
                .filter(|commit| focused == Some(popup_id().with(("history-commit", &commit.oid))))
                .map(|commit| commit.oid.clone());
        }
    }
    pub(super) fn open_detail(&mut self, ctx: &egui::Context, hash: &str, demo: bool) -> bool {
        if self.pending.is_some()
            || self.owner.is_none()
            || self
                .snapshot
                .as_ref()
                .is_none_or(|snapshot| !snapshot.commits.iter().any(|commit| commit.oid == hash))
        {
            return false;
        }
        self.detail
            .start(ctx, self.owner.as_ref().unwrap(), hash, demo);
        self.detail.list_origin = Some(hash.into());
        self.commit_controls.clear();
        self.detail_back = None;
        self.detail_key = None;
        self.detail_focus = true;
        ctx.request_repaint();
        true
    }
    fn open_parent(&mut self, ctx: &egui::Context, parent: &str, demo: bool) -> bool {
        if self.pending.is_some()
            || self.owner.is_none()
            || self.detail.pending.is_some()
            || self.detail.parent_hops >= 32
        {
            return false;
        }
        let valid = self.detail.result.as_ref().is_some_and(|result| {
            result.as_ref().is_ok_and(|detail| {
                Some(detail.oid.as_str()) == self.detail.selected.as_deref()
                    && detail.parents.iter().any(|oid| oid == parent)
            })
        });
        if !valid {
            return false;
        }
        let hops = self.detail.parent_hops + 1;
        let origin = self.detail.list_origin.take();
        self.detail
            .start(ctx, self.owner.as_ref().unwrap(), parent, demo);
        self.detail.parent_hops = hops;
        self.detail.list_origin = origin;
        self.detail_back = None;
        self.detail_focus = true;
        ctx.request_repaint();
        true
    }
    pub(super) fn close_detail(&mut self, ctx: &egui::Context) {
        let selected = self
            .detail
            .list_origin
            .take()
            .or_else(|| self.detail.selected.take());
        self.detail.clear();
        self.detail_key = None;
        self.detail_back = None;
        self.detail_back_key = false;
        self.detail_focus = false;
        if let Some(hash) = selected {
            ctx.memory_mut(|memory| {
                memory.request_focus(popup_id().with(("history-commit", hash)))
            });
        }
        ctx.request_repaint();
    }
    pub(super) fn show_detail(&mut self, ui: &mut egui::Ui, demo: bool) -> bool {
        if !self.has_detail() {
            return false;
        }
        let copy_key = self.detail.copy_key.take();
        let branch_key = self.detail.branch_key.take();
        self.detail.branch_control = None;
        let parent_key = self.detail.parent_key.take();
        self.detail.parent_controls.clear();
        let mut inspect_parent = None;
        self.detail.copy_controls = [None; 2];
        let back = ui.add(egui::Button::new(theme::small(ui, "Back to history")).frame(false));
        self.detail_back = Some(back.id);
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(popup_id().with("detail-back"), (back.id, back.rect))
        });
        if std::mem::take(&mut self.detail_focus) {
            back.request_focus();
        }
        if back.clicked() || std::mem::take(&mut self.detail_back_key) {
            self.close_detail(ui.ctx());
            return true;
        }
        let hash = self.detail.selected.as_ref().unwrap();
        ui.add(
            egui::Label::new(
                RichText::new(hash)
                    .monospace()
                    .size(10.5)
                    .color(theme::palette(ui.ctx()).dim),
            )
            .wrap(),
        );
        if self.detail.pending.is_some() {
            ui.label(theme::small(ui, "Reading local commit message…"));
        } else if let Some(result) = &self.detail.result {
            match result {
                Ok(detail) => {
                    // Full object identity is part of each widget, never just row position.
                    // Pending/error/mismatched bodies have no clipboard controls.
                    if detail.oid == *hash {
                        if self.detail.comparison.show(ui, detail) {
                            self.detail.files = Default::default();
                            self.detail.patch = Default::default();
                        }
                        ui.horizontal_wrapped(|ui| {
                            for (index, label) in
                                ["Copy ID", "Copy message"].into_iter().enumerate()
                            {
                                let response = ui
                                    .push_id(("commit-copy", hash, index), |ui| {
                                        ui.add(
                                            egui::Button::new(theme::small(ui, label)).frame(false),
                                        )
                                    })
                                    .inner;
                                self.detail.copy_controls[index] = Some(response.id);
                                #[cfg(test)]
                                ui.ctx().data_mut(|data| {
                                    data.insert_temp(
                                        popup_id().with(("commit-copy", index)),
                                        (response.id, response.rect),
                                    )
                                });
                                if response.clicked() || copy_key == Some(response.id) {
                                    ui.ctx().copy_text(
                                        if index == 0 {
                                            &detail.oid
                                        } else {
                                            &detail.message
                                        }
                                        .clone(),
                                    );
                                }
                            }
                            if let Some(owner) = &self.owner {
                                self.detail.files.show_control(
                                    ui,
                                    owner,
                                    detail,
                                    demo,
                                    !self.detail.patch.pending(),
                                    self.detail.comparison.base,
                                );
                                self.detail.patch.show_control(
                                    ui,
                                    owner,
                                    detail,
                                    demo,
                                    !self.detail.files.pending(),
                                    self.detail.comparison.base,
                                );
                            }
                        });
                    }
                    if let Some(path) =
                        self.detail
                            .files
                            .show_result(ui, hash, !self.detail.patch.pending())
                        && detail.oid == *hash
                        && let Some(owner) = &self.owner
                    {
                        self.detail.patch.select_file(
                            ui.ctx(),
                            owner,
                            detail,
                            &path,
                            demo,
                            self.detail.comparison.base,
                        );
                    }
                    let date = chrono::DateTime::from_timestamp(detail.time, 0)
                        .map(|date| date.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                        .unwrap_or_else(|| "Unknown date".into());
                    ui.label(theme::small(ui, format!("Committer date · {date}")));
                    ui.label(theme::small(
                        ui,
                        match detail.parents.len() {
                            0 => "Root commit · no parents".into(),
                            1 => "1 parent".into(),
                            count => format!("Merge commit · {count} parents"),
                        },
                    ));
                    for (index, parent) in detail.parents.iter().enumerate() {
                        let label = format!(
                            "{} {}",
                            if index == 0 { "First parent" } else { "Parent" },
                            parent
                        );
                        let available = detail.oid == *hash && self.detail.parent_hops < 32;
                        if available {
                            let response=ui.push_id(("inspect-parent",hash,parent),|ui|ui.add(egui::Button::new(RichText::new(label).monospace().size(10.5)).frame(false).wrap())).inner.on_hover_text("Inspect this exact parent from the loaded commit, even outside the history list. Local objects only; missing objects report an error. No fetch, checkout or diff-base change. Back returns to the list. Up to 32 parent steps per inspection.");
                            self.detail.parent_controls.push((response.id, index));
                            if response.clicked() || parent_key == Some(response.id) {
                                inspect_parent = Some(parent.clone());
                            }
                            #[cfg(test)]
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(
                                    popup_id().with(("inspect-parent", index)),
                                    (response.id, response.rect),
                                )
                            });
                        } else {
                            ui.add(egui::Label::new(RichText::new(label).monospace().size(10.5).color(theme::palette(ui.ctx()).dim)).wrap().selectable(false)).on_hover_text("Parent inspection limit reached (32 steps). Back returns to the retained list; no objects are fetched.");
                        }
                    }
                    self.detail.patch.show_result(ui);
                    ui.add(
                        egui::Label::new(theme::small(
                            ui,
                            "Local commit message · untrusted repository text",
                        ))
                        .wrap(),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(if detail.message.is_empty() {
                                "(empty commit message)"
                            } else {
                                &detail.message
                            })
                            .size(12.0),
                        )
                        .wrap()
                        .selectable(false),
                    );
                    if detail.oid == *hash && !demo {
                        let prepare = ui
                            .push_id(("history-branch-draft", hash), |ui| {
                                prompt_ui::button(
                                    ui,
                                    "Prepare branch here…",
                                    prompt_ui::Button::Secondary,
                                    self.branch_enabled && !demo,
                                )
                            })
                            .inner;
                        self.detail.branch_control = Some(prepare.id);
                        #[cfg(test)]
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(
                                popup_id().with("history-branch-draft"),
                                (prepare.id, prepare.rect, prepare.enabled()),
                            )
                        });
                        if prepare.enabled()
                            && (prepare.clicked() || branch_key == Some(prepare.id))
                        {
                            self.branch_draft = Some(detail.oid.clone());
                        }
                    }
                }
                Err(error) => {
                    ui.add(egui::Label::new(theme::small(ui, error)).wrap());
                }
            }
        }
        if let Some(parent) = inspect_parent {
            self.open_parent(ui.ctx(), &parent, demo);
        }
        true
    }
}
impl Changes {
    pub(in crate::app) fn demo_commit_detail(&mut self, ctx: &egui::Context) {
        if self.branches.demo
            && let Some(hash) = self
                .branches
                .history
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.head.clone())
        {
            self.branches.history.open_detail(ctx, &hash, true);
        }
    }
}
#[cfg(test)]
mod branch_tests;
#[cfg(test)]
mod copy_tests;
#[cfg(test)]
mod parent_tests;
#[cfg(test)]
mod tests;
