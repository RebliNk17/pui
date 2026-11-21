//! Repository-local identity creation; transient review, no global config.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
pub(super) struct Setup {
    owner: Option<(PathBuf, String)>,
    name: String,
    email: String,
    override_inherited: bool,
    edit_local: bool,
    auto_loaded: bool,
    loading: Option<workspace::read_only::Request<Result<(String, String), String>>>,
    pending: Option<
        workspace::read_only::Request<Result<workspace::commit::identity_setup::Review, String>>,
    >,
    review: Option<workspace::commit::identity_setup::Review>,
    applying: Option<workspace::read_only::Request<Result<(), String>>>,
    notice: Option<String>,
    controls: Vec<egui::Id>,
}
impl Changes {
    fn start_identity_load(&mut self, ctx: &egui::Context, root: PathBuf) {
        let s = &mut self.identity_setup;
        let Some(permit) = branches::permit(ctx) else {
            s.notice = Some("Git busy; retry explicitly.".into());
            return;
        };
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        s.owner = Some(self.identity.clone());
        s.loading = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        if std::thread::Builder::new()
            .name("pui-identity-load".into())
            .spawn(move || {
                let result = workspace::commit::identity_setup::load_local(&root, &cancel);
                drop(permit);
                if !cancel.load(Ordering::Acquire) {
                    let _ = tx.send(result);
                    ctx.request_repaint();
                }
            })
            .is_err()
        {
            s.loading = None;
            s.notice = Some("Cannot load local identity.".into());
        }
    }

    fn start_identity_review(&mut self, ctx: &egui::Context, root: PathBuf) {
        let s = &mut self.identity_setup;
        let Some(permit) = branches::permit(ctx) else {
            s.notice = Some("Git busy; retry explicitly.".into());
            return;
        };
        let name = s.name.clone();
        let email = s.email.clone();
        let allow = s.override_inherited;
        let edit = s.edit_local;
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        s.owner = Some(self.identity.clone());
        s.pending = Some(workspace::read_only::Request::new(rx, cancel.clone()));
        let ctx = ctx.clone();
        if std::thread::Builder::new()
            .name("pui-identity-review".into())
            .spawn(move || {
                let result = if edit {
                    workspace::commit::identity_setup::inspect_mode(
                        &root, &name, &email, false, true, &cancel,
                    )
                } else if allow {
                    workspace::commit::identity_setup::inspect_override(
                        &root, &name, &email, true, &cancel,
                    )
                } else {
                    workspace::commit::identity_setup::inspect(&root, &name, &email, &cancel)
                };
                drop(permit);
                if !cancel.load(Ordering::Acquire) {
                    let _ = tx.send(result);
                    ctx.request_repaint();
                }
            })
            .is_err()
        {
            s.pending = None;
            s.notice = Some("Cannot inspect identity setup.".into());
        }
    }

    pub(super) fn identity_setup_ui(&mut self, ui: &mut egui::Ui, workspace: &Workspace) {
        if !workspace.is_git {
            if self.repo_sheet == Some(RepoSheet::Identity) {
                ui.label(theme::small(ui, "Not a Git repository."));
                Self::qm_divider(ui);
                prompt_ui::footer(ui, |ui| {
                    if prompt_ui::button(ui, "Cancel", prompt_ui::Button::Secondary, true).clicked()
                    {
                        self.repo_sheet = None;
                    }
                });
            }
            return;
        }
        let sheet_open = self.repo_sheet == Some(RepoSheet::Identity);
        let modal_ok = self.repo_sheet.is_some() || ui.memory(|m| m.top_modal_layer().is_none());
        let enabled = self.index_writable
            && self.identity_setup.loading.is_none()
            && self.identity_setup.pending.is_none()
            && self.identity_setup.applying.is_none()
            && self.identity_setup.review.is_none()
            && !egui::Popup::is_any_open(ui.ctx())
            && modal_ok;
        if sheet_open {
            if !self.identity_setup.auto_loaded
                && self.identity_setup.loading.is_none()
                && self.identity_setup.name.is_empty()
                && self.identity_setup.email.is_empty()
                && enabled
            {
                self.identity_setup.auto_loaded = true;
                self.start_identity_load(ui.ctx(), workspace.root.clone());
            }
            let colors = theme::palette(ui.ctx());
            let active_bg = ui.visuals().selection.bg_fill;
            let selected_tint = egui::Color32::from_rgba_unmultiplied(
                active_bg.r(),
                active_bg.g(),
                active_bg.b(),
                36,
            );
            self.identity_setup.controls.clear();
            let mut submit = false;
            let mut cancel_sheet = false;
            let total_w = ui.available_width();
            let half_w = ((total_w - 10.0) / 2.0).max(100.0);
            if self.identity_setup.loading.is_some() {
                ui.label(theme::small(ui, "Loading current local identity…"));
            }
            ui.add_enabled_ui(enabled, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.vertical(|ui| {
                        ui.set_width(half_w);
                        Self::qm_field_label(ui, "Name", None);
                        ui.add_space(4.0);
                        let r = Self::qm_text_input(
                            ui,
                            &mut self.identity_setup.name,
                            "Your Name",
                            512,
                            false,
                            half_w,
                        );
                        self.identity_setup.controls.push(r.id);
                    });
                    ui.vertical(|ui| {
                        ui.set_width(half_w);
                        Self::qm_field_label(ui, "Email", None);
                        ui.add_space(4.0);
                        let r = Self::qm_text_input(
                            ui,
                            &mut self.identity_setup.email,
                            "you@example.com",
                            512,
                            false,
                            half_w,
                        );
                        self.identity_setup.controls.push(r.id);
                    });
                });
                ui.add_space(10.0);
                Self::qm_field_label(ui, "Write mode", None);
                ui.add_space(5.0);
                let current_mode = if self.identity_setup.edit_local {
                    1
                } else if self.identity_setup.override_inherited {
                    2
                } else {
                    0
                };
                for (idx, title, desc) in [
                    (
                        0,
                        "Set local identity",
                        "Creates local user.name / user.email if unset",
                    ),
                    (
                        1,
                        "Edit existing local identity",
                        "Reviews old → new local values before saving",
                    ),
                    (
                        2,
                        "Override inherited identity locally",
                        "Writes repo-local config without replacing existing local values",
                    ),
                ] {
                    let selected = current_mode == idx;
                    let (_, rect) = ui.allocate_space(egui::vec2(total_w, 46.0));
                    let resp = ui
                        .interact(rect, egui::Id::new(("id-write-mode", idx)), Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    self.identity_setup.controls.push(resp.id);
                    ui.painter().rect_filled(
                        rect,
                        7,
                        if selected {
                            selected_tint
                        } else if resp.hovered() {
                            colors.hover
                        } else {
                            colors.surface
                        },
                    );
                    ui.painter().rect_stroke(
                        rect,
                        7,
                        Stroke::new(1.0, if selected { colors.accent } else { colors.border }),
                        egui::StrokeKind::Inside,
                    );
                    let dot_center = rect.left_center() + egui::vec2(17.0, 0.0);
                    if selected {
                        ui.painter().circle_filled(dot_center, 7.0, active_bg);
                        ui.painter()
                            .circle_filled(dot_center, 2.5, egui::Color32::WHITE);
                    } else {
                        ui.painter()
                            .circle_stroke(dot_center, 7.0, Stroke::new(1.5, colors.border));
                    }
                    ui.painter().text(
                        egui::pos2(rect.left() + 33.0, rect.top() + 14.0),
                        egui::Align2::LEFT_CENTER,
                        title,
                        FontId::proportional(12.0),
                        colors.text,
                    );
                    ui.painter().text(
                        egui::pos2(rect.left() + 33.0, rect.top() + 31.0),
                        egui::Align2::LEFT_CENTER,
                        desc,
                        FontId::proportional(11.0),
                        colors.muted,
                    );
                    if resp.clicked() {
                        self.identity_setup.edit_local = idx == 1;
                        self.identity_setup.override_inherited = idx == 2;
                    }
                    ui.add_space(5.0);
                }
            });
            if let Some(notice) = &self.identity_setup.notice {
                ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
            }
            Self::qm_divider(ui);
            let valid = enabled
                && workspace::commit::identity_setup::valid(
                    &self.identity_setup.name,
                    &self.identity_setup.email,
                );
            prompt_ui::footer(ui, |ui| {
                if prompt_ui::button(ui, "Cancel", prompt_ui::Button::Secondary, true).clicked() {
                    cancel_sheet = true;
                }
                let review_btn = prompt_ui::button(
                    ui,
                    "Review identity…",
                    prompt_ui::Button::Primary,
                    valid,
                );
                self.identity_setup.controls.push(review_btn.id);
                if review_btn.clicked() {
                    submit = true;
                }
            });
            if cancel_sheet {
                self.repo_sheet = None;
            } else if submit {
                self.start_identity_review(ui.ctx(), workspace.root.clone());
            }
            return;
        }
        self.identity_setup.controls.clear();
        let mut do_load = false;
        let mut do_review = false;
        egui::CollapsingHeader::new("Set repository commit identity…")
            .show(ui, |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    let load = prompt_ui::button(
                        ui,
                        "Load current local identity",
                        prompt_ui::Button::Secondary,
                        true,
                    );
                    self.identity_setup.controls.push(load.id);
                    if load.clicked() {
                        do_load = true;
                        return;
                    }
                    for (label, value) in [
                        ("Name", &mut self.identity_setup.name),
                        ("Email", &mut self.identity_setup.email),
                    ] {
                        ui.label(theme::small(ui, label));
                        let input = ui.add(
                            egui::TextEdit::singleline(value)
                                .char_limit(512)
                                .desired_width(ui.available_width()),
                        );
                        self.identity_setup.controls.push(input.id);
                    }
                    let edit = ui.checkbox(
                        &mut self.identity_setup.edit_local,
                        "Edit existing direct-local identity (review old → new)",
                    );
                    self.identity_setup.controls.push(edit.id);
                    let toggle = ui.checkbox(
                        &mut self.identity_setup.override_inherited,
                        "Override inherited identity locally (never replace local values)",
                    );
                    self.identity_setup.controls.push(toggle.id);
                    let response = prompt_ui::button(
                        ui,
                        "Review local identity…",
                        prompt_ui::Button::Secondary,
                        workspace::commit::identity_setup::valid(
                            &self.identity_setup.name,
                            &self.identity_setup.email,
                        ),
                    );
                    self.identity_setup.controls.push(response.id);
                    if response.clicked() {
                        do_review = true;
                    }
                });
                if let Some(notice) = &self.identity_setup.notice {
                    ui.add(egui::Label::new(theme::small(ui, notice)).wrap());
                }
            });
        if do_load {
            self.start_identity_load(ui.ctx(), workspace.root.clone());
        } else if do_review {
            self.start_identity_review(ui.ctx(), workspace.root.clone());
        }
    }
    pub(super) fn identity_setup_inputs(&mut self, ctx: &egui::Context, visible: bool) {
        let s = &mut self.identity_setup;
        if !visible || !self.index_writable || s.owner.as_ref().is_some_and(|o| o != &self.identity)
        {
            *s = Default::default();
            return;
        }
        if let Some(result) = s.loading.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Local identity load ended; retry explicitly.".into())),
        }) {
            s.loading = None;
            match result {
                Ok((name, email)) => {
                    s.name = name;
                    s.email = email;
                    s.edit_local = true;
                    s.override_inherited = false;
                    s.notice = Some(
                        "Loaded local values into draft only. Edit, then review before saving."
                            .into(),
                    );
                }
                Err(e) => {
                    if !s.auto_loaded {
                        s.notice = Some(e);
                    }
                }
            }
        }
        let mut review_ready = false;
        if let Some(result) = s.pending.as_ref().and_then(|r| match r.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Identity review ended.".into())),
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
                "Identity write uncertain; inspect configuration before retry.".into(),
            )),
        }) {
            s.applying = None;
            s.notice=Some(match result{Ok(())=>"Repository-local name/email saved. Review staged commit again. No commit or network.".into(),Err(e)=>e});
        }
        let focus = ctx.memory(|m| m.focused());
        if s.review.is_some() || focus.is_some_and(|id| s.controls.contains(&id)) {
            ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        }
    }
    pub(super) fn identity_setup_dialog(&mut self, ctx: &egui::Context) {
        let Some(review) = &self.identity_setup.review else {
            return;
        };
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("local-identity-confirm"),
        );
        if !self.index_writable
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.identity_setup.review = None;
            return;
        }
        let choice=confirmations::Dialog::new("local-identity-confirm","Save repository commit identity?","Writes user.name and user.email only to this repository’s shared local Git config. Existing local values are replaced only in explicit Edit mode, shown below. Explicit inherited overrides add local values; inherited/global files stay unchanged. Future commits may publicly expose this identity. No global settings, credentials, commit or network.","Cancel","Save local identity").destructive(false).show(ctx,|ui|{ui.add(egui::Label::new(theme::small(ui,format!("{}\n{} <{}>",review.root.display(),review.name,review.email))).wrap());for(key,value)in &review.inherited{ui.add(egui::Label::new(theme::small(ui,format!("{} {key}: {value} → {}",if review.edit_local{"Previous local"}else{"Inherited"},if key=="user.name"{&review.name}else{&review.email}))).wrap());}});
        if let Some(choice) = choice {
            let review = self.identity_setup.review.take().unwrap();
            if choice == confirmations::Choice::Confirm {
                let Some(permit) = branches::permit(ctx) else {
                    self.identity_setup.notice = Some("Git busy; review again.".into());
                    return;
                };
                let (tx, rx) = mpsc::channel();
                let cancel = Arc::new(AtomicBool::new(false));
                self.identity_setup.applying =
                    Some(workspace::read_only::Request::new(rx, cancel.clone()));
                let ctx = ctx.clone();
                if std::thread::Builder::new()
                    .name("pui-local-identity".into())
                    .spawn(move || {
                        let result = workspace::commit::identity_setup::apply(review, &cancel);
                        drop(permit);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    })
                    .is_err()
                {
                    self.identity_setup.applying = None;
                    self.identity_setup.notice = Some("Cannot start identity write.".into());
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
