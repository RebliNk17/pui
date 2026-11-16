//! Docked changes inspector. Git and recorded edits are deliberately separate sources.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
mod branches;
mod commit;
pub(super) mod git_files;
mod git_modes;
mod index_actions;
mod inline_review;
pub(super) mod lines;
mod push;
pub(super) mod review_comments;
mod review_files;

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
    push: push::Push,
    pub(super) index_writable: bool,
    pub(super) review: Option<diff_viewer::Document>,
    close: bool,
    review_files: review_files::Files,
    git_files: git_files::Git,
    branches: branches::Branches,
    pub(super) review_comments: review_comments::Comments,
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
            self.push = Default::default();
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
        self.bind_inline_git(workspace);
        self.poll_git_files(ctx, workspace);
        self.poll_inline_diffs();
        self.poll_branches(workspace);
        let colors = theme::palette(ctx);
        let expanded = self.expanded;
        self.index_actions.controls.clear();
        if expanded {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(colors.bg))
                .show(ctx, |ui| {
                    self.contents(ui, workspace, outputs, loading, revision)
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
                        self.contents(ui, workspace, outputs, loading, revision)
                    });
            } else {
                egui::SidePanel::right("workspace-changes-dock")
                    .default_width((available.width() * 0.52).clamp(320.0, 760.0))
                    .width_range(280.0..=(available.width() - 320.0).max(280.0))
                    .resizable(true)
                    .frame(egui::Frame::new().fill(colors.bg))
                    .show(ctx, |ui| {
                        self.contents(ui, workspace, outputs, loading, revision)
                    });
            }
        }
        self.index_dialog(ctx);
        self.commit_dialog(ctx);
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
            .exact_height(44.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(16, 0)))
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("Changes").size(13.0).color(colors.muted));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
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
                        if ui
                            .add(egui::Button::new(theme::small(ui, "Refresh")).frame(false))
                            .clicked()
                        {
                            self.refresh = true;
                            self.invalidate();
                        }
                        self.branch_button(ui, workspace);
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
        egui::TopBottomPanel::top("changes-controls")
            .frame(egui::Frame::new().inner_margin(16))
            .show_inside(ui, |ui| {
                ui.add(
                    egui::Label::new(theme::small(
                        ui,
                        if workspace.is_git {
                            format!("{} · {}", workspace.root.display(), workspace.branch)
                        } else if loading {
                            "Loading the session's repository…".into()
                        } else {
                            "Git changes require the session's repository root".into()
                        },
                    ))
                    .wrap(),
                );
                if let Some(error) = &workspace.error {
                    ui.label(theme::small(ui, error));
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let tabs = ["Current Git", "Recorded edits"].map(|label| {
                        ui.painter()
                            .layout_no_wrap(
                                label.into(),
                                ui.style().text_styles[&egui::TextStyle::Button].clone(),
                                colors.text,
                            )
                            .size()
                            .x
                            + 2.0 * ui.spacing().button_padding.x
                    });
                    // Include the search frame and widget margins. Under-counting
                    // them makes content a few pixels wider than its parent;
                    // egui remembers that larger panel width on every frame.
                    let width = (ui.available_width()
                        - tabs.iter().sum::<f32>()
                        - ui.spacing().item_spacing.x * 2.0
                        - 18.0)
                        .max(40.0);
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
                                    .hint_text("Search changes…")
                                    .id_salt("changes-search"),
                            );
                        });
                    ui.selectable_value(&mut self.source, Source::Git, "Current Git");
                    ui.selectable_value(&mut self.source, Source::Recorded, "Recorded edits");
                });
            });
        if self.source == Source::Git {
            self.git_mode_controls(ui);
            self.commit_control(ui, workspace, loading);
            self.push_review(ui, workspace, loading);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("changes-rows", self.source == Source::Git))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        egui::Frame::new().inner_margin(16).show(ui, |ui| {
                            ui.label(theme::small(
                                ui,
                                if self.source == Source::Recorded {
                                    "Recorded edits, in order—not a net Git diff."
                                } else {
                                    match self.git_mode{workspace::DiffMode::Combined=>"Combined: working tree vs HEAD. Submodule worktree edits are not inspected.",workspace::DiffMode::Staged=>"Staged: index vs HEAD. These are the changes currently staged for a commit.",workspace::DiffMode::Unstaged=>"Unstaged: working tree vs index. Untracked files have no Git patch."}
                                },
                            ));
                        });
                        if let Some(notice)=&self.index_actions.notice{ui.add(egui::Label::new(theme::small(ui,notice)).wrap());}
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
                                    count += 1;
                                    if self.header(
                                        ui,
                                        &file.path,
                                        &file.added.to_string(),
                                        &file.removed.to_string(),
                                        false,
                                    ) {
                                        ui.push_id(
                                            (&self.identity, "recorded-diff", &file.path),
                                            |ui| {
                                                Self::inline_diff(ui, &mut self.inline, &self.identity, &file.path, &file.diff, true);
                                            },
                                        );
                                    }
                                }
                            }
                            Source::Git => {
                                for file in workspace
                                    .files
                                    .iter()
                                    .filter(|file| file.path.to_lowercase().contains(&query))
                                {
                                    if !self.git_mode_includes(file){continue;}
                                    count += 1;
                                    self.index_row(ui,workspace,file,loading);
                                    if self.header(ui, &file.path, if self.git_mode==workspace::DiffMode::Combined{&file.added}else{"?"}, if self.git_mode==workspace::DiffMode::Combined{&file.removed}else{"?"}, true)
                                    {
                                        if file.status == "??" {
                                            egui::Frame::new().inner_margin(12).show(ui, |ui| {
                                                ui.label(theme::small(ui, "Untracked file"));
                                                ui.add(egui::Label::new(theme::small(ui, "This file isn’t tracked by Git, so there’s no diff to show.")).wrap());
                                            });
                                            continue;
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

    fn header(
        &mut self,
        ui: &mut egui::Ui,
        path: &str,
        added: &str,
        removed: &str,
        git: bool,
    ) -> bool {
        let colors = theme::palette(ui.ctx());
        let key = (git, path.to_owned());
        let mut open = self.open_rows.contains(&key);
        let (_, rect) = ui.allocate_space(Vec2::new(ui.available_width(), 42.0));
        let allowed = (open || self.can_expand_inline(path))
            && !egui::Popup::is_any_open(ui.ctx())
            && ui.ctx().memory(|memory| memory.top_modal_layer().is_none());
        let response = ui
            .add_enabled_ui(allowed, |ui| {
                ui.interact(rect, self.inline_header_id(git, path), Sense::click())
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
        if response.clicked() {
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
        ui.painter().rect_filled(
            rect,
            0,
            if response.hovered() {
                colors.hover
            } else {
                colors.sidebar
            },
        );
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            Stroke::new(1.0, colors.border),
        );
        icons::chevron(
            ui,
            egui::Rect::from_center_size(
                rect.left_center() + egui::vec2(18.0, 0.0),
                Vec2::splat(12.0),
            ),
            open,
            colors.dim,
        );
        icons::draw(
            ui,
            egui::Rect::from_center_size(
                rect.left_center() + egui::vec2(40.0, 0.0),
                Vec2::splat(16.0),
            ),
            Icon::File,
            colors.text,
        );
        let mut job = egui::text::LayoutJob::simple_singleline(
            path.into(),
            FontId::proportional(12.0),
            colors.text,
        );
        job.wrap.max_width = (rect.width() - 160.0).max(30.0);
        job.wrap.max_rows = 1;
        let galley = ui.painter().layout_job(job);
        if galley.elided {
            response.on_hover_text(path);
        }
        ui.painter().galley(
            rect.left_center() + egui::vec2(60.0, -galley.size().y / 2.0),
            galley,
            colors.text,
        );
        ui.painter().text(
            rect.right_center() - egui::vec2(58.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            format!("+{}", if added.is_empty() { "0" } else { added }),
            FontId::proportional(12.0),
            colors.green,
        );
        ui.painter().text(
            rect.right_center() - egui::vec2(14.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            format!("−{}", if removed.is_empty() { "0" } else { removed }),
            FontId::proportional(12.0),
            colors.red,
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
