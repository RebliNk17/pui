//! Explicit reviewed clone form; transient intent, no automatic network.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
pub(super) struct Clone {
    owner: Option<(PathBuf, String)>,
    url: String,
    parent: Option<PathBuf>,
    name: String,
    branch: String,
    timeout: Option<u64>,
    pending: Option<workspace::read_only::Request<Result<workspace::clone::Review, String>>>,
    review: Option<workspace::clone::Review>,
    applying: Option<workspace::read_only::Request<Result<(), String>>>,
    notice: Option<String>,
    controls: Vec<egui::Id>,
}

fn suggest_folder_name(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let tail = trimmed.rsplit(['/', ':']).next()?;
    let name = tail.strip_suffix(".git").unwrap_or(tail);
    workspace::clone::valid_dest_name(name).then(|| name.to_owned())
}

impl Changes {
    fn start_clone_review(&mut self, ctx: &egui::Context, parent: PathBuf, use_branch: bool) {
        let s = &mut self.clone;
        let Some(permit) = branches::permit(ctx) else {
            s.notice = Some("Git busy; retry explicitly.".into());
            return;
        };
        let url = s.url.clone();
        let name = s.name.clone();
        let branch = if !use_branch || s.branch.trim().is_empty() {
            None
        } else {
            Some(s.branch.trim().to_owned())
        };
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        s.owner = Some(self.identity.clone());
        s.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        if std::thread::Builder::new()
            .name("pui-clone-review".into())
            .spawn(move || {
                let result =
                    workspace::clone::inspect(&parent, &name, &url, branch.as_deref(), &cancel);
                drop(permit);
                if !cancel.load(Ordering::Acquire) {
                    let _ = tx.send(result);
                    ctx.request_repaint();
                }
            })
            .is_err()
        {
            s.pending = None;
            s.notice = Some("Cannot start clone review.".into());
        }
    }

    pub(super) fn clone_ui(&mut self, ui: &mut egui::Ui, workspace: &Workspace, loading: bool) {
        let enabled = self.index_writable
            && !loading
            && self.clone.pending.is_none()
            && self.clone.applying.is_none()
            && self.clone.review.is_none()
            && !egui::Popup::is_any_open(ui.ctx())
            && (self.repo_sheet.is_some() || ui.memory(|m| m.top_modal_layer().is_none()));
        let sheet_open = self.repo_sheet == Some(RepoSheet::Clone);
        if sheet_open {
            self.clone.controls.clear();
            let colors = theme::palette(ui.ctx());
            let total_w = ui.available_width();
            let default_parent = if workspace.root.as_os_str().is_empty() {
                self.identity.0.clone()
            } else {
                workspace.root.clone()
            };
            let current_parent = self.clone.parent.clone().unwrap_or(default_parent);
            let mut submit = false;
            let mut cancel_sheet = false;
            ui.add_enabled_ui(enabled, |ui| {
                Self::qm_field_label(ui, "Repository URL", Some("No embedded credentials"));
                ui.add_space(4.0);
                let url_resp = Self::qm_text_input(
                    ui,
                    &mut self.clone.url,
                    "https://github.com/org/repo.git",
                    8192,
                    true,
                    total_w,
                );
                self.clone.controls.push(url_resp.id);
                let derived_name = suggest_folder_name(&self.clone.url);
                if let Some(name) = &derived_name {
                    self.clone.name = name.clone();
                } else {
                    self.clone.name.clear();
                }
                ui.add_space(10.0);
                Self::qm_field_label(ui, "Destination folder", Some("Uses remote default branch"));
                ui.add_space(4.0);
                let browse_w = 92.0;
                let path_w = (total_w - browse_w - 6.0).max(100.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let (_, path_rect) = ui.allocate_space(egui::vec2(path_w, 32.0));
                    ui.painter().rect_filled(path_rect, 7, colors.sidebar);
                    ui.painter().rect_stroke(
                        path_rect,
                        7,
                        Stroke::new(1.0, colors.border),
                        egui::StrokeKind::Inside,
                    );
                    let mut job = egui::text::LayoutJob::simple_singleline(
                        current_parent.display().to_string(),
                        FontId::monospace(11.5),
                        colors.muted,
                    );
                    job.wrap.max_width = (path_w - 20.0).max(30.0);
                    job.wrap.max_rows = 1;
                    let galley = ui.painter().layout_job(job);
                    ui.painter().galley(
                        path_rect.left_center() + egui::vec2(10.0, -galley.size().y / 2.0),
                        galley,
                        colors.muted,
                    );
                    let (_, browse_rect) = ui.allocate_space(egui::vec2(browse_w, 32.0));
                    let browse_resp = ui
                        .interact(
                            browse_rect,
                            egui::Id::new("clone-browse-folder"),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    self.clone.controls.push(browse_resp.id);
                    ui.painter().rect_filled(
                        browse_rect,
                        7,
                        if browse_resp.hovered() {
                            colors.hover
                        } else {
                            colors.surface
                        },
                    );
                    ui.painter().rect_stroke(
                        browse_rect,
                        7,
                        Stroke::new(1.0, colors.border),
                        egui::StrokeKind::Inside,
                    );
                    icons::draw(
                        ui,
                        egui::Rect::from_center_size(
                            browse_rect.left_center() + egui::vec2(16.0, 0.0),
                            Vec2::splat(13.0),
                        ),
                        Icon::Folder,
                        colors.muted,
                    );
                    ui.painter().text(
                        browse_rect.left_center() + egui::vec2(28.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        "Browse…",
                        FontId::proportional(12.0),
                        colors.text,
                    );
                    if browse_resp.clicked()
                        && let Some(picked) = rfd::FileDialog::new()
                            .set_directory(&current_parent)
                            .pick_folder()
                    {
                        self.clone.parent = Some(picked);
                    }
                });
                if let Some(name) = &derived_name {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!(
                            "Clones into {}",
                            current_parent.join(name).display()
                        ))
                        .size(11.0)
                        .color(colors.muted),
                    );
                }
            });
            if let Some(notice) = &self.clone.notice {
                ui.add_space(4.0);
                ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
            }
            Self::qm_divider(ui);
            let valid = enabled
                && workspace::push::remote::valid_url(&self.clone.url)
                && workspace::clone::valid_dest_name(&self.clone.name);
            prompt_ui::footer(ui, |ui| {
                let review_btn =
                    prompt_ui::button(ui, "Review clone…", prompt_ui::Button::Primary, valid);
                self.clone.controls.push(review_btn.id);
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
                self.start_clone_review(ui.ctx(), current_parent, false);
            }
            return;
        }
        self.clone.controls.clear();
        let mut do_review = false;
        ui.add_enabled_ui(enabled, |ui| {
            egui::CollapsingHeader::new("Clone repository…").show(ui, |ui| {
                for (label, value, limit) in [
                    (
                        "Repository URL (no embedded credentials)",
                        &mut self.clone.url,
                        8192,
                    ),
                    (
                        "Destination folder name (inside current project)",
                        &mut self.clone.name,
                        255,
                    ),
                ] {
                    ui.label(theme::small(ui, label));
                    let input = ui.add(
                        egui::TextEdit::singleline(value)
                            .char_limit(limit)
                            .desired_width(ui.available_width()),
                    );
                    self.clone.controls.push(input.id);
                }
                ui.label(theme::small(
                    ui,
                    "Start branch (optional; blank uses remote default)",
                ));
                let branch = ui.add(
                    egui::TextEdit::singleline(&mut self.clone.branch)
                        .char_limit(1000)
                        .desired_width(ui.available_width()),
                );
                self.clone.controls.push(branch.id);
                let valid = workspace::push::remote::valid_url(&self.clone.url)
                    && workspace::clone::valid_dest_name(&self.clone.name)
                    && (self.clone.branch.is_empty()
                        || workspace::branches::create::valid_name(&self.clone.branch));
                let response =
                    prompt_ui::button(ui, "Review clone…", prompt_ui::Button::Secondary, valid);
                self.clone.controls.push(response.id);
                if response.clicked() {
                    do_review = true;
                }
            });
            if let Some(notice) = &self.clone.notice {
                ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
            }
        });
        if do_review {
            self.start_clone_review(ui.ctx(), workspace.root.clone(), true);
        }
    }
    pub(super) fn clone_inputs(&mut self, ctx: &egui::Context, visible: bool) {
        let s = &mut self.clone;
        if !visible || !self.index_writable || s.owner.as_ref().is_some_and(|o| o != &self.identity)
        {
            *s = Default::default();
            return;
        }
        let mut review_ready = false;
        if let Some(result) = s.pending.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Clone review ended.".into())),
        }) {
            s.pending = None;
            match result {
                Ok(r) => {
                    s.review = Some(r);
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
                "Clone completion uncertain; inspect the destination folder.".into(),
            )),
        }) {
            s.applying = None;
            s.notice = Some(match result {
                Ok(()) => {
                    "Repository cloned into the destination folder. No push, hooks or force.".into()
                }
                Err(e) => e,
            });
            self.refresh = true;
        }
        let focus = ctx.memory(|m| m.focused());
        if s.review.is_some() || focus.is_some_and(|id| s.controls.contains(&id)) {
            ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        }
    }
    pub(super) fn clone_dialog(&mut self, ctx: &egui::Context) {
        let Some(review) = &self.clone.review else {
            return;
        };
        let own = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("clone-confirm"));
        if !self.index_writable
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.clone.review = None;
            return;
        }
        let choice = confirmations::Dialog::new(
            "clone-confirm",
            "Clone repository?",
            "Clones into an empty destination folder under the current project. Uses conservative SSH settings; interactive login and unknown SSH hosts refuse; credentials may still be used by configured helpers. No hooks, force or push. Remote completion can be uncertain.",
            "Cancel",
            "Clone",
        )
        .destructive(false)
        .show(ctx, |ui| {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    format!(
                        "{}\ninto {}\n{}",
                        review.url,
                        review.parent.join(&review.name).display(),
                        review.branch.as_deref().unwrap_or("remote default branch")
                    ),
                ))
                .wrap(),
            );
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    "Clone timeout (seconds, includes network; cleanup may take longer)",
                ))
                .wrap(),
            );
            let timeout = ui.add(
                egui::DragValue::new(self.clone.timeout.get_or_insert(30)).range(1..=300),
            );
            self.clone.controls.push(timeout.id);
        });
        if let Some(choice) = choice {
            let review = self.clone.review.take().unwrap();
            if choice == confirmations::Choice::Confirm {
                let Some(permit) = branches::permit(ctx) else {
                    self.clone.notice = Some("Git busy; review again.".into());
                    return;
                };
                let seconds = self.clone.timeout.unwrap_or(30);
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.clone.applying = Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-clone".into())
                    .spawn(move || {
                        let result = workspace::clone::apply(review, seconds, &cancel);
                        drop(permit);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    })
                    .is_err()
                {
                    self.clone.applying = None;
                    self.clone.notice = Some("Cannot start clone worker.".into());
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
