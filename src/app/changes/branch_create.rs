//! Local create-only branch form; transient intent, no checkout.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
pub(super) struct Create {
    owner: Option<(PathBuf, String)>,
    name: String,
    switch: bool,
    start: String,
    reveal: bool,
    branch_query: String,
    dropdown_open: bool,
    branches_loaded: bool,
    branches_error: Option<String>,
    local_branches: Vec<String>,
    current_branch: Option<String>,
    branches_rx:
        Option<workspace::read_only::Request<Result<workspace::branches::Snapshot, String>>>,
    pending:
        Option<workspace::read_only::Request<Result<workspace::branches::create::Review, String>>>,
    review: Option<workspace::branches::create::Review>,
    applying: Option<workspace::read_only::Request<Result<(), String>>>,
    notice: Option<String>,
    controls: Vec<egui::Id>,
}
impl Changes {
    fn start_branch_list_load(&mut self, ctx: &egui::Context, root: PathBuf) {
        let state = &mut self.branch_create;
        let Some(permit) = branches::permit(ctx) else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        state.owner = Some(self.identity.clone());
        state.branches_loaded = true;
        state.branches_error = None;
        state.branches_rx = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        if std::thread::Builder::new()
            .name("pui-branch-list-picker".into())
            .spawn(move || {
                let result = workspace::branches::read(&root, &cancel);
                drop(permit);
                if !cancel.load(Ordering::Acquire) {
                    let _ = tx.send(result);
                    ctx.request_repaint();
                }
            })
            .is_err()
        {
            state.branches_rx = None;
            state.branches_error = Some("Cannot load local branches.".into());
        }
    }

    fn start_branch_review(&mut self, ctx: &egui::Context, root: PathBuf) {
        let state = &mut self.branch_create;
        let Some(permit) = branches::permit(ctx) else {
            state.notice = Some("Git busy; retry explicitly.".into());
            return;
        };
        let name = state.name.clone();
        let switching = state.switch;
        let start = state.start.clone();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        state.owner = Some(self.identity.clone());
        state.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        if std::thread::Builder::new()
            .name("pui-branch-create-review".into())
            .spawn(move || {
                let result = if switching {
                    workspace::branches::create::inspect_action(&root, &name, true, &cancel)
                } else if !start.is_empty() {
                    workspace::branches::create::inspect_start(&root, &name, &start, &cancel)
                } else {
                    workspace::branches::create::inspect(&root, &name, &cancel)
                };
                drop(permit);
                if !cancel.load(Ordering::Acquire) {
                    let _ = tx.send(result);
                    ctx.request_repaint();
                }
            })
            .is_err()
        {
            state.pending = None;
            state.notice = Some("Cannot inspect branch creation.".into());
        }
    }

    pub(super) fn branch_draft_available(&self) -> bool {
        self.index_writable
            && self.branch_create.pending.is_none()
            && self.branch_create.applying.is_none()
            && self.branch_create.review.is_none()
    }
    pub(super) fn prepare_branch_switch(&mut self, name: String) -> bool {
        if !self.branch_draft_available() || !workspace::branches::create::valid_name(&name) {
            return false;
        }
        self.branch_create=Create{owner:Some(self.identity.clone()),name,switch:true,reveal:true,notice:Some("Highlighted branch copied into draft. Review the switch to inspect clean state and changed paths. Nothing switched.".into()),..Default::default()};
        true
    }
    pub(super) fn prepare_history_branch(&mut self, hash: String) -> bool {
        if !self.branch_draft_available() || !workspace::branches::create::valid_start(&hash) {
            return false;
        }
        self.branch_create=Create{owner:Some(self.identity.clone()),start:hash,reveal:true,notice:Some("History commit copied into draft. Enter a new branch name, then review. No branch created.".into()),..Default::default()};
        true
    }
    pub(super) fn create_branch_ui(&mut self, ui: &mut egui::Ui, workspace: &Workspace) {
        let sheet_open = self.repo_sheet == Some(RepoSheet::CreateBranch);
        let modal_ok = self.repo_sheet.is_some() || ui.memory(|m| m.top_modal_layer().is_none());
        let enabled = self.index_writable
            && workspace.is_git
            && self.branch_create.pending.is_none()
            && self.branch_create.applying.is_none()
            && self.branch_create.review.is_none()
            && !egui::Popup::is_any_open(ui.ctx())
            && modal_ok;
        if sheet_open {
            if self.branch_create.switch
                && workspace.is_git
                && !self.branch_create.branches_loaded
                && self.branch_create.branches_rx.is_none()
            {
                self.start_branch_list_load(ui.ctx(), workspace.root.clone());
            }
            self.branch_create.controls.clear();
            let colors = theme::palette(ui.ctx());
            let active_bg = ui.visuals().selection.bg_fill;
            let selected_tint = egui::Color32::from_rgba_unmultiplied(
                active_bg.r(),
                active_bg.g(),
                active_bg.b(),
                56,
            );
            let total_w = ui.available_width();
            let mut submit = false;
            let mut cancel_sheet = false;
            ui.add_enabled_ui(enabled, |ui| {
                // Inset segmented toggle: Create new | Switch existing
                egui::Frame::new()
                    .fill(colors.sidebar)
                    .stroke(Stroke::new(1.0, colors.border))
                    .corner_radius(7)
                    .inner_margin(egui::Margin::same(3))
                    .show(ui, |ui| {
                        ui.spacing_mut().interact_size.y = 26.0;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 3.0;
                            let tab_w = ((total_w - 11.0) / 2.0).max(80.0);
                            for (is_switch, label) in
                                [(false, "Create new"), (true, "Switch existing")]
                            {
                                let active = self.branch_create.switch == is_switch;
                                let text = if active {
                                    RichText::new(label)
                                        .size(12.0)
                                        .strong()
                                        .color(egui::Color32::WHITE)
                                } else {
                                    RichText::new(label).size(12.0).color(colors.muted)
                                };
                                let btn = ui.add_sized(
                                    [tab_w, 26.0],
                                    egui::Button::new(text)
                                        .fill(if active {
                                            active_bg
                                        } else {
                                            egui::Color32::TRANSPARENT
                                        })
                                        .stroke(Stroke::NONE)
                                        .corner_radius(5),
                                );
                                self.branch_create.controls.push(btn.id);
                                if btn.clicked() && self.branch_create.switch != is_switch {
                                    self.branch_create.switch = is_switch;
                                    self.branch_create.dropdown_open = is_switch;
                                    self.branch_create.branch_query.clear();
                                    if is_switch
                                        && workspace.is_git
                                        && !self.branch_create.branches_loaded
                                        && self.branch_create.branches_rx.is_none()
                                    {
                                        self.start_branch_list_load(
                                            ui.ctx(),
                                            workspace.root.clone(),
                                        );
                                    }
                                }
                            }
                        });
                    });
                ui.add_space(10.0);
                if !self.branch_create.switch {
                    Self::qm_field_label(ui, "New branch name", None);
                    ui.add_space(4.0);
                    let name_r = Self::qm_text_input(
                        ui,
                        &mut self.branch_create.name,
                        "feat/my-branch",
                        1000,
                        true,
                        total_w,
                    );
                    self.branch_create.controls.push(name_r.id);
                    ui.add_space(10.0);
                    Self::qm_field_label(
                        ui,
                        "Start commit",
                        Some("Optional full ID · defaults to HEAD"),
                    );
                    ui.add_space(4.0);
                    let start_r = Self::qm_text_input(
                        ui,
                        &mut self.branch_create.start,
                        "HEAD",
                        64,
                        true,
                        total_w,
                    );
                    self.branch_create.controls.push(start_r.id);
                } else {
                    Self::qm_field_label(ui, "Existing branch", None);
                    ui.add_space(4.0);
                    let (_, trig_rect) = ui.allocate_space(egui::vec2(total_w, 32.0));
                    let trig_resp = ui
                        .interact(
                            trig_rect,
                            egui::Id::new("branch-switch-combo-trigger"),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    self.branch_create.controls.push(trig_resp.id);
                    ui.painter().rect_filled(trig_rect, 7, colors.sidebar);
                    ui.painter().rect_stroke(
                        trig_rect,
                        7,
                        Stroke::new(
                            1.0,
                            if self.branch_create.dropdown_open || trig_resp.hovered() {
                                colors.accent
                            } else {
                                colors.border
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    let has_sel = !self.branch_create.name.is_empty();
                    if has_sel {
                        icons::draw(
                            ui,
                            egui::Rect::from_center_size(
                                trig_rect.left_center() + egui::vec2(15.0, 0.0),
                                Vec2::splat(13.0),
                            ),
                            Icon::Branch,
                            colors.muted,
                        );
                    }
                    ui.painter().text(
                        trig_rect.left_center()
                            + egui::vec2(if has_sel { 28.0 } else { 10.0 }, 0.0),
                        egui::Align2::LEFT_CENTER,
                        if has_sel {
                            self.branch_create.name.as_str()
                        } else {
                            "Select a branch…"
                        },
                        FontId::monospace(12.0),
                        if has_sel { colors.text } else { colors.dim },
                    );
                    ui.painter().text(
                        trig_rect.right_center() - egui::vec2(10.0, 0.0),
                        egui::Align2::RIGHT_CENTER,
                        if self.branch_create.dropdown_open {
                            "▴"
                        } else {
                            "▾"
                        },
                        FontId::proportional(11.0),
                        colors.muted,
                    );
                    if trig_resp.clicked() {
                        self.branch_create.dropdown_open = !self.branch_create.dropdown_open;
                    }
                    if self.branch_create.dropdown_open {
                        ui.add_space(4.0);
                        egui::Frame::new()
                            .fill(colors.surface)
                            .stroke(Stroke::new(1.0, colors.border))
                            .corner_radius(8)
                            .inner_margin(egui::Margin::same(6))
                            .show(ui, |ui| {
                                let inner_w = (total_w - 14.0).max(60.0);
                                ui.set_width(inner_w);
                                let q_resp = Self::qm_text_input(
                                    ui,
                                    &mut self.branch_create.branch_query,
                                    "Search branches…",
                                    128,
                                    false,
                                    inner_w,
                                );
                                self.branch_create.controls.push(q_resp.id);
                                ui.add_space(4.0);
                                if self.branch_create.branches_rx.is_some() {
                                    ui.label(theme::small(ui, "Loading local branches…"));
                                } else if let Some(err) = &self.branch_create.branches_error {
                                    ui.add(egui::Label::new(theme::small(ui, err)).wrap());
                                } else {
                                    let q = self.branch_create.branch_query.trim().to_lowercase();
                                    let matches: Vec<String> = self
                                        .branch_create
                                        .local_branches
                                        .iter()
                                        .filter(|b| q.is_empty() || b.to_lowercase().contains(&q))
                                        .cloned()
                                        .collect();
                                    if matches.is_empty() {
                                        ui.label(theme::small(ui, "No matching branches."));
                                    } else {
                                        egui::ScrollArea::vertical()
                                            .id_salt("branch-switch-combo-scroll")
                                            .max_height(160.0)
                                            .show(ui, |ui| {
                                                ui.spacing_mut().item_spacing.y = 2.0;
                                                for branch_name in matches {
                                                    let is_cur = self
                                                        .branch_create
                                                        .current_branch
                                                        .as_deref()
                                                        == Some(branch_name.as_str());
                                                    let is_sel =
                                                        self.branch_create.name == branch_name;
                                                    let (_, row_rect) = ui
                                                        .allocate_space(egui::vec2(inner_w, 28.0));
                                                    let row_resp = ui
                                                        .interact(
                                                            row_rect,
                                                            egui::Id::new((
                                                                "branch-combo-item",
                                                                &branch_name,
                                                            )),
                                                            Sense::click(),
                                                        )
                                                        .on_hover_cursor(
                                                            egui::CursorIcon::PointingHand,
                                                        );
                                                    self.branch_create.controls.push(row_resp.id);
                                                    if is_sel {
                                                        ui.painter().rect_filled(
                                                            row_rect,
                                                            5,
                                                            selected_tint,
                                                        );
                                                        ui.painter().rect_stroke(
                                                            row_rect,
                                                            5,
                                                            Stroke::new(1.0, colors.accent),
                                                            egui::StrokeKind::Inside,
                                                        );
                                                    } else if row_resp.hovered() {
                                                        ui.painter().rect_filled(
                                                            row_rect,
                                                            5,
                                                            colors.hover,
                                                        );
                                                    }
                                                    icons::draw(
                                                        ui,
                                                        egui::Rect::from_center_size(
                                                            row_rect.left_center()
                                                                + egui::vec2(13.0, 0.0),
                                                            Vec2::splat(12.5),
                                                        ),
                                                        Icon::Branch,
                                                        if is_sel {
                                                            colors.text
                                                        } else {
                                                            colors.muted
                                                        },
                                                    );
                                                    ui.painter().text(
                                                        row_rect.left_center()
                                                            + egui::vec2(26.0, 0.0),
                                                        egui::Align2::LEFT_CENTER,
                                                        &branch_name,
                                                        FontId::monospace(11.5),
                                                        colors.text,
                                                    );
                                                    if is_cur {
                                                        ui.painter().text(
                                                            row_rect.right_center()
                                                                - egui::vec2(8.0, 0.0),
                                                            egui::Align2::RIGHT_CENTER,
                                                            "current",
                                                            FontId::proportional(10.5),
                                                            colors.muted,
                                                        );
                                                    }
                                                    if row_resp.clicked() {
                                                        self.branch_create.name = branch_name;
                                                        self.branch_create.dropdown_open = false;
                                                    }
                                                }
                                            });
                                    }
                                }
                            });
                    }
                    ui.add_space(8.0);
                    egui::Frame::new()
                        .fill(colors.surface)
                        .stroke(Stroke::new(1.0, colors.border))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(9, 6))
                        .show(ui, |ui| {
                            ui.set_width((total_w - 20.0).max(60.0));
                            ui.label(
                                RichText::new("Switching requires a clean working tree and index.")
                                    .size(11.0)
                                    .color(colors.muted),
                            );
                        });
                }
            });
            if let Some(notice) = &self.branch_create.notice {
                ui.add_space(4.0);
                ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
            }
            Self::qm_divider(ui);
            let valid = enabled
                && workspace::branches::create::valid_name(&self.branch_create.name)
                && (self.branch_create.switch
                    || self.branch_create.start.is_empty()
                    || workspace::branches::create::valid_start(&self.branch_create.start));
            prompt_ui::footer(ui, |ui| {
                let review_btn = prompt_ui::button(
                    ui,
                    if self.branch_create.switch {
                        "Review branch switch…"
                    } else {
                        "Review branch creation…"
                    },
                    prompt_ui::Button::Primary,
                    valid,
                );
                self.branch_create.controls.push(review_btn.id);
                if review_btn.clicked() {
                    submit = true;
                }
                if prompt_ui::button(ui, "Cancel", prompt_ui::Button::Secondary, true).clicked() {
                    cancel_sheet = true;
                }
            });
            if cancel_sheet {
                self.repo_sheet = None;
            } else if submit {
                self.start_branch_review(ui.ctx(), workspace.root.clone());
            }
            return;
        }
        self.branch_create.controls.clear();
        let open = std::mem::take(&mut self.branch_create.reveal).then_some(true);
        let mut do_review = false;
        egui::CollapsingHeader::new("Create or switch local branch…")
            .open(open)
            .show(ui, |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    let toggle = ui.checkbox(
                        &mut self.branch_create.switch,
                        "Switch to an existing branch (clean repository only)",
                    );
                    self.branch_create.controls.push(toggle.id);
                    let input = ui.add(
                        egui::TextEdit::singleline(&mut self.branch_create.name)
                            .hint_text(if self.branch_create.switch {
                                "Existing branch name…"
                            } else {
                                "New branch name…"
                            })
                            .char_limit(1000)
                            .desired_width(ui.available_width()),
                    );
                    self.branch_create.controls.push(input.id);
                    if !self.branch_create.switch {
                        ui.label(theme::small(
                            ui,
                            "Start commit (optional full ID; blank uses current HEAD)",
                        ));
                        let start = ui.add(
                            egui::TextEdit::singleline(&mut self.branch_create.start)
                                .char_limit(64)
                                .desired_width(ui.available_width()),
                        );
                        self.branch_create.controls.push(start.id);
                    }
                    let button = prompt_ui::button(
                        ui,
                        if self.branch_create.switch {
                            "Review branch switch…"
                        } else {
                            "Review branch creation…"
                        },
                        prompt_ui::Button::Secondary,
                        workspace::branches::create::valid_name(&self.branch_create.name)
                            && (self.branch_create.switch
                                || self.branch_create.start.is_empty()
                                || workspace::branches::create::valid_start(
                                    &self.branch_create.start,
                                )),
                    );
                    self.branch_create.controls.push(button.id);
                    if button.clicked() {
                        do_review = true;
                    }
                });
                if let Some(notice) = &self.branch_create.notice {
                    ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
                }
            });
        if do_review {
            self.start_branch_review(ui.ctx(), workspace.root.clone());
        }
    }
    pub(super) fn create_branch_inputs(&mut self, ctx: &egui::Context, visible: bool) {
        let s = &mut self.branch_create;
        if !visible || !self.index_writable || s.owner.as_ref().is_some_and(|o| o != &self.identity)
        {
            *s = Default::default();
            return;
        }
        if let Some(result) = s.branches_rx.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Branch list worker ended.".into())),
        }) {
            s.branches_rx = None;
            match result {
                Ok(snapshot) => {
                    s.local_branches = snapshot.local;
                    s.current_branch = match snapshot.current {
                        workspace::branches::Current::Local(name) => Some(name),
                        workspace::branches::Current::Detached => None,
                    };
                    s.branches_error = None;
                }
                Err(e) => s.branches_error = Some(e),
            }
        }
        let mut review_ready = false;
        if let Some(result) = s.pending.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Branch inspection ended.".into())),
        }) {
            s.pending = None;
            match result {
                Ok(review) => {
                    s.review = Some(review);
                    review_ready = true;
                }
                Err(e) => s.notice = Some(e),
            }
        }
        if review_ready {
            self.repo_sheet = None;
        }
        if let Some(result) = s.applying.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(
                "Branch completion uncertain; inspect before retry.".into()
            )),
        }) {
            s.applying = None;
            s.notice=Some(match result{Ok(())=>"Branch operation completed. Refresh/inspect current branch before continuing. No push.".into(),Err(e)=>e});
            self.refresh = true;
            self.inline.clear();
            self.diffs.clear();
            self.git_files.stale = true;
        }
        let focus = ctx.memory(|m| m.focused());
        if s.review.is_some() || focus.is_some_and(|id| s.controls.contains(&id)) {
            ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        }
    }
    pub(super) fn create_branch_dialog(&mut self, ctx: &egui::Context) {
        let Some(review) = &self.branch_create.review else {
            return;
        };
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("create-local-branch"),
        );
        if !self.index_writable
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.branch_create.review = None;
            return;
        }
        let title = if review.switch {
            "Switch local branch?"
        } else {
            "Create local branch?"
        };
        let description = if review.switch {
            "Switches the current branch, index and tracked working files to the reviewed target. Requires completely clean state with no untracked or ignored files. No force, merge, stash or push. Configured checkout filters and submodule trees refuse. Checkout hooks are bypassed. External writes may race; inspect state after any failure."
        } else {
            "Creates only a new local branch at this exact commit. Does not switch branches, modify index/working files, set upstream or push. Reference hooks are bypassed. Existing names are never replaced."
        };
        let choice = confirmations::Dialog::new(
            "create-local-branch",
            title,
            description,
            "Cancel",
            if review.switch {
                "Switch branch"
            } else {
                "Create branch"
            },
        )
        .destructive(false)
        .show(ctx, |ui| {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    format!(
                        "{}\n{}\n{}",
                        review.root.display(),
                        review.name,
                        review.target
                    ),
                ))
                .wrap(),
            );
            if review.switch {
                ui.label(theme::small(
                    ui,
                    format!(
                        "{} changed paths · source → target · renames as delete/add",
                        review.files.len()
                    ),
                ));
                egui::ScrollArea::both()
                    .id_salt("switch-changed-files")
                    .max_height(160.0)
                    .show_rows(ui, 16.0, review.files.len(), |ui, range| {
                        for entry in &review.files[range] {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!(
                                        "{}  {}",
                                        entry.status,
                                        entry.path.escape_debug()
                                    ))
                                    .monospace()
                                    .size(11.0),
                                )
                                .extend(),
                            );
                        }
                    });
                if review.files.is_empty() {
                    ui.label(theme::small(
                        ui,
                        "No tracked content differences between these commits.",
                    ));
                }
            }
        });
        if let Some(choice) = choice {
            let review = self.branch_create.review.take().unwrap();
            if choice == confirmations::Choice::Confirm {
                let Some(permit) = branches::permit(ctx) else {
                    self.branch_create.notice = Some("Git busy; inspect again.".into());
                    return;
                };
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.branch_create.applying =
                    Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-create-branch".into())
                    .spawn(move || {
                        let result = workspace::branches::create::apply(review, &cancel);
                        drop(permit);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    })
                    .is_err()
                {
                    self.branch_create.applying = None;
                    self.branch_create.notice = Some("Cannot start branch creation.".into());
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
