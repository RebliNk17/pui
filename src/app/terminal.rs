//! Project-local terminals dock below the chat; hiding never kills the shell.
use super::*;
use crate::terminal::Session;
use std::{collections::BTreeMap, path::Path};
mod capture;
mod chrome;
mod persistence;
mod tabs;
pub(super) use persistence::SavedTabs;

type Selection = Option<((u16, u16), (u16, u16))>;

#[derive(Default)]
pub(super) struct Terminals {
    open: bool,
    expanded: bool,
    root: PathBuf,
    projects: BTreeMap<PathBuf, Project>,
    focus: bool,
    confirm_stop: Option<(PathBuf, u64)>,
    controls: Vec<egui::Id>,
    restored_roots: std::collections::BTreeSet<PathBuf>,
    restore_guard: bool,
    demo_context: bool,
}
#[derive(Default)]
struct Tab {
    id: u64,
    label: u64,
    session: Option<Session>,
    starting: Option<Receiver<Result<Session, String>>>,
    error: Option<String>,
    selection: Selection,
    scroll: f32,
    demo: Option<vt100::Parser>,
    restored: bool,
}

#[derive(Default)]
struct Project {
    tabs: Vec<Tab>,
    selected: usize,
    next_label: u64,
}
impl Project {
    fn active(&self) -> Option<&Tab> {
        self.tabs.get(self.selected)
    }
    fn active_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.selected)
    }
}
impl Tab {
    fn input_id(&self) -> egui::Id {
        egui::Id::new(("project-terminal-input", self.id))
    }
    fn focus(&self, ctx: &egui::Context) {
        ctx.memory_mut(|memory| {
            memory.request_focus_with_filter(
                self.input_id(),
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
    }
    fn running(&self) -> bool {
        self.starting.is_some() || self.session.as_ref().is_some_and(Session::running)
    }
}

impl Terminals {
    pub(super) fn is_open(&self) -> bool {
        self.open
    }
    pub(super) fn context_inputs(&mut self, ctx: &egui::Context, visible: bool) {
        for tab in self
            .projects
            .values_mut()
            .flat_map(|project| project.tabs.iter_mut())
        {
            tabs::poll_tab(tab);
        }
        if visible && self.open {
            capture::inputs(ctx);
            self.tab_inputs(ctx);
        } else {
            capture::clear(ctx);
        }
    }
    pub(super) fn demo_tabs(&mut self, ctx: &egui::Context) {
        self.start(ctx, true);
        self.start(ctx, true);
        if let Some(group) = self.projects.get_mut(&self.root) {
            for tab in &mut group.tabs {
                if let Some(parser) = &mut tab.demo {
                    parser.process(format!("\r\nShell {} · independent demo screen\r\nNo process or provider was started.\r\n",tab.label).as_bytes());
                }
            }
        }
    }
    pub(super) fn demo_context(&mut self) {
        if let Some(project) = self
            .projects
            .get_mut(&self.root)
            .and_then(Project::active_mut)
            && let Some(parser) = project.demo.as_mut()
        {
            parser.process(b"\x1b[2J\x1b[HBuild complete: 42 tests passed.\r\nNo commands are sent by adding context.\r\n");
            project.selection = Some(((0, 0), (0, 31)));
            self.demo_context = true;
        }
    }
    pub(super) fn restore_saved_layout(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        layout: (bool, bool),
    ) {
        self.set_layout(ctx, layout);
        self.restore_guard = true;
        self.root = root.into();
        if layout.0
            && !self.projects.contains_key(root)
            && self.projects.len() + self.restored_roots.len() < tabs::MAX_TOTAL_TABS
            && self.total_tabs() < tabs::MAX_TOTAL_TABS
        {
            self.restored_roots.insert(root.into());
        }
    }
    pub(super) fn layout(&self) -> (bool, bool) {
        (self.open, self.expanded)
    }
    pub(super) fn set_layout(&mut self, ctx: &egui::Context, (open, expanded): (bool, bool)) {
        self.close(ctx);
        self.open = open;
        self.expanded = expanded;
        self.confirm_stop = None;
    }
    pub(super) fn navigation_state(&self) -> Option<bool> {
        self.open.then_some(self.expanded)
    }
    pub(super) fn restore_navigation(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        demo: bool,
        state: Option<bool>,
    ) {
        if let Some(expanded) = state {
            if !self.open {
                self.toggle(ctx, root, demo);
            }
            self.expanded = expanded;
        }
    }
    pub(super) fn close(&mut self, ctx: &egui::Context) {
        capture::clear(ctx);
        self.open = false;
        self.focus = false;
        self.surrender_focus(ctx);
    }
    pub(super) fn confirming_stop(&self) -> bool {
        self.confirm_stop.is_some()
    }
    pub(super) fn focused(&self, ctx: &egui::Context) -> bool {
        self.open && self.owns_focus(ctx)
    }
    pub(super) fn running(&self) -> bool {
        self.projects
            .values()
            .any(|project| project.tabs.iter().any(Tab::running))
    }
    pub(super) fn shutdown(&mut self) {
        self.projects.clear();
        self.restored_roots.clear();
    }
    pub(super) fn toggle(&mut self, ctx: &egui::Context, root: &Path, demo: bool) {
        self.open = !self.open;
        if self.open {
            self.workspace(ctx, root, demo);
            self.focus = true;
        } else {
            capture::clear(ctx);
            self.surrender_focus(ctx);
        }
    }
    fn workspace(&mut self, ctx: &egui::Context, root: &Path, demo: bool) {
        if self.root != root {
            capture::clear(ctx);
            self.root = root.to_path_buf();
            self.focus = true;
            self.confirm_stop = None;
        }
        if !self.projects.contains_key(root)
            && !self.restored_roots.contains(root)
            && !self.restore_guard
        {
            self.start(ctx, demo);
        }
    }
    fn can_start(&self) -> bool {
        let reserved = usize::from(self.restored_roots.contains(&self.root));
        self.projects
            .get(&self.root)
            .is_none_or(|project| project.tabs.len() < tabs::MAX_TABS)
            && self.total_tabs().saturating_sub(reserved) < tabs::MAX_TOTAL_TABS
            && (self.projects.contains_key(&self.root)
                || (self.projects.len() + self.restored_roots.len()).saturating_sub(reserved)
                    < tabs::MAX_TOTAL_TABS)
    }
    fn start(&mut self, ctx: &egui::Context, demo: bool) {
        if !self.can_start() {
            return;
        }
        self.restored_roots.remove(&self.root);
        let label = self
            .projects
            .get(&self.root)
            .map_or(1, |project| project.next_label.saturating_add(1));
        let mut project = Tab {
            id: next_tab_id(),
            label,
            ..Default::default()
        };
        launch(&mut project, ctx, &self.root, demo);
        let group = self.projects.entry(self.root.clone()).or_default();
        group.next_label = label;
        group.tabs.push(project);
        group.selected = group.tabs.len() - 1;
        self.focus = true;
        capture::clear(ctx);
    }
    pub(super) fn show(&mut self, ctx: &egui::Context, root: &Path, demo: bool) -> bool {
        self.workspace(ctx, root, demo);
        let colors = theme::palette(ctx);
        let height = ctx.available_rect().height();
        let frame = egui::Frame::new()
            .fill(colors.bg)
            .inner_margin(egui::Margin::symmetric(16, 8))
            .stroke(Stroke::new(1.0, colors.hover));
        let expanded = self.expanded;
        if expanded {
            egui::CentralPanel::default()
                .frame(frame)
                .show(ctx, |ui| self.content(ui, demo));
        } else {
            egui::TopBottomPanel::bottom("project-terminal")
                .resizable(true)
                .default_height((height * 0.38).clamp(180.0, 320.0))
                .height_range(140.0..=(height * 0.75).max(140.0))
                .frame(frame)
                .show(ctx, |ui| self.content(ui, demo));
        }
        if let Some((root, id)) = self.confirm_stop.clone() {
            let Some(label) = self
                .projects
                .get(&root)
                .and_then(|project| project.tabs.iter().find(|tab| tab.id == id))
                .map(|tab| tab.label)
            else {
                self.confirm_stop = None;
                return expanded;
            };
            let choice = confirmations::Dialog::new(
                "stop-project-terminal",
                "Stop this terminal?",
                "This ends its shell and interrupts any commands still running.",
                "Keep running",
                "Stop terminal",
            )
            .show(ctx, |ui| {
                confirmations::note(
                    ui,
                    &format!(
                        "Shell {} · Project: {}\nOther shell tabs, files and chat sessions will be kept.",
                        label,root.display()
                    ),
                );
            });
            match choice {
                Some(confirmations::Choice::Cancel) => self.confirm_stop = None,
                Some(confirmations::Choice::Confirm) => {
                    self.close_tab(ctx, &root, id);
                    self.confirm_stop = None;
                    // The adjacent surviving PTY gets focus on the next frame,
                    // not the confirmation's Enter event in this one.
                    tabs::consume_activation(ctx);
                }
                None => {}
            }
        }
        expanded
    }
    fn content(&mut self, ui: &mut egui::Ui, demo: bool) {
        if self.restored_roots.contains(&self.root) {
            self.controls.clear();
            prompt_ui::heading(
                ui,
                "Terminal restored · not running",
                "The previous process was not restarted. Start a shell explicitly to continue.",
            );
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                let start = prompt_ui::button(
                    ui,
                    "Start shell",
                    prompt_ui::Button::Primary,
                    self.can_start()
                        && prompt_ui::keyboard_available(ui)
                        && !egui::Popup::is_any_open(ui.ctx()),
                )
                .on_disabled_hover_text(
                    "Close another terminal tab first · maximum 32 per window.",
                );
                self.controls.push(start.id);
                #[cfg(test)]
                ui.ctx().data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new("terminal-legacy-start"),
                        (start.id, start.rect, start.enabled()),
                    );
                });
                if start.clicked() {
                    tabs::consume_activation(ui.ctx());
                    self.start(ui.ctx(), demo);
                    self.focus = true;
                }
                let hide =
                    prompt_ui::button(ui, "Hide terminal", prompt_ui::Button::Secondary, true);
                self.controls.push(hide.id);
                if hide.clicked() {
                    self.close(ui.ctx());
                }
                if self.expanded {
                    let restore =
                        prompt_ui::button(ui, "Restore chat", prompt_ui::Button::Secondary, true);
                    self.controls.push(restore.id);
                    if restore.clicked() {
                        self.expanded = false;
                    }
                }
            });
            return;
        }
        let colors = theme::palette(ui.ctx());
        let mut restart = false;
        let mut capture_button = None;
        self.controls.clear();
        let heading = ui.horizontal(|ui| {
            ui.spacing_mut().interact_size.y = 26.0;
            ui.spacing_mut().item_spacing.x = 6.0;
            icons::inline(ui, Icon::Terminal, colors.muted);
            ui.label(
                RichText::new("Terminal")
                    .size(12.0)
                    .strong()
                    .color(colors.text),
            );
            let path = self.root.to_string_lossy();
            let name = self
                .root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&path);
            let width =
                (ui.available_width() - 4.0 * 26.0 - 4.0 * ui.spacing().item_spacing.x).max(8.0);
            ui.allocate_ui_with_layout(
                Vec2::new(width, 26.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_min_width(width);
                    if width >= 72.0 {
                        ui.label(theme::small(ui, "/"));
                        ui.add(egui::Label::new(theme::small(ui, name)).truncate())
                            .on_hover_text(path.as_ref());
                    }
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let hide = icons::button(ui, Icon::Close, "Hide terminal · commands keep running");
                self.controls.push(hide.id);
                if hide.clicked() {
                    capture::clear(ui.ctx());
                    self.open = false;
                    self.surrender_focus(ui.ctx());
                }
                let expand = icons::button(
                    ui,
                    if self.expanded {
                        Icon::Collapse
                    } else {
                        Icon::Expand
                    },
                    if self.expanded {
                        "Restore terminal"
                    } else {
                        "Expand terminal"
                    },
                );
                self.controls.push(expand.id);
                if expand.clicked() {
                    self.expanded = !self.expanded;
                }
                let available = self
                    .projects
                    .get(&self.root)
                    .and_then(Project::active)
                    .is_some_and(|project| project.session.is_some() || project.demo.is_some());
                capture_button = Some(
                    ui.add_enabled_ui(available, |ui| {
                        let response =
                            icons::button(ui, Icon::Details, "Add terminal context to chat");
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                available,
                                "Add terminal context to chat",
                            )
                        });
                        #[cfg(test)]
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(
                                egui::Id::new("terminal-context-button"),
                                response.rect,
                            )
                        });
                        response
                    })
                    .inner,
                );
                if let Some(button) = &capture_button {
                    self.controls.push(button.id);
                }
                let running = self
                    .projects
                    .get(&self.root)
                    .and_then(Project::active)
                    .is_some_and(Tab::running);
                if running {
                    let stop = icons::button(ui, Icon::Stop, "Stop terminal…");
                    self.controls.push(stop.id);
                    if stop.clicked() {
                        self.confirm_stop = self
                            .projects
                            .get(&self.root)
                            .and_then(Project::active)
                            .map(|tab| (self.root.clone(), tab.id));
                    }
                } else {
                    let new = icons::button(ui, Icon::Refresh, "Start a new terminal");
                    self.controls.push(new.id);
                    if new.clicked() {
                        tabs::consume_activation(ui.ctx());
                        restart = true;
                    }
                }
            });
        });
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new("terminal-heading-bounds"),
                heading.response.rect,
            )
        });
        let _ = heading;
        ui.add_space(4.0);
        if restart {
            self.start(ui.ctx(), demo);
            self.focus = true;
        }
        let rail_width = (ui.available_width() * 0.22).clamp(104.0, 128.0);
        let rail = egui::SidePanel::left("terminal-shell-rail")
            .exact_width(rail_width)
            .resizable(false)
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(4, 4)))
            .show_inside(ui, |ui| self.tab_rail(ui, demo));
        let separator = rail.response.rect.right();
        ui.painter().vline(
            separator,
            rail.response.rect.y_range(),
            Stroke::new(1.0, colors.border.gamma_multiply(0.65)),
        );
        // Header/rail activation owns its Space text pair, never the PTY.
        if ui.ctx().memory(|memory| {
            memory
                .focused()
                .is_some_and(|id| self.controls.contains(&id))
        }) {
            tabs::consume_activation(ui.ctx());
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 4)))
            .show_inside(ui, |ui| self.screen(ui, demo, capture_button.as_ref()));
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new("terminal-shell-rail-bounds"),
                rail.response.rect,
            )
        });
    }
    fn screen(&mut self, ui: &mut egui::Ui, demo: bool, capture_button: Option<&egui::Response>) {
        if !self.projects.contains_key(&self.root) {
            prompt_ui::heading(
                ui,
                "Terminal · not running",
                "Start a new shell explicitly for this project. Nothing was replayed.",
            );
            ui.add_space(12.0);
            let start = prompt_ui::button(
                ui,
                "Start shell",
                prompt_ui::Button::Primary,
                self.can_start()
                    && prompt_ui::keyboard_available(ui)
                    && !egui::Popup::is_any_open(ui.ctx()),
            );
            self.controls.push(start.id);
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("terminal-unknown-start"),
                    (start.id, start.rect, start.enabled()),
                );
            });
            if start.clicked() {
                tabs::consume_activation(ui.ctx());
                self.start(ui.ctx(), demo);
            }
            return;
        }
        let restored = self
            .projects
            .get(&self.root)
            .and_then(Project::active)
            .is_some_and(|tab| tab.restored);
        if restored {
            let tab = self
                .projects
                .get(&self.root)
                .and_then(Project::active)
                .unwrap();
            // Keep an inert, independently owned input surface alive so switching
            // placeholders cannot hand terminal shortcuts to an inline approval.
            let response = ui.interact(
                ui.available_rect_before_wrap(),
                tab.input_id(),
                Sense::click(),
            );
            if self.focus || response.clicked() {
                tab.focus(ui.ctx());
                self.focus = false;
            }
            if response.has_focus() {
                ui.input_mut(|input| {
                    input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
                });
            }
            prompt_ui::heading(
                ui,
                "Terminal restored · not running",
                "Output is not saved. No commands or processes were replayed.",
            );
            ui.add_space(12.0);
            let start = prompt_ui::button(
                ui,
                "Start shell",
                prompt_ui::Button::Primary,
                prompt_ui::keyboard_available(ui) && !egui::Popup::is_any_open(ui.ctx()),
            );
            self.controls.push(start.id);
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("terminal-restored-start"),
                    (start.id, start.rect),
                );
            });
            if start.clicked() {
                tabs::consume_activation(ui.ctx());
                self.activate_restored(ui.ctx(), demo);
            }
            return;
        }
        let colors = theme::palette(ui.ctx());
        let Some(project) = self
            .projects
            .get_mut(&self.root)
            .and_then(Project::active_mut)
        else {
            return;
        };
        tabs::poll_tab(project);
        if let Some(error) = &project.error {
            ui.label(RichText::new(error).size(11.0).color(colors.muted));
        }
        if project.starting.is_some() {
            ui.label(theme::small(ui, "Starting shell…"));
        }
        if let Some(session) = project.session.as_mut() {
            let (status, error) = {
                let state = session.state.lock().unwrap();
                (state.exited.clone(), state.error.clone())
            };
            if let Some(status) = status.or(error) {
                ui.label(theme::small(ui, &status));
            }
        }
        let font = FontId::monospace(12.0);
        let cell = ui.fonts(|f| Vec2::new(f.glyph_width(&font, 'M'), f.row_height(&font).ceil()));
        let size = ui.available_size().max(Vec2::splat(1.0));
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let input_id = project.input_id();
        let response = ui.interact(rect, input_id, Sense::click_and_drag());
        #[cfg(test)]
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new("terminal-output-bounds"), rect));
        if self.focus || response.clicked() || response.drag_started() {
            project.focus(ui.ctx());
            self.focus = false;
        }
        if response.has_focus() {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    input_id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
        }
        if demo
            && std::mem::take(&mut self.demo_context)
            && let Some(button) = &capture_button
        {
            egui::Popup::open_id(ui.ctx(), button.id.with("terminal-context"));
        }
        let rows = ((rect.height() / cell.y) as u16).clamp(1, 200);
        let cols = ((rect.width() / cell.x) as u16).clamp(1, 400);
        if let Some(parser) = project.demo.as_mut() {
            parser.set_size(rows.try_into().unwrap(), cols.try_into().unwrap());
            interact(
                ui,
                &response,
                rect,
                cell,
                parser,
                &mut project.selection,
                &mut project.scroll,
            );
            paint(
                ui,
                rect,
                cell,
                &font,
                parser.screen(),
                project.selection,
                response.has_focus(),
            );
            capture::offer_with_tab(
                &response,
                parser.screen(),
                project.selection,
                &self.root,
                false,
                Some(project.label),
            );
            if let Some(button) = &capture_button {
                capture::offer_with_tab(
                    button,
                    parser.screen(),
                    project.selection,
                    &self.root,
                    true,
                    Some(project.label),
                );
            }
        }
        if let Some(session) = project.session.as_mut() {
            session.resize(rows, cols);
            let mut state = session.state.lock().unwrap();
            let input = interact(
                ui,
                &response,
                rect,
                cell,
                &mut state.parser,
                &mut project.selection,
                &mut project.scroll,
            );
            paint(
                ui,
                rect,
                cell,
                &font,
                state.parser.screen(),
                project.selection,
                response.has_focus(),
            );
            capture::offer_with_tab(
                &response,
                state.parser.screen(),
                project.selection,
                &self.root,
                false,
                Some(project.label),
            );
            if let Some(button) = &capture_button {
                capture::offer_with_tab(
                    button,
                    state.parser.screen(),
                    project.selection,
                    &self.root,
                    true,
                    Some(project.label),
                );
            }
            drop(state);
            if let Err(error) = session.send(input) {
                project.error = Some(error);
            }
        }
    }
}

fn next_tab_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}
fn launch(tab: &mut Tab, ctx: &egui::Context, root: &Path, demo: bool) {
    if demo {
        let mut parser = vt100::Parser::new(16.try_into().unwrap(), 90.try_into().unwrap(), 100);
        parser.process(b"\x1b[32mproject\x1b[0m \x1b[36m$\x1b[0m pwd\r\n");
        parser.process(root.to_string_lossy().as_bytes());
        parser.process(b"\r\n\x1b[32mproject\x1b[0m \x1b[36m$\x1b[0m ");
        tab.demo = Some(parser);
    } else {
        let (tx, rx) = mpsc::channel();
        tab.starting = Some(rx);
        let root = root.to_path_buf();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = Session::start(&ctx, &root, portable_pty::PtySize::default());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
}

fn interact<CB: vt100::Callbacks>(
    ui: &egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
    cell: Vec2,
    parser: &mut vt100::Parser<CB>,
    selection: &mut Selection,
    scroll: &mut f32,
) -> Vec<u8> {
    if egui::Popup::is_any_open(ui.ctx()) || !prompt_ui::keyboard_available(ui) {
        return Vec::new();
    }
    let screen = parser.screen_mut();
    let (rows, cols) = screen.size();
    if response.hovered() {
        *scroll += ui.input(|i| i.smooth_scroll_delta.y) / cell.y;
        let lines = scroll.trunc() as isize;
        if lines != 0 {
            screen.set_scrollback(screen.scrollback().saturating_add_signed(lines));
            *scroll -= lines as f32;
            *selection = None;
        }
    }
    if let Some(pos) = response.interact_pointer_pos() {
        let point_at = |pos: egui::Pos2| {
            (
                (((pos.y - rect.top()) / cell.y) as u16).min(rows.get() - 1),
                (((pos.x - rect.left()) / cell.x) as u16).min(cols.get() - 1),
            )
        };
        let point = point_at(pos);
        if response.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui
                .input(|input| input.pointer.press_origin())
                .unwrap_or(pos);
            *selection = Some((point_at(origin), point));
        }
        if response.dragged_by(egui::PointerButton::Primary) {
            if let Some((_, end)) = selection {
                *end = (point.0, (point.1 + 1).min(cols.get()));
            }
        } else if response.clicked() {
            *selection = None;
        }
    }
    let mut input = Vec::new();
    if response.has_focus() {
        let mut copied = None;
        ui.input_mut(|i| {
            let interrupt = !cfg!(target_os = "macos") && i.modifiers.ctrl && !i.modifiers.shift;
            i.events.retain(|event| {
                if matches!(event, egui::Event::Copy) {
                    if interrupt {
                        input.push(3);
                        return false;
                    }
                    let text = if let Some((a, b)) = *selection {
                        let (a, b) = if a <= b { (a, b) } else { (b, a) };
                        screen.contents_between(a.0, a.1, b.0, b.1)
                    } else {
                        String::new()
                    };
                    if !text.is_empty() {
                        copied = Some(text);
                    }
                    return false;
                }
                if let Some(bytes) =
                    encode(event, screen.application_cursor(), screen.bracketed_paste())
                {
                    input.extend(bytes);
                    return false;
                }
                true
            });
        });
        if let Some(text) = copied {
            ui.ctx().copy_text(text);
        }
        if !input.is_empty() {
            screen.set_scrollback(0);
            *selection = None;
        }
    }
    input
}

fn encode(event: &egui::Event, application_cursor: bool, bracketed: bool) -> Option<Vec<u8>> {
    use egui::{Event, Key};
    match event {
        Event::Text(text) => Some(text.as_bytes().to_vec()),
        Event::Paste(text) => {
            // Strip terminal controls so a paste cannot terminate its own bracket.
            let text: String = text
                .chars()
                .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
                .collect();
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            Some(if bracketed {
                format!("\x1b[200~{text}\x1b[201~").into_bytes()
            } else {
                text.replace('\n', "\r").into_bytes()
            })
        }
        Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => {
            if modifiers.mac_cmd {
                return None;
            }
            if modifiers.ctrl
                && !modifiers.alt
                && matches!(
                    key,
                    Key::A
                        | Key::B
                        | Key::C
                        | Key::D
                        | Key::E
                        | Key::F
                        | Key::G
                        | Key::H
                        | Key::I
                        | Key::J
                        | Key::K
                        | Key::L
                        | Key::M
                        | Key::N
                        | Key::O
                        | Key::P
                        | Key::Q
                        | Key::R
                        | Key::S
                        | Key::T
                        | Key::U
                        | Key::V
                        | Key::W
                        | Key::X
                        | Key::Y
                        | Key::Z
                        | Key::OpenBracket
                        | Key::Backslash
                        | Key::CloseBracket
                        | Key::Space
                )
            {
                let control = match key {
                    Key::A => 1,
                    Key::B => 2,
                    Key::C => 3,
                    Key::D => 4,
                    Key::E => 5,
                    Key::F => 6,
                    Key::G => 7,
                    Key::H => 8,
                    Key::I => 9,
                    Key::J => 10,
                    Key::K => 11,
                    Key::L => 12,
                    Key::M => 13,
                    Key::N => 14,
                    Key::O => 15,
                    Key::P => 16,
                    Key::Q => 17,
                    Key::R => 18,
                    Key::S => 19,
                    Key::T => 20,
                    Key::U => 21,
                    Key::V => 22,
                    Key::W => 23,
                    Key::X => 24,
                    Key::Y => 25,
                    Key::Z => 26,
                    Key::OpenBracket => 27,
                    Key::Backslash => 28,
                    Key::CloseBracket => 29,
                    Key::Space => 0,
                    _ => return None,
                };
                return Some(vec![control]);
            }
            let arrow = match key {
                Key::ArrowUp => Some('A'),
                Key::ArrowDown => Some('B'),
                Key::ArrowRight => Some('C'),
                Key::ArrowLeft => Some('D'),
                Key::Home => Some('H'),
                Key::End => Some('F'),
                _ => None,
            };
            if let Some(arrow) = arrow {
                let modifier = 1
                    + usize::from(modifiers.shift)
                    + 2 * usize::from(modifiers.alt)
                    + 4 * usize::from(modifiers.ctrl);
                return Some(if modifier > 1 {
                    format!("\x1b[1;{modifier}{arrow}").into_bytes()
                } else {
                    format!("\x1b{}{arrow}", if application_cursor { 'O' } else { '[' })
                        .into_bytes()
                });
            }
            let text = match key {
                Key::Enter => "\r",
                Key::Backspace => "\x7f",
                Key::Escape => "\x1b",
                Key::Tab if modifiers.shift => "\x1b[Z",
                Key::Tab => "\t",
                Key::Insert => "\x1b[2~",
                Key::Delete => "\x1b[3~",
                Key::PageUp => "\x1b[5~",
                Key::PageDown => "\x1b[6~",
                Key::F1 => "\x1bOP",
                Key::F2 => "\x1bOQ",
                Key::F3 => "\x1bOR",
                Key::F4 => "\x1bOS",
                Key::F5 => "\x1b[15~",
                Key::F6 => "\x1b[17~",
                Key::F7 => "\x1b[18~",
                Key::F8 => "\x1b[19~",
                Key::F9 => "\x1b[20~",
                Key::F10 => "\x1b[21~",
                Key::F11 => "\x1b[23~",
                Key::F12 => "\x1b[24~",
                _ => return None,
            };
            let mut bytes = Vec::new();
            if modifiers.alt {
                bytes.push(27);
            }
            bytes.extend_from_slice(text.as_bytes());
            Some(bytes)
        }
        _ => None,
    }
}

fn color(color: vt100::Color, default: Color32, bright: bool) -> Color32 {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        vt100::Color::Idx(mut n) => {
            if bright && n < 8 {
                n += 8;
            }
            let rgb = match n {
                0..=15 => [
                    [30, 30, 30],
                    [205, 70, 70],
                    [80, 175, 100],
                    [200, 165, 65],
                    [85, 135, 215],
                    [175, 100, 200],
                    [65, 165, 175],
                    [195, 195, 195],
                    [105, 105, 105],
                    [245, 105, 105],
                    [115, 215, 135],
                    [235, 205, 110],
                    [130, 175, 245],
                    [215, 145, 235],
                    [110, 215, 225],
                    [245, 245, 245],
                ][n as usize],
                16..=231 => {
                    let n = n - 16;
                    let channel = |n| if n == 0 { 0 } else { 55 + n * 40 };
                    [channel(n / 36), channel(n / 6 % 6), channel(n % 6)]
                }
                _ => [8 + (n - 232) * 10; 3],
            };
            Color32::from_rgb(rgb[0], rgb[1], rgb[2])
        }
    }
}

fn paint(
    ui: &egui::Ui,
    rect: egui::Rect,
    cell_size: Vec2,
    font: &FontId,
    screen: &vt100::Screen,
    selection: Selection,
    focused: bool,
) {
    let colors = theme::palette(ui.ctx());
    let painter = ui.painter().with_clip_rect(rect);
    let (rows, cols) = screen.size();
    let selection = selection.map(|(a, b)| if a <= b { (a, b) } else { (b, a) });
    for row in 0..rows.get() {
        for col in 0..cols.get() {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let pos = rect.min + Vec2::new(col as f32 * cell_size.x, row as f32 * cell_size.y);
            let size = Vec2::new(
                cell_size.x * if cell.is_wide() { 2.0 } else { 1.0 },
                cell_size.y,
            );
            let cell_rect = egui::Rect::from_min_size(pos, size);
            let mut fg = color(cell.fgcolor(), colors.text, cell.bold());
            let mut bg = color(cell.bgcolor(), colors.bg, false);
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.dim() {
                fg = fg.gamma_multiply(0.6);
            }
            if selection.is_some_and(|(a, b)| a <= (row, col) && (row, col) < b) {
                bg = colors.accent_bg;
            }
            if bg != colors.bg {
                painter.rect_filled(cell_rect, 0, bg);
            }
            if cell.has_contents() {
                painter.text(
                    pos,
                    egui::Align2::LEFT_TOP,
                    cell.contents(),
                    font.clone(),
                    fg,
                );
                if cell.underline() {
                    painter.line_segment(
                        [cell_rect.left_bottom(), cell_rect.right_bottom()],
                        Stroke::new(1.0, fg),
                    );
                }
            }
        }
    }
    if !screen.hide_cursor() && screen.scrollback() == 0 {
        let (row, col) = screen.cursor_position();
        let cursor = egui::Rect::from_min_size(
            rect.min + Vec2::new(col as f32 * cell_size.x, row as f32 * cell_size.y),
            cell_size,
        );
        if focused {
            painter.rect_filled(cursor, 0, colors.accent.gamma_multiply(0.5));
        } else {
            painter.rect_stroke(
                cursor,
                0,
                Stroke::new(1.0, colors.muted),
                egui::StrokeKind::Inside,
            );
        }
    }
}

#[cfg(test)]
mod restore_tests;
#[cfg(test)]
mod tests;
