//! Docked changes inspector. Git and recorded edits are deliberately separate sources.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
mod branch_create;
mod branches;
mod clone;
mod commit;
mod conflict;
mod fetch;
pub(super) mod git_files;
mod git_modes;
mod identity_setup;
mod index_actions;
mod initialize;
mod inline_review;
pub(super) mod lines;
mod push;
mod read_state;
pub(super) mod review_comments;
mod review_files;
mod worktrees;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Source {
    Git,
    #[default]
    Recorded,
}
#[derive(Default)]
struct Diff {
    content: Option<Result<String, String>>,
    rx: Option<workspace::read_only::Request<Result<String, String>>>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RepoSheet {
    Initialize,
    Clone,
    Identity,
    CreateRemote,
    CreateBranch,
    Push,
    Fetch,
}

#[derive(Default)]
pub(super) struct Changes {
    expanded: bool,
    source: Source,
    git_mode: workspace::DiffMode,
    mode_controls: Vec<egui::Id>,
    query: String,
    identity: (PathBuf, String),
    open_rows: BTreeSet<(bool, String)>,
    inline: inline_review::Inline,
    diffs: BTreeMap<String, Diff>,
    refresh: bool,
    index_actions: index_actions::IndexActions,
    commit: commit::Commit,
    conflict: conflict::Conflict,
    initialize: initialize::Initialize,
    clone: clone::Clone,
    fetch: fetch::Fetch,
    identity_setup: identity_setup::Setup,
    push: push::Push,
    branch_create: branch_create::Create,
    pub(super) index_writable: bool,
    pub(super) review: Option<diff_viewer::Document>,
    close: bool,
    review_files: review_files::Files,
    git_files: git_files::Git,
    branches: branches::Branches,
    pub(super) review_comments: review_comments::Comments,
    search_open: bool,
    read_state: read_state::ReadState,
    worktrees: worktrees::Worktrees,
    pub(super) repo_sheet: Option<RepoSheet>,
}

impl Changes {
    #[cfg(test)]
    pub(super) fn test_diff(&mut self, path: &str, content: &str) {
        self.diffs.insert(
            path.into(),
            Diff {
                content: Some(Ok(content.into())),
                rx: None,
            },
        );
    }
    pub(super) fn expanded(&self) -> bool {
        self.expanded
    }
    pub(super) fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    pub(super) fn open_review(
        &mut self,
        ctx: &egui::Context,
        mut document: diff_viewer::Document,
        recorded: bool,
        cwd: &std::path::Path,
        session: &str,
    ) {
        self.close_branches(ctx);
        self.bind(cwd, session);
        document.3 = recorded;
        context_capture::clear_source(ctx);
        let mut selection = egui::text_selection::LabelSelectionState::load(ctx);
        selection.clear_selection();
        selection.store(ctx);
        document.2.focus(ctx);
        self.review = Some(document);
        self.review_files = Default::default();
        self.git_files = Default::default();
        self.expanded = false; // opening never implicitly replaces chat
        self.close = false;
    }
    pub(super) fn review_inputs(
        &mut self,
        ctx: &egui::Context,
        visible: bool,
        outputs: &[RecordedFile],
        revision: u64,
        workspace: &Workspace,
    ) {
        self.git_mode_inputs(ctx, visible);
        self.index_inputs(ctx, visible);
        self.commit_inputs(ctx, visible);
        self.conflict_inputs(visible);
        self.initialize_inputs(ctx, visible);
        self.clone_inputs(ctx, visible);
        self.fetch_inputs(ctx, visible);
        self.identity_setup_inputs(ctx, visible);
        self.push_inputs(ctx, visible);
        self.create_branch_inputs(ctx, visible);
        self.branch_inputs(ctx, visible, workspace);
        if visible {
            self.inline_inputs(ctx, outputs, revision, workspace);
        }
        if !visible || self.review.is_none() {
            self.review_comments.clear_markers();
            self.review_comments.clear_recorded_targets();
            self.review_comments.cancel_confirmation();
        }
        if self
            .review_comments
            .bind(&self.identity.0, &self.identity.1, revision)
            && let Some(document) = &mut self.review
            && document.3
        {
            document.2.stale_comments();
        }
        if visible
            && let Some(document) = &self.review
            && self.review_comments.inputs(ctx, document)
        {
            return;
        }
        if visible {
            self.poll_git_files(ctx, workspace);
            self.git_file_inputs(ctx, workspace);
            self.review_file_inputs(ctx, outputs, revision);
        } else {
            self.cancel_git_read();
        }
        if visible
            && let Some(document) = &mut self.review
            && diff_viewer::inputs(ctx, document)
        {
            self.review = None;
            self.git_files = Default::default();
            context_capture::clear_source(ctx);
        }
    }
    pub(super) fn take_close(&mut self) -> bool {
        std::mem::take(&mut self.close)
    }

    fn bind(&mut self, cwd: &std::path::Path, session: &str) {
        let session_changed = self.identity.1 != session;
        if self.identity.0 != cwd || session_changed {
            self.commit = Default::default();
            self.conflict = Default::default();
            self.initialize = Default::default();
            self.clone = Default::default();
            self.fetch = Default::default();
            self.identity_setup = Default::default();
            self.push = Default::default();
            self.branch_create = Default::default();
            self.identity = (cwd.to_path_buf(), session.to_owned());
            self.open_rows.clear();
            self.inline = Default::default();
            self.diffs.clear();
            self.query.clear();
            self.git_mode = Default::default();
            self.review = None;
            self.review_files = Default::default();
            self.git_files = Default::default();
            self.branches.cancel();
            self.review_comments.stale(None);
            self.search_open = false;
            self.read_state.clear();
            self.worktrees.reset();
            if session_changed {
                self.source = Source::Recorded;
            }
        }
    }

    pub(super) fn take_refresh(&mut self) -> bool {
        std::mem::take(&mut self.refresh)
    }

    pub(super) fn cancel_inline_reads(&mut self) {
        // Completed bodies may stay cached in saved layouts; workers may not.
        self.diffs.retain(|_, diff| diff.rx.is_none());
    }

    pub(super) fn invalidate(&mut self) {
        self.stale_review_comments();
        self.inline.clear();
        self.diffs.clear();
        self.git_files.stale = true;
        self.cancel_git_read();
    }

    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        cwd: &std::path::Path,
        session: &str,
        workspace: &Workspace,
        recorded: (&[RecordedFile], u64),
        loading: bool,
    ) -> bool {
        let (outputs, revision) = recorded;
        if self.identity.0 != cwd || self.identity.1 != session {
            self.close_branches(ctx);
        }
        self.bind(cwd, session);
        if self.review_comments.bind(cwd, session, revision)
            && let Some(document) = &mut self.review
            && document.3
        {
            document.2.stale_comments();
        }
        self.bind_inline_revision(ctx, outputs, revision);
        let wt_popup_open = egui::Popup::is_id_open(ctx, egui::Id::new("changes-worktrees-popup"));
        self.worktrees.poll(ctx, workspace, wt_popup_open);
        let inspected_ws = self.worktrees.inspected.clone();
        let active_workspace = inspected_ws.as_ref().unwrap_or(workspace);
        let saved_writable = self.index_writable;
        if inspected_ws.is_some() {
            self.index_writable = false;
        }
        self.bind_inline_git(active_workspace);
        self.poll_git_files(ctx, active_workspace);
        self.poll_inline_diffs();
        self.poll_branches(active_workspace);
        let colors = theme::palette(ctx);
        let expanded = self.expanded;
        self.index_actions.controls.clear();
        if expanded {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(colors.bg))
                .show(ctx, |ui| {
                    self.contents(ui, active_workspace, outputs, loading, revision)
                });
        } else {
            let available = ctx.available_rect();
            if available.width() < 700.0 {
                egui::TopBottomPanel::bottom("workspace-changes-stack")
                    .default_height((available.height() * 0.45).clamp(160.0, 340.0))
                    .height_range(140.0..=(available.height() - 240.0).max(140.0))
                    .resizable(true)
                    .frame(egui::Frame::new().fill(colors.bg))
                    .show(ctx, |ui| {
                        ui.set_min_height(ui.available_height());
                        self.contents(ui, active_workspace, outputs, loading, revision)
                    });
            } else {
                egui::SidePanel::right("workspace-changes-dock")
                    .default_width((available.width() * 0.52).clamp(320.0, 760.0))
                    .width_range(280.0..=(available.width() - 320.0).max(280.0))
                    .resizable(true)
                    .frame(egui::Frame::new().fill(colors.bg))
                    .show(ctx, |ui| {
                        self.contents(ui, active_workspace, outputs, loading, revision)
                    });
            }
        }
        self.index_writable = saved_writable;
        self.repo_sheet_dialog(ctx, active_workspace, loading);
        self.index_dialog(ctx);
        self.restore_dialog(ctx);
        self.commit_dialog(ctx);
        self.conflict_dialog(ctx);
        self.initialize_dialog(ctx);
        self.clone_dialog(ctx);
        self.fetch_dialog(ctx);
        self.identity_setup_dialog(ctx);
        self.push_dialog(ctx);
        self.create_branch_dialog(ctx);
        expanded
    }

    fn contents(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        outputs: &[RecordedFile],
        loading: bool,
        revision: u64,
    ) {
        let colors = theme::palette(ui.ctx());
        egui::TopBottomPanel::top("changes-heading")
            .exact_height(42.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 0)))
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(
                        RichText::new("Changes")
                            .size(12.5)
                            .strong()
                            .color(colors.text),
                    );
                    self.worktree_button(ui, workspace);
                    self.branch_button(ui, workspace);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        let close = icons::button(ui, Icon::Close, "Hide changes pane");
                        if close.clicked() {
                            self.close_branches(ui.ctx());
                            self.close = true;
                        }
                        #[cfg(test)]
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(egui::Id::new("changes-close"), close.rect)
                        });
                        if icons::button(
                            ui,
                            if self.expanded {
                                Icon::Collapse
                            } else {
                                Icon::Expand
                            },
                            if self.expanded {
                                "Restore changes panel"
                            } else {
                                "Expand changes panel"
                            },
                        )
                        .clicked()
                        {
                            self.expanded = !self.expanded;
                        }
                        let refresh = icons::button(ui, Icon::Refresh, "Refresh changes");
                        #[cfg(test)]
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(egui::Id::new("changes-refresh"), refresh.rect)
                        });
                        if refresh.clicked() {
                            self.refresh = true;
                            self.invalidate();
                            if let Some(path) = self.worktrees.active_path.clone() {
                                self.worktrees.select(ui.ctx(), path);
                            }
                        }
                        let (search_rect, search_resp) =
                            ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                        let active_bg = ui.visuals().selection.bg_fill;
                        if self.search_open {
                            ui.painter().rect_filled(search_rect, 6, active_bg);
                        } else if search_resp.hovered() {
                            ui.painter().rect_filled(search_rect, 6, colors.surface);
                        }
                        icons::draw(
                            ui,
                            search_rect.shrink(6.0),
                            Icon::Search,
                            if self.search_open {
                                egui::Color32::WHITE
                            } else if search_resp.hovered() {
                                colors.text
                            } else {
                                colors.muted
                            },
                        );
                        if search_resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Filter file paths")
                            .clicked()
                        {
                            self.search_open = !self.search_open;
                            if !self.search_open {
                                self.query.clear();
                            }
                        }
                    });
                });
            });
        if let Some(recorded) = self.review.as_ref().map(|doc| doc.3) {
            self.mode_controls.clear();
            if !recorded {
                self.review_mode_controls(ui, workspace);
            }
            let files = if recorded {
                self.review_file_navigation(ui.ctx(), outputs, revision)
            } else {
                self.git_file_navigation(ui.ctx())
            };
            let source_root = if recorded {
                Some(&self.identity.0)
            } else {
                workspace.is_git.then_some(&workspace.root)
            };
            if self.review_comments.source_root.as_ref() != source_root {
                self.review_comments.stale(Some(recorded));
                self.review_comments.source_root = source_root.cloned();
            }
            self.review_comments.prepare_recorded_targets(
                outputs,
                revision,
                self.review.as_ref().unwrap(),
                self.review_files.available(revision),
            );
            let action = diff_viewer::contents_with_review(
                ui,
                self.review.as_mut().unwrap(),
                Some(&files),
                Some(&mut self.review_comments),
            );
            if action.back {
                self.review = None;
                self.git_files = Default::default();
                self.review_comments.clear_recorded_targets();
                context_capture::clear_source(ui.ctx());
            } else if recorded
                && self.review_files.available(revision)
                && let Some((position, document)) = self.review_comments.take_recorded_open(
                    ui.ctx(),
                    outputs,
                    revision,
                    self.review.as_ref().unwrap(),
                )
            {
                self.cancel_git_read();
                self.review_files.set_position(position);
                self.review = Some(document);
                ui.ctx().request_repaint();
            } else if let Some(next) = action.next {
                if recorded {
                    self.step_recorded_file(ui.ctx(), outputs, revision, next);
                } else {
                    self.step_git_file(ui.ctx(), workspace, next);
                }
            }
            return;
        }
        self.review_comments.clear_recorded_targets();
        if self.search_open || !self.query.is_empty() {
            egui::TopBottomPanel::top("changes-search-bar")
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 4)))
                .show_inside(ui, |ui| {
                    let width = (ui.available_width() - 18.0).max(40.0);
                    egui::Frame::new()
                        .fill(colors.surface)
                        .stroke(Stroke::new(1.0, colors.border))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(8, 5))
                        .show(ui, |ui| {
                            ui.add_sized(
                                [width, 18.0],
                                egui::TextEdit::singleline(&mut self.query)
                                    .frame(false)
                                    .desired_width(width - 8.0)
                                    .hint_text("Filter file paths…")
                                    .id_salt("changes-search"),
                            );
                        });
                });
        }
        egui::TopBottomPanel::top("changes-controls")
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)))
            .show_inside(ui, |ui| {
                if let Some(error) = &workspace.error {
                    ui.label(theme::small(ui, error));
                }
                self.mode_controls.clear();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    self.source_icon_toggle(ui);
                    if self.source == Source::Git {
                        self.git_mode_pills(ui);
                        self.git_action_icons(ui, workspace, loading);
                    }
                });
            });
        if self.source == Source::Git {
            self.active_git_notices(ui, workspace, loading);
        }
        if let Some(wt_path) = self.worktrees.active_path.clone() {
            egui::TopBottomPanel::top("changes-worktree-banner")
                .frame(
                    egui::Frame::new()
                        .fill(colors.surface)
                        .inner_margin(egui::Margin::symmetric(12, 5)),
                )
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(theme::small(
                            ui,
                            if self.worktrees.loading_workspace() {
                                format!("Loading worktree {}…", wt_path.display())
                            } else {
                                format!("Worktree (read-only): {}", wt_path.display())
                            },
                        ));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(egui::Button::new(theme::small(ui, "Session root")).frame(false))
                                .clicked()
                            {
                                self.worktrees.clear_selection();
                                self.diffs.clear();
                                self.open_rows.clear();
                            }
                        });
                    });
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show_inside(ui, |ui| {
                let row_width = ui.available_width();
                egui::ScrollArea::vertical()
                    .id_salt(("changes-rows", self.source == Source::Git))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(row_width);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        egui::Frame::new()
                            .inner_margin(egui::Margin { left: 12, right: 12, top: 10, bottom: 8 })
                            .show(ui, |ui| {
                                ui.label(theme::small(
                                    ui,
                                    if self.source == Source::Recorded {
                                        "Recorded edits, in order—not a net Git diff."
                                    } else {
                                        match self.git_mode {
                                            workspace::DiffMode::Combined => "Combined: working tree vs HEAD.",
                                            workspace::DiffMode::Staged => "Staged: index vs HEAD.",
                                            workspace::DiffMode::Unstaged => "Unstaged: working tree vs index.",
                                        }
                                    },
                                ));
                            });
                        if let Some(notice)=&self.index_actions.notice{ui.add(egui::Label::new(theme::small(ui,notice)).wrap());}
                        self.recovery_control(ui,workspace);
                        self.inline_capacity(ui);
                        let query = self.query.trim().to_lowercase();
                        let mut count = 0;
                        match self.source {
                            Source::Recorded => {
                                // Outputs card is newest-file-first; this panel follows recorded order.
                                for file in outputs
                                    .iter()
                                    .rev()
                                    .filter(|file| file.path.to_lowercase().contains(&query))
                                {
                                    let summary_sig = read_state::recorded_summary_sig(
                                        file.added,
                                        file.removed,
                                        &file.diff,
                                    );
                                    let diff_sig = Some(read_state::hash_str(&file.diff));
                                    let is_read = self.read_state.reconcile(
                                        false,
                                        &file.path,
                                        summary_sig,
                                        diff_sig,
                                    );
                                    count += 1;
                                    egui::Frame::new()
                                        .outer_margin(egui::Margin { left: 12, right: 12, top: 0, bottom: 6 })
                                        .fill(colors.sidebar)
                                        .stroke(Stroke::new(1.0, colors.border))
                                        .corner_radius(7)
                                        .show(ui, |ui| {
                                            if self.header_with_read(
                                                ui,
                                                &file.path,
                                                &file.added.to_string(),
                                                &file.removed.to_string(),
                                                false,
                                                (is_read, summary_sig, diff_sig),
                                                None,
                                            ) {
                                                ui.push_id(
                                                    (&self.identity, "recorded-diff", &file.path),
                                                    |ui| {
                                                        Self::inline_diff(ui, &mut self.inline, &self.identity, &file.path, &file.diff, true);
                                                    },
                                                );
                                            }
                                        });
                                }
                            }
                            Source::Git => {
                                for file in workspace
                                    .files
                                    .iter()
                                    .filter(|file| file.path.to_lowercase().contains(&query))
                                {
                                    if !self.git_mode_includes(file){continue;}
                                    let summary_sig = read_state::git_summary_sig(
                                        &file.status,
                                        &file.added,
                                        &file.removed,
                                    );
                                    let diff_sig = self
                                        .diffs
                                        .get(&file.path)
                                        .and_then(|d| d.content.as_ref())
                                        .and_then(|r| r.as_ref().ok())
                                        .map(|s| read_state::hash_str(s));
                                    let is_read = self.read_state.reconcile(
                                        true,
                                        &file.path,
                                        summary_sig,
                                        diff_sig,
                                    );
                                    count += 1;
                                    egui::Frame::new()
                                        .outer_margin(egui::Margin { left: 12, right: 12, top: 0, bottom: 6 })
                                        .fill(colors.sidebar)
                                        .stroke(Stroke::new(1.0, colors.border))
                                        .corner_radius(7)
                                        .show(ui, |ui| {
                                            self.conflict_row(ui, workspace, file);
                                            if self.header_with_read(
                                                ui,
                                                &file.path,
                                                if self.git_mode==workspace::DiffMode::Combined{&file.added}else{"?"},
                                                if self.git_mode==workspace::DiffMode::Combined{&file.removed}else{"?"},
                                                true,
                                                (is_read, summary_sig, diff_sig),
                                                Some((workspace, file, loading)),
                                            )
                                            {
                                                if file.status == "??" {
                                                    egui::Frame::new().inner_margin(12).show(ui, |ui| {
                                                        ui.label(theme::small(ui, "Untracked file"));
                                                        ui.add(egui::Label::new(theme::small(ui, "This file isn’t tracked by Git, so there’s no diff to show.")).wrap());
                                                    });
                                                    return;
                                                }
                                                if !self.diffs.contains_key(&file.path) {
                                                    self.load_diff(ui.ctx(), &workspace.root, file.clone());
                                                }
                                                match self
                                                    .diffs
                                                    .get(&file.path)
                                                    .and_then(|diff| diff.content.as_ref())
                                                {
                                                    None => {
                                                        ui.label(theme::small(ui, "Loading diff…"));
                                                    }
                                                    Some(Err(error)) => {
                                                        // An error is never numbered, selected or captured as patch text.
                                                        ui.label(theme::small(ui, error));
                                                    }
                                                    Some(Ok(content)) => {
                                                        ui.push_id(
                                                            (&self.identity, "working-diff", &file.path),
                                                            |ui| {
                                                                if content.trim().is_empty() {
                                                                    ui.label(theme::small(ui, "No changes to display."));
                                                                } else {
                                                                    Self::inline_diff(ui, &mut self.inline, &self.identity, &file.path, content, false);
                                                                }
                                                            },
                                                        );
                                                    }
                                                }
                                            }
                                        });
                                }
                            }
                        }
                        if count == 0 {
                            egui::Frame::new().inner_margin(16).show(ui, |ui| {
                                ui.label(theme::small(
                                    ui,
                                    if !query.is_empty() {
                                        "No matching changes."
                                    } else if self.source == Source::Recorded {
                                        "No successful recorded edits in this chat."
                                    } else if loading {
                                        "Loading Git changes…"
                                    } else if let Some(error) = &workspace.error {
                                        error
                                    } else if workspace.is_git {
                                        match self.git_mode{workspace::DiffMode::Combined=>"Working tree is clean.",workspace::DiffMode::Staged=>"No staged changes.",workspace::DiffMode::Unstaged=>"No unstaged changes."}
                                    } else {
                                        "Not a Git repository. Recorded edits are still available."
                                    },
                                ));
                            });
                        }
                    });
            });
        self.finish_inline_inputs(ui.ctx());
    }

    fn worktree_button(&mut self, ui: &mut egui::Ui, workspace: &Workspace) {
        let colors = theme::palette(ui.ctx());
        let name = if workspace.root.as_os_str().is_empty() {
            self.identity
                .0
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("main")
                .to_owned()
        } else {
            self.worktrees.active_label(workspace)
        };
        let label = format!("{name} ▾");
        let popup_id = egui::Id::new("changes-worktrees-popup");
        let open = self.worktrees.active_path.is_some()
            || egui::Popup::is_id_open(ui.ctx(), popup_id);
        let galley = ui.painter().layout_no_wrap(
            label.clone(),
            FontId::proportional(11.5),
            colors.text,
        );
        let pill_w = (galley.size().x + 34.0).clamp(64.0, 140.0);
        let (_, rect) = ui.allocate_space(egui::vec2(pill_w, 26.0));
        let resp = ui
            .interact(rect, popup_id.with("trigger"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Switch / inspect linked worktree");
        ui.painter().rect_filled(
            rect,
            6,
            if resp.hovered() || open {
                colors.hover
            } else {
                colors.surface
            },
        );
        ui.painter().rect_stroke(
            rect,
            6,
            Stroke::new(1.0, colors.border),
            egui::StrokeKind::Inside,
        );
        icons::draw(
            ui,
            egui::Rect::from_center_size(
                rect.left_center() + egui::vec2(12.0, 0.0),
                Vec2::splat(13.0),
            ),
            Icon::Folder,
            colors.muted,
        );
        ui.painter().galley(
            rect.left_center() + egui::vec2(24.0, -galley.size().y / 2.0),
            galley,
            colors.text,
        );
        let width = (ui.ctx().screen_rect().width() - 32.0).clamp(180.0, 290.0);
        egui::Popup::from_toggle_button_response(&resp)
            .id(popup_id)
            .width(width)
            .frame(prompt_ui::popup(ui.ctx()))
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                let inner_w = (width - 20.0).max(1.0);
                ui.set_width(inner_w);
                prompt_ui::style(ui);
                ui.label(theme::small(ui, "Linked worktrees"));
                ui.add_space(4.0);
                if !workspace.is_git {
                    if let Some(err) = &workspace.error {
                        ui.add(egui::Label::new(theme::small(ui, err)).wrap());
                        ui.add_space(4.0);
                        if Self::dropdown_item(
                            ui,
                            popup_id.with("refresh-ws"),
                            inner_w,
                            "Refresh workspace",
                            None,
                        )
                        .clicked()
                        {
                            self.refresh = true;
                            self.invalidate();
                            egui::Popup::close_id(ui.ctx(), popup_id);
                        }
                    } else {
                        ui.label(theme::small(ui, "Not a Git repository."));
                    }
                    return;
                }
                if self.worktrees.loading_list() {
                    ui.label(theme::small(ui, "Loading worktrees…"));
                    return;
                }
                if let Some(err) = &self.worktrees.list_error {
                    ui.add(egui::Label::new(theme::small(ui, err)).wrap());
                    return;
                }
                if self.worktrees.entries.is_empty() {
                    ui.label(theme::small(ui, "No worktrees found."));
                    return;
                }
                let active_bg = ui.visuals().selection.bg_fill;
                let selected_tint = egui::Color32::from_rgba_unmultiplied(
                    active_bg.r(),
                    active_bg.g(),
                    active_bg.b(),
                    64,
                );
                for entry in self.worktrees.entries.clone() {
                    let selected = self
                        .worktrees
                        .active_path
                        .as_ref()
                        .is_some_and(|p| p == &entry.path)
                        || (self.worktrees.active_path.is_none()
                            && entry.path == workspace.root);
                    let (_, row_rect) = ui.allocate_space(egui::vec2(inner_w, 28.0));
                    let row_resp = ui
                        .add_enabled_ui(!entry.bare, |ui| {
                            ui.interact(
                                row_rect,
                                popup_id.with(&entry.path),
                                Sense::click(),
                            )
                        })
                        .inner
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if selected {
                        ui.painter().rect_filled(row_rect, 5, selected_tint);
                        ui.painter().rect_stroke(
                            row_rect,
                            5,
                            Stroke::new(1.0, colors.accent),
                            egui::StrokeKind::Inside,
                        );
                    } else if row_resp.hovered() {
                        ui.painter().rect_filled(row_rect, 5, colors.hover);
                    }
                    icons::draw(
                        ui,
                        egui::Rect::from_center_size(
                            row_rect.left_center() + egui::vec2(14.0, 0.0),
                            Vec2::splat(13.0),
                        ),
                        Icon::Folder,
                        if selected { colors.text } else { colors.muted },
                    );
                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        &entry.short_name(),
                        0.0,
                        egui::TextFormat::simple(FontId::proportional(12.0), colors.text),
                    );
                    job.append(
                        &format!(" ({})", entry.branch),
                        0.0,
                        egui::TextFormat::simple(FontId::proportional(12.0), colors.muted),
                    );
                    job.wrap.max_width = (inner_w - 80.0).max(30.0);
                    job.wrap.max_rows = 1;
                    let galley = ui.painter().layout_job(job);
                    ui.painter().galley(
                        row_rect.left_center() + egui::vec2(26.0, -galley.size().y / 2.0),
                        galley,
                        colors.text,
                    );
                    ui.painter().text(
                        row_rect.right_center() - egui::vec2(8.0, 0.0),
                        egui::Align2::RIGHT_CENTER,
                        if selected { "active" } else { "inspect" },
                        FontId::proportional(10.5),
                        colors.muted,
                    );
                    if row_resp.clicked() {
                        if entry.path == workspace.root || selected {
                            self.worktrees.clear_selection();
                        } else {
                            self.worktrees.select(ui.ctx(), entry.path);
                        }
                        self.diffs.clear();
                        self.open_rows.clear();
                        egui::Popup::close_id(ui.ctx(), popup_id);
                    }
                }
            });
    }

    fn dropdown_item(
        ui: &mut egui::Ui,
        id: egui::Id,
        width: f32,
        label: &str,
        right_hint: Option<&str>,
    ) -> egui::Response {
        let colors = theme::palette(ui.ctx());
        let (_, rect) = ui.allocate_space(egui::vec2(width, 28.0));
        let resp = ui
            .interact(rect, id, Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.hovered() {
            ui.painter().rect_filled(rect, 5, colors.hover);
        }
        ui.painter().text(
            rect.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::proportional(12.0),
            colors.text,
        );
        if let Some(hint) = right_hint {
            ui.painter().text(
                rect.right_center() - egui::vec2(8.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                hint,
                FontId::proportional(10.5),
                colors.muted,
            );
        }
        resp
    }

    fn boxed_icon_button(ui: &mut egui::Ui, icon: Icon, tip: &str) -> egui::Response {
        let colors = theme::palette(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
        let bg = if response.hovered() && ui.is_enabled() {
            colors.hover
        } else {
            colors.surface
        };
        let fg = if ui.is_enabled() {
            colors.text
        } else {
            colors.dim
        };
        ui.painter().rect_filled(rect, 6, bg);
        ui.painter().rect_stroke(
            rect,
            6,
            Stroke::new(1.0, colors.border),
            egui::StrokeKind::Inside,
        );
        icons::draw(ui, rect.shrink(6.0), icon, fg);
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tip)
    }

    fn source_icon_toggle(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        let active_bg = ui.visuals().selection.bg_fill;
        egui::Frame::new()
            .fill(colors.sidebar)
            .stroke(Stroke::new(1.0, colors.border))
            .corner_radius(6)
            .inner_margin(egui::Margin::same(2))
            .show(ui, |ui| {
                ui.spacing_mut().interact_size.y = 20.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (src, icon, tip, _test_id) in [
                        (Source::Git, Icon::Branch, "Current Git", "changes-source-git"),
                        (
                            Source::Recorded,
                            Icon::Clock,
                            "Recorded edits",
                            "changes-source-recorded",
                        ),
                    ] {
                        let active = self.source == src;
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(26.0, 20.0), Sense::click());
                        #[cfg(test)]
                        ui.ctx()
                            .data_mut(|d| d.insert_temp(egui::Id::new(_test_id), rect));
                        if active {
                            ui.painter().rect_filled(rect, 4, active_bg);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(rect, 4, colors.hover);
                        }
                        icons::draw(
                            ui,
                            egui::Rect::from_center_size(rect.center(), Vec2::splat(13.0)),
                            icon,
                            if active {
                                egui::Color32::WHITE
                            } else if resp.hovered() {
                                colors.text
                            } else {
                                colors.muted
                            },
                        );
                        if resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(tip)
                            .clicked()
                        {
                            self.source = src;
                        }
                    }
                });
            });
    }

    fn git_action_icons(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        loading: bool,
    ) {
        let colors = theme::palette(ui.ctx());
        let modal_clear = !egui::Popup::is_any_open(ui.ctx())
            && ui.memory(|m| m.top_modal_layer().is_none());
        let width = (ui.ctx().screen_rect().width() - 32.0).clamp(220.0, 260.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let setup_id = egui::Id::new("changes-repo-settings-popup");
            let setup_resp = Self::boxed_icon_button(
                ui,
                Icon::Settings,
                "Repository settings (Clone, Identity, Branch)",
            );
            egui::Popup::from_toggle_button_response(&setup_resp)
                .id(setup_id)
                .width(width)
                .frame(prompt_ui::popup(ui.ctx()))
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    let inner_w = (width - 20.0).max(1.0);
                    ui.set_width(inner_w);
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(
                        RichText::new("Repository settings")
                            .size(11.0)
                            .color(colors.muted),
                    );
                    if !workspace.is_git
                        && Self::dropdown_item(
                            ui,
                            setup_id.with("init"),
                            inner_w,
                            "Initialize Git repository…",
                            None,
                        )
                        .clicked()
                    {
                        self.repo_sheet = Some(RepoSheet::Initialize);
                        egui::Popup::close_id(ui.ctx(), setup_id);
                    }
                    if Self::dropdown_item(
                        ui,
                        setup_id.with("identity"),
                        inner_w,
                        "Set commit identity…",
                        None,
                    )
                    .clicked()
                    {
                        self.repo_sheet = Some(RepoSheet::Identity);
                        egui::Popup::close_id(ui.ctx(), setup_id);
                    }
                    if Self::dropdown_item(
                        ui,
                        setup_id.with("clone"),
                        inner_w,
                        "Clone repository…",
                        None,
                    )
                    .clicked()
                    {
                        self.repo_sheet = Some(RepoSheet::Clone);
                        egui::Popup::close_id(ui.ctx(), setup_id);
                    }
                    if Self::dropdown_item(
                        ui,
                        setup_id.with("remote"),
                        inner_w,
                        "Create remote…",
                        None,
                    )
                    .clicked()
                    {
                        self.repo_sheet = Some(RepoSheet::CreateRemote);
                        egui::Popup::close_id(ui.ctx(), setup_id);
                    }
                    let (sep_rect, _) =
                        ui.allocate_exact_size(egui::vec2(inner_w, 1.0), Sense::hover());
                    ui.painter().rect_filled(sep_rect, 0, colors.border);
                    if Self::dropdown_item(
                        ui,
                        setup_id.with("branch"),
                        inner_w,
                        "Create or switch branch…",
                        None,
                    )
                    .clicked()
                    {
                        self.repo_sheet = Some(RepoSheet::CreateBranch);
                        egui::Popup::close_id(ui.ctx(), setup_id);
                    }
                });

            let fetch_id = egui::Id::new("changes-fetch-popup");
            let fetch_resp =
                Self::boxed_icon_button(ui, Icon::ArrowDown, "Fetch remote branch…");
            egui::Popup::from_toggle_button_response(&fetch_resp)
                .id(fetch_id)
                .width(width)
                .frame(prompt_ui::popup(ui.ctx()))
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    let inner_w = (width - 20.0).max(1.0);
                    ui.set_width(inner_w);
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(
                        RichText::new("Fetch remote branch")
                            .size(11.0)
                            .color(colors.muted),
                    );
                    if Self::dropdown_item(
                        ui,
                        fetch_id.with("fetch"),
                        inner_w,
                        "Fetch remote branch…",
                        None,
                    )
                    .clicked()
                    {
                        self.repo_sheet = Some(RepoSheet::Fetch);
                        egui::Popup::close_id(ui.ctx(), fetch_id);
                    }
                });

            let push_id = egui::Id::new("changes-push-popup");
            let push_resp = Self::boxed_icon_button(ui, Icon::ArrowUp, "Push & remotes…");
            egui::Popup::from_toggle_button_response(&push_resp)
                .id(push_id)
                .width(width)
                .frame(prompt_ui::popup(ui.ctx()))
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    let inner_w = (width - 20.0).max(1.0);
                    ui.set_width(inner_w);
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(
                        RichText::new("Push & remotes")
                            .size(11.0)
                            .color(colors.muted),
                    );
                    if Self::dropdown_item(
                        ui,
                        push_id.with("inspect"),
                        inner_w,
                        "Inspect push destination…",
                        None,
                    )
                    .clicked()
                    {
                        self.repo_sheet = Some(RepoSheet::Push);
                        egui::Popup::close_id(ui.ctx(), push_id);
                    }
                    if Self::dropdown_item(
                        ui,
                        push_id.with("remote"),
                        inner_w,
                        "Create remote…",
                        None,
                    )
                    .clicked()
                    {
                        self.repo_sheet = Some(RepoSheet::CreateRemote);
                        egui::Popup::close_id(ui.ctx(), push_id);
                    }
                });

            let commit_enabled = self.index_writable
                && workspace.is_git
                && !loading
                && workspace.error.is_none()
                && self.commit_ready()
                && modal_clear;
            let commit_resp = ui.add_enabled_ui(commit_enabled, |ui| {
                Self::boxed_icon_button(ui, Icon::Check, "Review staged commit…")
            });
            if commit_resp.inner.clicked() {
                self.start_commit_inspection(ui.ctx(), workspace.root.clone());
            }
        });
    }

    fn repo_sheet_dialog(
        &mut self,
        ctx: &egui::Context,
        workspace: &Workspace,
        loading: bool,
    ) {
        let Some(sheet) = self.repo_sheet else {
            return;
        };
        let (title, subtitle) = match sheet {
            RepoSheet::Initialize => (
                "Initialize Git repository",
                "Creates a local .git directory without staging or publishing files.",
            ),
            RepoSheet::Clone => (
                "Clone repository",
                "Clone a remote repository into a local folder inside this project.",
            ),
            RepoSheet::Identity => (
                "Set repository commit identity",
                "Saved to repository-local git config (.git/config).",
            ),
            RepoSheet::CreateRemote => (
                "Create remote",
                "Add a new Git remote to this repository.",
            ),
            RepoSheet::CreateBranch => (
                "Create or switch local branch",
                "Inspect and confirm before creating or switching a branch.",
            ),
            RepoSheet::Push => (
                "Push & remotes",
                "Inspect push destination and commits before pushing.",
            ),
            RepoSheet::Fetch => (
                "Fetch remote branch",
                "Updates only the tracking ref; working tree is unchanged.",
            ),
        };
        let mut close = false;
        let response = prompt_ui::modal(ctx, egui::Id::new("changes-repo-sheet"), 420.0, |ui| {
            prompt_ui::heading(ui, title, subtitle);
            match sheet {
                RepoSheet::Initialize => self.initialize_ui(ui, workspace, loading),
                RepoSheet::Clone => self.clone_ui(ui, workspace, loading),
                RepoSheet::Identity => self.identity_setup_ui(ui, workspace),
                RepoSheet::CreateRemote => self.remote_create(ui, workspace),
                RepoSheet::CreateBranch => self.create_branch_ui(ui, workspace),
                RepoSheet::Push => self.push_review(ui, workspace, loading),
                RepoSheet::Fetch => self.fetch_ui(ui, workspace),
            }
            prompt_ui::footer(ui, |ui| {
                if prompt_ui::button(ui, "Close", prompt_ui::Button::Secondary, true).clicked() {
                    close = true;
                }
            });
        });
        if close
            || response.should_close()
            || ctx.input(|i| i.key_pressed(egui::Key::Escape))
        {
            self.repo_sheet = None;
        }
    }

    fn active_git_notices(
        &mut self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        loading: bool,
    ) {
        if self.commit_has_status() {
            egui::TopBottomPanel::top("changes-commit-status")
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 4)))
                .show_inside(ui, |ui| {
                    self.commit_control(ui, workspace, loading);
                });
        }
    }

    #[cfg(test)]
    fn header(
        &mut self,
        ui: &mut egui::Ui,
        path: &str,
        added: &str,
        removed: &str,
        git: bool,
    ) -> bool {
        self.header_with_read(ui, path, added, removed, git, (false, 0, None), None)
    }

    fn header_with_read(
        &mut self,
        ui: &mut egui::Ui,
        path: &str,
        added: &str,
        removed: &str,
        git: bool,
        read_info: (bool, u64, Option<u64>),
        git_row: Option<(&Workspace, &workspace::FileChange, bool)>,
    ) -> bool {
        let (is_read, summary_sig, diff_sig) = read_info;
        let colors = theme::palette(ui.ctx());
        let key = (git, path.to_owned());
        let mut open = self.open_rows.contains(&key);
        let row_w = ui.available_width().min(ui.clip_rect().width()).max(80.0);
        let (_, rect) = ui.allocate_space(Vec2::new(row_w, 36.0));
        let allowed = (open || self.can_expand_inline(path))
            && !egui::Popup::is_any_open(ui.ctx())
            && ui.ctx().memory(|memory| memory.top_modal_layer().is_none());

        let has_git_actions = git_row.is_some();
        let row_click_rect = egui::Rect::from_min_max(
            rect.min,
            egui::pos2(
                if has_git_actions {
                    (rect.right() - 252.0).max(rect.left() + 60.0)
                } else {
                    (rect.right() - 40.0).max(rect.left() + 60.0)
                },
                rect.max.y,
            ),
        );
        let response = ui
            .add_enabled_ui(allowed, |ui| {
                ui.interact(row_click_rect, self.inline_header_id(git, path), Sense::click())
            })
            .inner
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if !open && !self.can_expand_inline(path) {
            response
                .clone()
                .on_disabled_hover_text(if path.len() > 4096 {
                    "Inline review unavailable above the 4 KiB path limit."
                } else {
                    "Close another expanded file (32 max)."
                });
        }
        self.inline_header_input(ui.ctx(), &response);

        let read_rect = egui::Rect::from_center_size(
            rect.right_center() - egui::vec2(20.0, 0.0),
            Vec2::splat(22.0),
        );
        let read_enabled = !egui::Popup::is_any_open(ui.ctx())
            && ui.ctx().memory(|memory| memory.top_modal_layer().is_none());
        let read_resp = ui
            .add_enabled_ui(read_enabled, |ui| {
                ui.interact(
                    read_rect,
                    self.inline_header_id(git, path).with("read-toggle"),
                    Sense::hover(),
                )
            })
            .inner
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(if is_read {
                "Mark as unread"
            } else {
                "Mark as read"
            });
        let read_clicked = read_enabled
            && read_resp.hovered()
            && ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary));

        if let Some((ws, file, loading)) = git_row {
            let actions_rect = egui::Rect::from_min_max(
                egui::pos2((rect.right() - 250.0).max(rect.left() + 80.0), rect.top() + 4.0),
                egui::pos2(rect.right() - 106.0, rect.bottom() - 4.0),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(actions_rect), |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    self.inline_index_buttons(ui, ws, file, loading);
                });
            });
        }

        if read_clicked {
            let now_read = self
                .read_state
                .toggle(git, path, summary_sig, diff_sig);
            if now_read {
                self.open_rows.remove(&key);
                self.inline.close(&key);
                open = false;
            }
        } else if response.clicked() {
            open = !open;
            if open {
                self.open_rows.insert(key);
            } else {
                self.open_rows.remove(&key);
                self.inline.close(&key);
                inline_review::Inline::clear_selection(ui.ctx());
                if git {
                    self.diffs.remove(path); // dropping pending delivery cancels the child
                }
            }
        }
        if response.hovered() && !read_resp.hovered() {
            ui.painter().rect_filled(rect, 7, colors.hover);
        }
        icons::chevron(
            ui,
            egui::Rect::from_center_size(
                rect.left_center() + egui::vec2(14.0, 0.0),
                Vec2::splat(11.0),
            ),
            open,
            colors.dim,
        );
        let row_text_color = if is_read { colors.dim } else { colors.text };
        let reserved_right = if has_git_actions { 256.0 } else { 114.0 };
        let mut job = egui::text::LayoutJob::simple_singleline(
            path.into(),
            FontId::monospace(11.5),
            row_text_color,
        );
        job.wrap.max_width = (rect.width() - reserved_right - 28.0).max(30.0);
        job.wrap.max_rows = 1;
        let galley = ui.painter().layout_job(job);
        if galley.elided {
            response.on_hover_text(path);
        }
        ui.painter().galley(
            rect.left_center() + egui::vec2(28.0, -galley.size().y / 2.0),
            galley,
            row_text_color,
        );
        ui.painter().text(
            rect.right_center() - egui::vec2(72.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            format!("+{}", if added.is_empty() { "0" } else { added }),
            FontId::monospace(11.0),
            colors.green,
        );
        ui.painter().text(
            rect.right_center() - egui::vec2(38.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            format!("−{}", if removed.is_empty() { "0" } else { removed }),
            FontId::monospace(11.0),
            colors.red,
        );
        let active_bg = ui.visuals().selection.bg_fill;
        ui.painter().rect_filled(
            read_rect,
            5,
            if is_read {
                active_bg
            } else if read_resp.hovered() {
                colors.hover
            } else {
                colors.surface
            },
        );
        ui.painter().rect_stroke(
            read_rect,
            5,
            Stroke::new(1.0, if is_read { active_bg } else { colors.border }),
            egui::StrokeKind::Inside,
        );
        icons::draw(
            ui,
            read_rect.shrink(4.0),
            Icon::Check,
            if is_read {
                egui::Color32::WHITE
            } else if read_resp.hovered() {
                colors.text
            } else {
                colors.muted
            },
        );
        open
    }

    fn poll_inline_diffs(&mut self) {
        for diff in self.diffs.values_mut() {
            let result = diff.rx.as_ref().and_then(|rx| match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Git diff worker ended without a result. Refresh to retry.".into(),
                )),
                Err(mpsc::TryRecvError::Empty) => None,
            });
            if let Some(result) = result {
                diff.content = Some(result);
                diff.rx = None;
            }
        }
    }

    fn load_diff(
        &mut self,
        ctx: &egui::Context,
        root: &std::path::Path,
        file: workspace::FileChange,
    ) {
        let path = file.path.clone();
        if let Some(rx) = git_files::start_inline(ctx, root, file, self.git_mode) {
            self.inline.reset_git(&path);
            self.diffs.insert(
                path,
                Diff {
                    content: None,
                    rx: Some(rx),
                },
            );
        }
    }
}

/// Selectable source with quiet line numbers and full-width addition/deletion backgrounds.
#[cfg(test)]
pub(super) fn numbered_diff(ui: &mut egui::Ui, path: &str, source: &str, recorded: bool) {
    let _ = numbered_diff_target(ui, path, source, recorded, None, false);
}

pub(super) fn numbered_diff_target(
    ui: &mut egui::Ui,
    path: &str,
    source: &str,
    recorded: bool,
    target: Option<usize>,
    wrap: bool,
) -> egui::Id {
    numbered_diff_annotated(ui, path, source, recorded, target, wrap, |_, _, _| {})
}

pub(super) fn numbered_diff_annotated(
    ui: &mut egui::Ui,
    path: &str,
    source: &str,
    recorded: bool,
    target: Option<usize>,
    wrap: bool,
    mut annotate: impl FnMut(&mut egui::Ui, &egui::Response, &egui::Galley),
) -> egui::Id {
    let colors = theme::palette(ui.ctx());
    ui.push_id(("numbered-diff", path, recorded), |ui| {
        let mut response_id = ui.id();
        let mut reveal = None;
        let mut scroll = egui::ScrollArea::new([!wrap, false]).auto_shrink([false, true]);
        if wrap || target.is_some() {
            scroll = scroll.horizontal_scroll_offset(0.0);
        }
        scroll.show(ui, |ui| {
            let mut job = egui::text::LayoutJob::default();
            let mut backgrounds = Vec::new();
            for row in lines::rows(source) {
                let line = row.text;
                if row.header {
                    backgrounds.push(None);
                    job.append(
                        lines::HUNK_CAPTION,
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::monospace(12.0),
                            color: colors.dim,
                            line_height: Some(21.0),
                            ..Default::default()
                        },
                    );
                    continue;
                }
                backgrounds.push(if row.added {
                    Some(colors.diff_add_bg)
                } else if row.removed {
                    Some(colors.diff_remove_bg)
                } else {
                    None
                });
                job.append(
                    &row.gutter(),
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::monospace(12.0),
                        color: colors.dim,
                        line_height: Some(21.0),
                        ..Default::default()
                    },
                );
                job.append(
                    &format!("{line}\n"),
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::monospace(12.0),
                        color: colors.text,
                        line_height: Some(21.0),
                        ..Default::default()
                    },
                );
            }
            job.wrap.max_width = if wrap {
                ui.available_width().max(1.0)
            } else {
                f32::INFINITY
            };
            job.wrap.break_anywhere = wrap;
            let galley = ui.fonts(|fonts| fonts.layout_job(job));
            let background = ui.painter().add(egui::Shape::Noop);
            let response = ui.add(
                egui::Label::new(galley.clone())
                    .selectable(true)
                    .halign(Align::Min),
            );
            response_id = response.id;
            context_capture::offer_diff_selection(&response, path, source, recorded);
            let width = response.rect.width().max(ui.available_width());
            let mut logical = 0;
            let mut shapes = Vec::new();
            let mut target_rect = egui::Rect::NOTHING;
            let mut target_rows = 0;
            for row in &galley.rows {
                let rect = egui::Rect::from_min_size(
                    response.rect.min + row.pos.to_vec2(),
                    Vec2::new(width, row.size.y),
                );
                if target_rows < 2
                    && target.is_some_and(|target| logical == target || logical == target + 1)
                {
                    target_rect = target_rect.union(rect);
                    target_rows += 1;
                }
                if let Some(Some(color)) = backgrounds.get(logical) {
                    shapes.push(egui::Shape::rect_filled(rect, 0, *color));
                }
                if row.ends_with_newline {
                    logical += 1;
                }
            }
            if target_rect.is_positive() {
                target_rect.set_width(1.0);
                reveal = Some(target_rect);
                #[cfg(test)]
                ui.ctx().data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new("diff-hunk-target"),
                        (target.unwrap(), target_rect),
                    );
                });
            }
            ui.painter().set(background, egui::Shape::Vec(shapes));
            annotate(ui, &response, &galley);
        });
        // The inner horizontal area consumes both scroll targets. Issue the
        // vertical reveal only after leaving it so the enclosing area receives it.
        if let Some(rect) = reveal {
            ui.scroll_to_rect(rect, Some(Align::Center));
        }
        response_id
    })
    .inner
}

#[cfg(test)]
mod tests;
