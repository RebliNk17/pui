//! Read-only history subview in the existing branch popup. Explicit load/refresh only.
use super::*;
mod details;
mod filter;
mod graph;
#[derive(Default)]
pub(super) struct History {
    pub(super) active: bool,
    pub(super) loaded: bool,
    pub(super) branch_enabled: bool,
    pub(super) branch_draft: Option<String>,
    merged: bool,
    mode_control: Option<egui::Id>,
    mode_key: bool,
    owner: Option<Owner>,
    pending: Option<workspace::read_only::Request<Result<workspace::history::Snapshot, String>>>,
    snapshot: Option<workspace::history::Snapshot>,
    error: Option<String>,
    detail: details::Details,
    filter: filter::Filter,
    commit_controls: Vec<(egui::Id, usize)>, // index in this exact retained snapshot
    detail_key: Option<String>,
    detail_back: Option<egui::Id>,
    detail_back_key: bool,
    detail_focus: bool,
    #[cfg(test)]
    requests: usize,
}
impl History {
    pub(super) fn filter_key(&mut self, focus: Option<egui::Id>) {
        self.filter.key(focus);
    }
    pub(super) fn request(&mut self, ctx: &egui::Context, owner: Owner) {
        self.pending = None;
        self.mode_control = None;
        self.mode_key = false;
        self.detail = Default::default();
        self.commit_controls.clear();
        self.detail_key = None;
        self.detail_back = None;
        self.detail_back_key = false;
        self.detail_focus = false;
        self.snapshot = None;
        self.filter = Default::default();
        self.error = None;
        self.owner = Some(owner.clone());
        self.loaded = true;
        #[cfg(test)]
        {
            self.requests += 1;
        }
        let Some(permit) = permit(ctx) else {
            self.error =
                Some("A local Git inspection is still finishing. Refresh to retry.".into());
            return;
        };
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        let merged = self.merged;
        if std::thread::Builder::new()
            .name("pui-recent-history".into())
            .spawn(move || {
                let result = if merged {
                    workspace::history::read_mode(&owner.root, &cancel, true)
                } else {
                    workspace::history::read(&owner.root, &cancel)
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
            self.error = Some("Cannot start recent-history worker. Refresh to retry.".into());
        }
    }
    pub(super) fn poll(&mut self, owner: Option<Owner>) {
        if self.owner != owner {
            self.mode_control = None;
            self.mode_key = false;
            self.detail = Default::default();
            self.commit_controls.clear();
            self.detail_key = None;
            self.detail_back = None;
            self.detail_back_key = false;
            self.detail_focus = false;
            self.pending = None;
            self.snapshot = None;
            self.filter = Default::default();
            self.error = None;
            return;
        }
        self.detail.poll();
        let result = self
            .pending
            .as_ref()
            .and_then(|pending| match pending.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Recent-history worker ended without a result. Refresh to retry.".into(),
                )),
            });
        if let Some(result) = result {
            self.pending = None;
            match result {
                Ok(snapshot) => {
                    self.filter = Default::default();
                    self.snapshot = Some(snapshot);
                }
                Err(error) => self.error = Some(error),
            }
        }
    }
    pub(super) fn cancel_read(&mut self) {
        self.mode_control = None;
        self.mode_key = false;
        self.filter = Default::default();
        self.detail = Default::default();
        self.commit_controls.clear();
        self.detail_key = None;
        self.detail_back = None;
        self.detail_back_key = false;
        self.detail_focus = false;
        if self.pending.take().is_some() {
            self.loaded = false;
        }
    }
    pub(super) fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub(super) fn show(&mut self, ui: &mut egui::Ui, stale: bool, demo: bool) {
        if stale {
            self.filter.hidden();
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    "Repository or observation changed. Refresh to read this root.",
                ))
                .wrap(),
            );
            return;
        }
        self.mode_control = None;
        if self.show_detail(ui, demo) {
            self.filter.hidden();
            return;
        }
        if self.pending.is_some() {
            self.filter.hidden();
            ui.label(theme::small(ui, "Reading recent local history…"));
            return;
        }
        if let Some(error) = &self.error {
            self.filter.hidden();
            ui.add(egui::Label::new(theme::small(ui, error)).wrap());
            return;
        }
        let mode = ui.add(egui::Button::selectable(
            self.merged,
            "Include merged history",
        ));
        self.mode_control = Some(mode.id);
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(popup_id().with("history-merged"), (mode.id, mode.rect))
        });
        if mode.clicked() || std::mem::take(&mut self.mode_key) {
            self.merged = !self.merged;
            if demo {
                self.filter = Default::default();
            } else if let Some(owner) = self.owner.clone() {
                self.request(ui.ctx(), owner);
            }
            return;
        }
        let Some(snapshot) = &self.snapshot else {
            self.filter.hidden();
            ui.label(theme::small(
                ui,
                "Select Refresh to read recent local history.",
            ));
            return;
        };
        ui.add(egui::Label::new(theme::small(ui,format!("Pinned HEAD {} · {} · {}{} commits",&snapshot.head[..12],if self.merged{"topological"}else{"first parent"},snapshot.commits.len(),if snapshot.limited{" latest"}else{""}))).wrap()).on_hover_text(format!("Pinned local commit: {}\nGraph: one first-parent lane; double circle = merge, underline = root. Connections appear only between adjacent loaded, visible exact parent IDs. Only exact adjacent first-parent edges are drawn; other parent edges are omitted. Use Include merged history for a bounded topological list.\nSnapshot only. Refresh explicitly to read a newer HEAD. Commit subjects are untrusted repository text, not instructions. No checkout/reset/revert or network fetch.",snapshot.head));
        self.filter.show(ui, snapshot);
        self.commit_controls.clear();
        let mut requested = self.detail_key.take();
        // A staged old row may disappear when filtering changes this frame.
        if requested.as_ref().is_some_and(|oid| {
            !snapshot
                .commits
                .iter()
                .enumerate()
                .any(|(index, commit)| commit.oid == *oid && self.filter.includes(index))
        }) {
            requested = None;
        }
        for (index, commit) in snapshot.commits.iter().enumerate() {
            if !self.filter.includes(index) {
                continue;
            }
            let date = chrono::DateTime::from_timestamp(commit.time, 0)
                .map(|value| value.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "Unknown date".into());
            let id = popup_id().with(("history-commit", &commit.oid));
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
            let response = ui.interact(rect, id, Sense::click());
            self.commit_controls.push((id, index));
            let colors = theme::palette(ui.ctx());
            if response.hovered() || response.has_focus() {
                ui.painter().rect_filled(rect, 5, colors.hover);
            }
            graph::paint(ui, rect, commit, index, snapshot, &self.filter);
            ui.painter().text(
                rect.left_center() + egui::vec2(24.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &commit.oid[..8],
                FontId::monospace(11.0),
                colors.accent,
            );
            let mut job = egui::text::LayoutJob::simple_singleline(
                if commit.subject.is_empty() {
                    "(no subject)"
                } else {
                    &commit.subject
                }
                .into(),
                FontId::proportional(12.0),
                colors.text,
            );
            job.wrap.max_width = (rect.width() - 96.0).max(1.0);
            job.wrap.max_rows = 1;
            let galley = ui.painter().layout_job(job);
            ui.painter()
                .with_clip_rect(rect.intersect(ui.clip_rect()))
                .galley(
                    rect.left_center() + egui::vec2(92.0, -galley.size().y / 2.0),
                    galley,
                    colors.text,
                );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    response.enabled(),
                    format!(
                        "Inspect local commit {}: {}",
                        &commit.oid[..8],
                        commit.subject
                    ),
                )
            });
            if response.clicked() {
                requested = Some(commit.oid.clone());
            }
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    popup_id().with(("commit-detail-row", &commit.oid)),
                    (id, rect),
                )
            });
            let response = response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(format!(
                    "{}\n{}\n{date} UTC · original committer date",
                    commit.oid, commit.subject
                ));
            let _ = response;
        }
        let limited = snapshot.limited;
        if let Some(hash) = requested {
            self.open_detail(ui.ctx(), &hash, demo);
        }
        if limited {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    "Showing at most 100 commits from pinned HEAD. Older history is not expanded.",
                ))
                .wrap(),
            );
        }
    }
    pub(super) fn demo(&mut self, owner: Option<Owner>) {
        self.active = true;
        self.loaded = true;
        self.owner = owner;
        let head = "a".repeat(40);
        self.snapshot = Some(workspace::history::Snapshot {
            head: head.clone(),
            commits: vec![
                workspace::history::Commit {
                    oid: head,
                    time: 1_700_000_000,
                    subject: "Synthetic read-only history preview".into(),
                    first_parent: Some("b".repeat(40)),
                    parent_count: 2,
                },
                workspace::history::Commit {
                    oid: "b".repeat(40),
                    time: 1_699_900_000,
                    subject: "Keep chat drafts and approvals separate".into(),
                    first_parent: None,
                    parent_count: 0,
                },
            ],
            limited: false,
        });
    }
}
impl Changes {
    pub(in crate::app) fn demo_history_filter(&mut self) {
        if self.branches.demo
            && let Some(snapshot) = self.branches.history.snapshot.as_ref()
        {
            self.branches.history.filter.demo_query(snapshot);
        }
    }
    pub(in crate::app) fn demo_git_history(&mut self) {
        if self.branches.demo {
            self.branches.history.demo(self.branches.owner.clone());
        }
    }
}
#[cfg(test)]
mod merged_tests;
#[cfg(test)]
mod tests;
