//! Read-only local branch popup. Selection is inspection, never a diff base or checkout.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use workspace::branches::{Current, Snapshot};
mod history;

fn popup_id() -> egui::Id {
    egui::Id::new("changes-local-branches")
}
fn history_filter_id() -> egui::Id {
    popup_id().with("history-filter")
}
fn search_id() -> egui::Id {
    popup_id().with("search")
}
fn trigger_id() -> egui::Id {
    popup_id().with("trigger")
}
fn guard_trigger(ctx: &egui::Context) {
    ctx.input_mut(|input| input.events.retain(|event| !matches!(event,
        egui::Event::Key { key: egui::Key::Enter, modifiers, .. } if *modifiers != egui::Modifiers::NONE
    ) && !matches!(event,
        egui::Event::Key { key: egui::Key::Backspace, modifiers, .. } if modifiers.command
    ) && !matches!(event, egui::Event::Key { key: egui::Key::Escape, .. })));
}
#[derive(Clone, PartialEq, Eq)]
struct Owner {
    identity: (PathBuf, String),
    root: PathBuf,
    observed: String,
}
impl Owner {
    fn new(identity: &(PathBuf, String), workspace: &Workspace) -> Option<Self> {
        (workspace.is_git
            && identity.0.as_os_str().len() <= 8192
            && identity.1.len() <= 8192
            && workspace.root.as_os_str().len() <= 8192
            && workspace.branch.len() <= 1024
            && !workspace.branch.chars().any(char::is_control))
        .then(|| Self {
            identity: identity.clone(),
            root: workspace.root.clone(),
            observed: workspace.branch.clone(),
        })
    }
}
struct Pending {
    rx: Receiver<Result<Snapshot, String>>,
    cancel: Arc<AtomicBool>,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
#[derive(Default)]
pub(super) struct Branches {
    open: bool,
    owner: Option<Owner>,
    pending: Option<Pending>,
    snapshot: Option<Snapshot>,
    error: Option<String>,
    stale: bool,
    demo: bool,
    query: String,
    hits: Vec<usize>,
    selected: usize,
    focus: bool,
    reveal: bool,
    trigger: Option<egui::Id>,
    trigger_rect: Option<egui::Rect>,
    refresh_key: bool,
    refresh_id: Option<egui::Id>,
    rows: Vec<egui::Id>,
    focus_search: bool,
    keys: Vec<egui::Event>,
    history: history::History,
    tabs: [Option<egui::Id>; 2],
    tab_key: Option<bool>, // false branches, true history
    #[cfg(test)]
    requests: usize,
}
impl Branches {
    pub(super) fn cancel(&mut self) {
        *self = Self::default(); // drops delivery, signals active child; no background retry
    }
    fn project(&mut self) {
        let query = self.query.trim().to_lowercase();
        self.hits = self
            .snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .local
                    .iter()
                    .enumerate()
                    .filter_map(|(n, name)| name.to_lowercase().contains(&query).then_some(n))
                    .collect()
            })
            .unwrap_or_default();
        self.selected = self.selected.min(self.hits.len().saturating_sub(1));
    }
    fn request(&mut self, ctx: &egui::Context, owner: Owner) {
        #[cfg(test)]
        {
            self.requests += 1;
        }
        self.pending = None;
        self.history = Default::default();
        self.owner = Some(owner.clone());
        self.snapshot = None;
        self.hits.clear();
        self.error = None;
        self.stale = false;
        let Some(permit) = permit(ctx) else {
            self.error = Some("A local branch read is still finishing. Refresh to retry.".into());
            return;
        };
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.pending = Some(Pending {
            rx,
            cancel: cancel.clone(),
        });
        let repaint = ctx.clone();
        if std::thread::Builder::new()
            .name("pui-local-branches".into())
            .spawn(move || {
                let result = workspace::branches::read(&owner.root, &cancel);
                drop(permit);
                if !cancel.load(Ordering::Acquire) {
                    let _ = tx.send(result);
                    repaint.request_repaint();
                }
            })
            .is_err()
        {
            self.pending = None;
            self.error = Some("Cannot start local branch worker. Refresh to retry.".into());
        }
    }
    fn poll(&mut self, owner: Option<Owner>) {
        self.history.poll(owner.clone());
        if self.owner != owner && self.open {
            self.pending = None;
            self.snapshot = None;
            self.hits.clear();
            self.stale = true;
        }
        if self.stale {
            return;
        }
        let result = self
            .pending
            .as_ref()
            .and_then(|pending| match pending.rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Local branch worker ended without a result. Refresh to retry.".into(),
                )),
            });
        if let Some(result) = result {
            self.pending = None;
            match result {
                Ok(snapshot) => self.snapshot = Some(snapshot),
                Err(error) => self.error = Some(error),
            }
            self.project();
        }
    }
    fn close(&mut self, ctx: &egui::Context, restore: bool) {
        let trigger = self.trigger;
        let trigger_rect = self.trigger_rect;
        self.cancel();
        self.trigger_rect = trigger_rect;
        egui::Popup::close_id(ctx, popup_id());
        if restore && let Some(trigger) = trigger {
            ctx.memory_mut(|memory| memory.request_focus(trigger));
        }
    }
}
fn bounded_key(event: &egui::Event) -> egui::Event {
    let clipped = |text: &str| text.chars().take(128).collect();
    match event {
        egui::Event::Text(text) => egui::Event::Text(clipped(text)),
        egui::Event::Paste(text) => egui::Event::Paste(clipped(text)),
        egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
            egui::Event::Ime(egui::ImeEvent::Commit(clipped(text)))
        }
        egui::Event::Ime(egui::ImeEvent::Preedit(text)) => {
            egui::Event::Ime(egui::ImeEvent::Preedit(clipped(text)))
        }
        _ => event.clone(),
    }
}
fn keyboard_event(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Key { .. }
            | egui::Event::Text(_)
            | egui::Event::Paste(_)
            | egui::Event::Copy
            | egui::Event::Cut
            | egui::Event::Ime(_)
    )
}
struct Permit(Arc<AtomicBool>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
fn permit(ctx: &egui::Context) -> Option<Permit> {
    let gate = ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<AtomicBool>>(popup_id().with("worker"))
            .clone()
    });
    gate.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .ok()
        .map(|_| Permit(gate))
}

impl Changes {
    /// Explicit screenshot fixture only: seeds one bounded snapshot; no worker,
    /// repository inspection, provider call, timer or repeated popup reopening.
    pub(in crate::app) fn demo_branches(
        &mut self,
        ctx: &egui::Context,
        cwd: &std::path::Path,
        session: &str,
        workspace: &Workspace,
    ) {
        self.bind(cwd, session);
        self.branches = Branches {
            open: true,
            demo: true,
            focus: true,
            owner: Owner::new(&self.identity, workspace),
            snapshot: Some(Snapshot {
                current: Current::Local("fixture/main".into()),
                local: ["fixture/main", "fixture/read-only-review", "fixture/日本語"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            }),
            ..Default::default()
        };
        self.branches.project();
        egui::Popup::open_id(ctx, popup_id());
        ctx.memory_mut(|memory| memory.request_focus(search_id()));
    }
    pub(in crate::app) fn close_branches(&mut self, ctx: &egui::Context) {
        self.branches.close(ctx, false);
    }
    pub(super) fn poll_branches(&mut self, workspace: &Workspace) {
        self.branches.poll(Owner::new(&self.identity, workspace));
    }
    pub(super) fn branch_inputs(
        &mut self,
        ctx: &egui::Context,
        visible: bool,
        workspace: &Workspace,
    ) {
        if !visible {
            self.branches.close(ctx, false);
            return;
        }
        if !self.branches.open || !egui::Popup::is_id_open(ctx, popup_id()) {
            self.branches.close(ctx, false);
            // A closed, keyboard-focused trigger owns modified activation too;
            // native clickable widgets otherwise synthesize Cmd-Enter clicks.
            if !egui::Popup::is_any_open(ctx)
                && ctx.memory(|memory| memory.top_modal_layer().is_none())
                && (ctx.memory(|memory| memory.focused()) == Some(trigger_id())
                    || self.branches.trigger_rect.is_some_and(|rect| {
                        ctx.input(|input| {
                            input.pointer.any_click()
                                && input
                                    .pointer
                                    .interact_pos()
                                    .is_some_and(|pos| rect.contains(pos))
                        })
                    }))
            {
                guard_trigger(ctx);
            }
            return;
        }
        self.branches.poll(Owner::new(&self.identity, workspace));
        if ctx.memory(|memory| memory.top_modal_layer().is_some()) {
            self.close_branches(ctx);
            return;
        }
        let branch = &mut self.branches;
        let focused = ctx.memory(|memory| memory.focused());
        let row_focus =
            focused.and_then(|focused| branch.rows.iter().position(|id| *id == focused));
        if !branch.history.active
            && focused != Some(search_id())
            && focused != branch.refresh_id
            && row_focus.is_none()
            && !branch.tabs.contains(&focused)
        {
            branch.focus_search = true;
            ctx.memory_mut(|memory| memory.request_focus(search_id()));
        }
        let mut close = false;
        let mut delta = 0isize;
        // Runs before global Stop, approvals/questions, composer and presets.
        // Strip activation keys on every popup subview, not just the search field.
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                let egui::Event::Key {
                    key,
                    pressed,
                    repeat,
                    modifiers,
                    ..
                } = event
                else {
                    return true;
                };
                match key {
                    egui::Key::Escape => {
                        close |= *pressed;
                        false
                    }
                    egui::Key::Enter => {
                        if *pressed && !repeat && *modifiers == egui::Modifiers::NONE {
                            branch.history.filter_key(focused);
                            branch.history.detail_inputs(focused, *key);
                            if let Some(tab) = focused.and_then(|focused| {
                                branch.tabs.iter().position(|id| *id == Some(focused))
                            }) {
                                branch.tab_key = Some(tab == 1);
                            }
                            branch.refresh_key |=
                                branch.refresh_id.is_some() && focused == branch.refresh_id;
                            if let Some(position) = row_focus {
                                branch.selected = position;
                            } else {
                                branch.focus_search |= !branch.history.active
                                    && focused != branch.refresh_id
                                    && !branch.tabs.contains(&focused);
                            }
                        }
                        false
                    }
                    egui::Key::Backspace if modifiers.command => false,
                    egui::Key::ArrowUp | egui::Key::ArrowDown => {
                        if *pressed && *modifiers == egui::Modifiers::NONE {
                            delta += if *key == egui::Key::ArrowUp { -1 } else { 1 };
                        }
                        false
                    }
                    egui::Key::ArrowLeft | egui::Key::ArrowRight if modifiers.alt => false,
                    egui::Key::Space
                        if focused != Some(search_id()) && focused != Some(history_filter_id()) =>
                    {
                        if *pressed && !repeat && *modifiers == egui::Modifiers::NONE {
                            branch.history.filter_key(focused);
                            branch.history.detail_inputs(focused, *key);
                            if let Some(tab) = focused.and_then(|focused| {
                                branch.tabs.iter().position(|id| *id == Some(focused))
                            }) {
                                branch.tab_key = Some(tab == 1);
                            }
                            if let Some(position) = row_focus {
                                branch.selected = position;
                            }
                            branch.refresh_key |=
                                branch.refresh_id.is_some() && focused == branch.refresh_id;
                        }
                        false
                    }
                    _ => true,
                }
            })
        });
        if focused != Some(search_id()) && focused != Some(history_filter_id()) {
            ctx.input_mut(|input| {
                input
                    .events
                    .retain(|event| !matches!(event, egui::Event::Text(text) if text == " "))
            });
        }
        branch.keys.clear();
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                if keyboard_event(event) {
                    if branch.keys.len() < 128 {
                        branch.keys.push(bounded_key(event));
                    }
                    false
                } else {
                    true
                }
            })
        });
        if !branch.history.active && delta != 0 && !branch.hits.is_empty() {
            branch.selected = branch
                .selected
                .saturating_add_signed(delta)
                .min(branch.hits.len() - 1);
            branch.reveal = true;
            branch.focus_search = true;
        }
        if close {
            branch.close(ctx, true);
        }
    }
    pub(super) fn branch_button(&mut self, ui: &mut egui::Ui, workspace: &Workspace) {
        let ctx = ui.ctx().clone();
        let owner = Owner::new(&self.identity, workspace);
        let branch = &mut self.branches;
        let label = branch
            .snapshot
            .as_ref()
            .filter(|_| branch.owner == owner && !branch.stale)
            .map(|s| match &s.current {
                Current::Local(name) => name.as_str(),
                Current::Detached => "detached HEAD",
            })
            .unwrap_or(if owner.is_some() {
                &workspace.branch
            } else {
                "Branches"
            });
        let full = format!(
            "Local branches · read-only\nLast observed: {label}\n{}\nSwitching branches is not implemented. Does not choose the Changes diff base.",
            workspace.root.display()
        );
        let width = (ui.available_width() - 8.0).clamp(28.0, 136.0);
        let (_, rect) = ui.allocate_space(egui::vec2(width, 26.0));
        branch.trigger_rect = Some(rect);
        if ctx.memory(|memory| memory.focused()) == Some(trigger_id())
            || ctx.input(|input| {
                input
                    .pointer
                    .interact_pos()
                    .is_some_and(|pos| rect.contains(pos))
            })
        {
            guard_trigger(&ctx);
        }
        let response = ui
            .interact(rect, trigger_id(), Sense::click())
            .on_hover_text(full);
        let colors = theme::palette(&ctx);
        if response.hovered() || branch.open {
            ui.painter().rect_filled(rect, 5, colors.hover);
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                5,
                Stroke::new(1.0, colors.accent),
                egui::StrokeKind::Inside,
            );
        }
        icons::draw(
            ui,
            egui::Rect::from_center_size(
                rect.left_center() + egui::vec2(12.0, 0.0),
                Vec2::splat(14.0),
            ),
            Icon::Branch,
            colors.muted,
        );
        let mut job = egui::text::LayoutJob::simple_singleline(
            label.into(),
            FontId::proportional(11.0),
            colors.muted,
        );
        job.wrap.max_width = (width - 29.0).max(0.0);
        job.wrap.max_rows = 1;
        let galley = ui.painter().layout_job(job);
        if width > 40.0 {
            ui.painter().galley(
                rect.left_center() + egui::vec2(26.0, -galley.size().y / 2.0),
                galley,
                colors.muted,
            );
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                owner.is_some(),
                "Local branches · read-only",
            )
        });
        #[cfg(test)]
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("branch-trigger"),
                (response.id, response.rect),
            )
        });
        if response.clicked() && owner.is_some() {
            if branch.open {
                branch.close(&ctx, true);
            } else {
                // Pointer-open plus an approval/send chord in the same frame
                // must not reach the underlying entry, presets or composer.
                ctx.input_mut(|input| input.events.retain(|event| !keyboard_event(event)));
                branch.open = true;
                branch.focus = true;
                branch.trigger = Some(response.id);
                branch.request(&ctx, owner.clone().unwrap());
                egui::Popup::open_id(&ctx, popup_id());
                ctx.memory_mut(|memory| memory.request_focus(search_id()));
            }
        }
        if !branch.open {
            return;
        }
        if ctx.memory(|memory| memory.top_modal_layer().is_some()) {
            branch.close(&ctx, false);
            return;
        }
        let width = (ctx.screen_rect().width() - 32.0).clamp(120.0, 380.0);
        egui::Popup::from_response(&response).id(popup_id()).open_memory(None)
            .width(width).layout(Layout::top_down_justified(Align::Min))
            .frame(prompt_ui::popup(&ctx)).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.input_mut(|input| input.events.append(&mut branch.keys));
                ui.set_width((width - 20.0).max(1.0));
                prompt_ui::style(ui);
                egui::ScrollArea::vertical().animated(false).max_height((ctx.screen_rect().height() - 100.0).clamp(80.0, 440.0)).show(ui, |ui| {
                    prompt_ui::heading(ui, if branch.history.active {"Recent history"}else{"Local branches"}, if branch.history.active {"Read-only · pinned local HEAD"}else{"Read-only · switching branches not implemented"});
                    ui.horizontal(|ui| {
                        let branches=ui.add(egui::Button::selectable(!branch.history.active,"Branches"));
                        let history=ui.add(egui::Button::selectable(branch.history.active,"History"));
                        branch.tabs=[Some(branches.id),Some(history.id)];
                        let tab_key=branch.tab_key.take();
                        #[cfg(test)] ctx.data_mut(|data|data.insert_temp(popup_id().with("history-tab"),(history.id,history.rect)));
                        if branches.clicked() || tab_key==Some(false) {branch.history.active=false;branch.history.cancel_read();branch.focus=true;}
                        if history.clicked() || tab_key==Some(true) {
                            branch.history.active=true;branch.pending=None;branch.rows.clear();
                            if !branch.history.loaded && !branch.demo && !branch.stale && let Some(owner)=owner.clone(){branch.history.request(&ctx,owner);}
                        }
                    });
                    if branch.demo { ui.add(egui::Label::new(theme::small(ui, "Demo fixture · synthetic local/current names · no repository read")).wrap()); }
                    if branch.history.active {
                        branch.rows.clear();
                        branch.history.show(ui,branch.stale,branch.demo);
                    } else {
                    ui.add(egui::Label::new(theme::small(ui, "Snapshot only · highlight to inspect. Changes still compares the working tree with HEAD.")).wrap());
                    let search = ui.add(egui::TextEdit::singleline(&mut branch.query).id(search_id()).char_limit(128).desired_width(ui.available_width()).hint_text("Filter local branches…"));
                    if std::mem::take(&mut branch.focus) || std::mem::take(&mut branch.focus_search) { search.request_focus(); }
                    if search.changed() { branch.selected = 0; branch.project(); }
                    branch.rows.clear();
                    if branch.stale { ui.add(egui::Label::new(theme::small(ui, "Repository or workspace observation changed. Refresh to read this root; no old list is shown.")).wrap()); }
                    else if branch.pending.is_some() { ui.label(theme::small(ui, "Reading local branches…")); }
                    else if let Some(error) = &branch.error { ui.add(egui::Label::new(theme::small(ui, error)).wrap()); }
                    else if let Some(snapshot) = &branch.snapshot {
                        let status = match &snapshot.current {
                            Current::Local(name) => format!("Current local: {name}{}", if snapshot.local.contains(name) { "" } else { " · unborn or no local ref" }),
                            Current::Detached => "Current: detached HEAD · no local branch selected".into(),
                        };
                        ui.add(egui::Label::new(theme::small(ui, status)).wrap());
                        if branch.hits.is_empty() { ui.label(theme::small(ui, if snapshot.local.is_empty() { "No local branches." } else { "No matching local branches." })); }
                        for (position, &index) in branch.hits.iter().enumerate() {
                            let name = &snapshot.local[index];
                            let current = snapshot.current == Current::Local(name.clone());
                            let text = if current { format!("{name} · current") } else { name.clone() };
                            let row = ui.add(egui::Button::selectable(position == branch.selected, RichText::new(text).size(12.0)).wrap());
                            branch.rows.push(row.id);
                            if row.clicked() { branch.selected = position; }
                            if branch.reveal && position == branch.selected { branch.reveal = false; row.scroll_to_me(Some(Align::Center)); }
                            #[cfg(test)] ctx.data_mut(|data| data.insert_temp(popup_id().with(("row", position)), (row.id, row.rect)));
                        }
                        if let Some(name) = branch.hits.get(branch.selected).map(|&n| &snapshot.local[n]) {
                            ui.add(egui::Label::new(theme::small(ui, format!("Highlighted local: {name} · {}", if snapshot.current == Current::Local(name.clone()) { "current" } else { "not current" }))).wrap());
                        }
                    }
                    }
                    let mut refresh = std::mem::take(&mut branch.refresh_key);
                    if !branch.history.has_detail() {
                    prompt_ui::footer(ui, |ui| {
                        let response = ui.push_id("branch-refresh", |ui| prompt_ui::button(ui, "Refresh", prompt_ui::Button::Secondary, !branch.demo && branch.pending.is_none() && !branch.history.pending() && !branch.history.has_detail() && owner.is_some())).inner;
                        branch.refresh_id = Some(response.id);
                        refresh |= response.clicked();
                        #[cfg(test)] ctx.data_mut(|data| data.insert_temp(popup_id().with("refresh-response"), (response.id, response.rect)));
                    });
                    } else {branch.refresh_id=None;}
                    if refresh && !branch.demo && branch.pending.is_none() && !branch.history.pending() && !branch.history.has_detail() && let Some(owner) = owner.clone() {
                        if branch.history.active {branch.stale=false;branch.owner=Some(owner.clone());branch.history.request(&ctx,owner);}
                        else {branch.request(&ctx, owner);}
                    }
                });
                // Agent input/select questions render after Changes and must not
                // receive this popup's editing/preset keys, even after focus theft.
                ui.input_mut(|input| input.events.retain(|event| !keyboard_event(event)));
            });
        if !egui::Popup::is_id_open(&ctx, popup_id()) {
            branch.close(&ctx, false);
        }
    }
}

#[cfg(test)]
mod interaction_tests;
#[cfg(test)]
mod tests;
