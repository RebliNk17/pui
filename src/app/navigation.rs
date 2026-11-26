//! Window-local view history. Restoring a location never aborts an agent or shell.
use super::*;

#[cfg(test)]
mod tests;

const MAX_HISTORY: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Page {
    Chat,
    Task {
        task: String,
        run: Option<String>,
        tab: tasks::work_item::Tab,
    },
    Board,
    Scheduled,
    Settings(String),
    Files,
    Browser,
    Changes,
}

#[derive(Clone, Debug)]
struct Chat {
    agent: String,
    anonymous: Option<u64>,
    path: Option<PathBuf>,
    cwd: PathBuf,
    title: String,
}
impl Chat {
    fn same(&self, other: &Self) -> bool {
        if let (Some(path), Some(other_path)) = (&self.path, &other.path) {
            path == other_path
        } else if !self.agent.is_empty() && !other.agent.is_empty() {
            self.agent == other.agent
        } else {
            // An unsaved, not-yet-connected chat has a window-local identity,
            // never a workspace-based identity or a synthetic live agent.
            self.anonymous.is_some() && self.anonymous == other.anonymous
        }
    }
}

#[derive(Clone, Debug)]
struct Location {
    chat: Chat,
    page: Page,
    terminal: Option<bool>,
    task: Option<(String, Option<String>, tasks::work_item::Tab)>,
    file_target: Option<String>,
}
impl Location {
    fn same(&self, other: &Self) -> bool {
        self.chat.same(&other.chat)
            && self.page == other.page
            && self.terminal == other.terminal
            && self.task == other.task
    }
}

#[derive(Default)]
pub(super) struct Navigation {
    entries: Vec<Location>,
    cursor: usize,
    anonymous: u64,
}
impl Navigation {
    fn observe(&mut self, location: Location) {
        if let Some(current) = self.entries.get_mut(self.cursor) {
            if current.same(&location) {
                // Session paths and titles may arrive asynchronously; that's not navigation.
                *current = location;
                return;
            }
        }
        self.entries.truncate(self.cursor + 1);
        self.entries.push(location);
        if self.entries.len() > MAX_HISTORY {
            self.entries.remove(0);
        }
        self.cursor = self.entries.len() - 1;
    }
    fn can_go(&self, forward: bool) -> bool {
        if forward {
            self.cursor + 1 < self.entries.len()
        } else {
            self.cursor > 0
        }
    }
}

impl Pui {
    fn navigation_location(&self) -> Location {
        let page = if self.settings_open {
            Page::Settings(self.settings_view.location())
        } else if self.automation.open {
            Page::Scheduled
        } else if self.kanban.open {
            Page::Board
        } else if self.files.browser_open() {
            Page::Browser
        } else if self.files.files_open() {
            Page::Files
        } else if self.inspector_panel {
            Page::Changes
        } else if self.tasks.open {
            self.tasks.current().map_or(Page::Chat, |task| Page::Task {
                task: task.id.clone(),
                run: self.tasks.selected_run.clone(),
                tab: self.work_item_tab(&task.id),
            })
        } else {
            Page::Chat
        };
        Location {
            chat: Chat {
                agent: self.agents.active.clone(),
                anonymous: (self.agents.active.is_empty()
                    && self.pending_session.is_none()
                    && self.state.session_file.is_none())
                .then_some(self.navigation.anonymous),
                path: self
                    .pending_session
                    .clone()
                    .or_else(|| self.state.session_file.as_ref().map(PathBuf::from)),
                cwd: self.prefs.cwd.clone(),
                title: self
                    .state
                    .chat_title()
                    .unwrap_or_else(|| "New conversation".into()),
            },
            terminal: if matches!(
                page,
                Page::Chat | Page::Task { .. } | Page::Files | Page::Browser | Page::Changes
            ) {
                self.terminals.navigation_state()
            } else {
                None
            },
            file_target: if matches!(page, Page::Files | Page::Browser) {
                self.files.navigation_target()
            } else {
                None
            },
            task: (self.tasks.open
                && matches!(
                    page,
                    Page::Task { .. } | Page::Files | Page::Browser | Page::Changes
                ))
            .then(|| {
                self.tasks.current().map(|task| {
                    (
                        task.id.clone(),
                        self.tasks.selected_run.clone(),
                        self.work_item_tab(&task.id),
                    )
                })
            })
            .flatten(),
            page,
        }
    }

    pub(super) fn remember_navigation(&mut self) {
        if !self.agents.background {
            self.navigation.observe(self.navigation_location());
        }
    }

    pub(super) fn bind_navigation_agent(&mut self, id: &str) {
        if self.agents.background {
            return;
        }
        for location in &mut self.navigation.entries {
            if location.chat.anonymous == Some(self.navigation.anonymous) {
                location.chat.agent = id.into();
                location.chat.anonymous = None;
            }
        }
        self.navigation.anonymous = self.navigation.anonymous.wrapping_add(1);
    }

    fn navigation_blocked(&self) -> bool {
        self.search_open
            || self.tasks.attachment_confirming()
            || self.task_operations_confirming()
            || self.section_view.confirming()
            || self.agents.close_prompt
            || self.terminals.confirming_stop()
            || self.history_view.is_open()
            || self.deletion.confirming()
            || self.automation.confirming()
            || self.menu.about
            || self.resource_reload.confirming()
            || self.updates.open
            || !self.state.dialogs.is_empty()
            || self.transcript.gallery.is_open()
            || self.draft_images.gallery.is_open()
            || (self.settings_open && self.settings_view.navigation_blocked())
            || self.ctx.memory(|memory| memory.top_modal_layer().is_some())
    }

    fn navigation_agent(&self, chat: &Chat) -> Option<String> {
        let Some(path) = &chat.path else {
            return (!chat.agent.is_empty()).then(|| chat.agent.clone());
        };
        self.agent_for_path(path).or_else(|| {
            let state = if chat.agent == self.agents.active {
                Some(&self.state)
            } else {
                self.agents
                    .parked
                    .get(&chat.agent)
                    .map(|parked| &parked.state)
            };
            state
                .filter(|state| {
                    state.session_file.as_deref().map(std::path::Path::new) == Some(path.as_path())
                })
                .map(|_| chat.agent.clone())
        })
    }

    fn location_available(&self, location: &Location) -> bool {
        if location
            .chat
            .path
            .as_ref()
            .is_some_and(|p| self.deletion.deleting_chat(p))
        {
            return false;
        }
        let live_agent = |id: &str| {
            id == self.agents.active
                || self.agents.parked.get(id).is_some_and(|chat| {
                    chat.client.is_some()
                        || chat
                            .state
                            .session_file
                            .as_ref()
                            .is_none_or(|path| std::path::Path::new(path).is_file())
                })
        };
        let chat_available = location.chat.same(&self.navigation_location().chat)
            || self
                .navigation_agent(&location.chat)
                .is_some_and(|id| live_agent(&id))
            || location
                .chat
                .path
                .as_ref()
                .is_some_and(|path| path.is_file());
        chat_available
            && match &location.page {
                Page::Task { task, .. } => self.tasks.records.iter().any(|t| t.id == *task),
                _ => true,
            }
    }

    fn restore_location(&mut self, location: &Location) -> bool {
        if !self.location_available(location) {
            return false;
        }
        if !location.chat.same(&self.navigation_location().chat) {
            let agent = self.navigation_agent(&location.chat);
            if let Some(agent) =
                agent.filter(|id| id == &self.agents.active || self.agents.parked.contains_key(id))
            {
                self.view_agent(&agent);
            } else if let Some(path) = &location.chat.path {
                self.open_chat(Session {
                    path: path.clone(),
                    cwd: location.chat.cwd.clone(),
                    project: location.chat.cwd.clone(),
                    title: location.chat.title.clone(),
                    created: None,
                    modified: std::time::SystemTime::now(),
                });
            }
        }
        self.settings_open = false;
        self.kanban.open = false;
        self.automation.open = false;
        self.tasks.open = false;
        self.files.close();
        self.inspector_panel = false;
        self.pr_pane.close();
        self.close_model_popovers();
        self.terminals.close(&self.ctx);
        if let Some((task, run, tab)) = &location.task {
            if self.tasks.records.iter().any(|record| record.id == *task) {
                self.open_task_from_board(task.clone(), run.clone());
                self.tasks.views.entry(task.clone()).or_default().tab = *tab;
            }
        }
        match &location.page {
            Page::Chat => {}
            Page::Task { task, run, tab } => {
                self.open_task_from_board(task.clone(), run.clone());
                self.tasks.views.entry(task.clone()).or_default().tab = *tab;
            }
            Page::Board => self.open_agents_board(),
            Page::Scheduled => self.open_automation(),
            Page::Settings(page) => {
                self.settings_view.select(page);
                self.settings_open = true;
            }
            Page::Files | Page::Browser => {
                self.diff_rx = None;
                if let Some(target) = &location.file_target {
                    if let Err(error) = self.files.open_target(&self.ctx, &self.prefs.cwd, target) {
                        self.state.notice = Some(error);
                    }
                }
                if !self.files.is_open() {
                    if location.page == Page::Files {
                        self.files.toggle(&self.ctx, &self.prefs.cwd);
                    } else {
                        self.files.toggle_browser(&self.ctx, &self.prefs.cwd);
                    }
                }
            }
            Page::Changes => {
                self.inspector_panel = true;
                self.changes.invalidate();
                self.refresh_workspace();
            }
        }
        self.terminals
            .restore_navigation(&self.ctx, &self.prefs.cwd, self.demo, location.terminal);
        self.focus_composer = matches!(location.page, Page::Chat) && location.terminal.is_none();
        self.focus_search = false;
        self.ctx.request_repaint();
        true
    }

    /// Resolve Spotlight hits through the same owner-preserving navigation path.
    pub(super) fn open_search_candidate(&mut self, candidate: &search::Candidate) -> bool {
        if self.search_blocked() {
            return false;
        }
        self.remember_navigation();
        let mut location = self.navigation_location();
        location.page = Page::Chat;
        location.task = None;
        location.file_target = None;
        location.terminal = None;
        match &candidate.target {
            sidebar_tree::Target::Task(task, run) => {
                location.page = Page::Task {
                    task: task.clone(),
                    run: run.clone(),
                    tab: if run.is_some() {
                        tasks::work_item::Tab::Activity
                    } else {
                        tasks::work_item::Tab::Overview
                    },
                };
            }
            sidebar_tree::Target::Agent(id) => {
                // An unsaved current chat must keep its anonymous identity.
                if id != &self.agents.active || candidate.path != location.chat.path {
                    location.chat = Chat {
                        agent: id.clone(),
                        anonymous: None,
                        path: candidate.path.clone(),
                        cwd: candidate.cwd.clone(),
                        title: candidate.title.clone(),
                    };
                }
            }
            sidebar_tree::Target::Session(_) | sidebar_tree::Target::Saved(_) => {
                location.chat = Chat {
                    agent: candidate
                        .path
                        .as_deref()
                        .and_then(|path| self.agent_for_path(path))
                        .unwrap_or_default(),
                    anonymous: None,
                    path: candidate.path.clone(),
                    cwd: candidate.cwd.clone(),
                    title: candidate.title.clone(),
                };
            }
        }
        let restored = self.restore_location(&location);
        if restored {
            self.remember_navigation();
        }
        restored
    }

    pub(super) fn navigate_history(&mut self, forward: bool) {
        if self.navigation_blocked() {
            return;
        }
        self.remember_navigation();
        while self.navigation.can_go(forward) {
            let next = if forward {
                self.navigation.cursor + 1
            } else {
                self.navigation.cursor - 1
            };
            let location = self.navigation.entries[next].clone();
            if self.restore_location(&location) {
                self.navigation.cursor = next;
                // Normalize restored identities without creating a new forward branch.
                self.navigation.entries[next] = self.navigation_location();
                return;
            }
            // Deleted chats/tasks must not trap the history cursor or be resurrected.
            self.navigation.entries.remove(next);
            if !forward {
                self.navigation.cursor -= 1;
            }
        }
    }

    pub(super) fn navigation_inputs(&mut self, ctx: &egui::Context) {
        if self.navigation_blocked() || egui::Popup::is_any_open(ctx) {
            return;
        }
        let browser_input = !self.settings_open
            && !self.kanban.open
            && !self.automation.open
            && self.files.focused(ctx);
        let mut direction = None;
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                if let egui::Event::PointerButton {
                    button, pressed, ..
                } = event
                {
                    match button {
                        egui::PointerButton::Extra1 | egui::PointerButton::Extra2
                            if !browser_input =>
                        {
                            if *pressed {
                                direction = Some(*button == egui::PointerButton::Extra2);
                            }
                            return false;
                        }
                        _ => {}
                    }
                }
                true
            });
            // Native Chromium retains its own page history and keyboard navigation.
            if browser_input {
                return;
            }
            // Bracket shortcuts avoid stealing word/line movement from text fields and shells.
            let modifiers = if cfg!(target_os = "macos") {
                egui::Modifiers::MAC_CMD
            } else {
                egui::Modifiers::ALT
            };
            for (key, forward) in [
                (egui::Key::OpenBracket, false),
                (egui::Key::CloseBracket, true),
            ] {
                if input.consume_key(modifiers, key) {
                    direction = Some(forward);
                }
            }
        });
        if let Some(forward) = direction {
            self.navigate_history(forward);
        }
    }

    /// Shared by the workspace titlebar and full-window settings.
    pub(super) fn navigation_header(&mut self, ui: &mut egui::Ui, width: f32) {
        self.remember_navigation();
        let location = self.navigation_location();
        let colors = theme::palette(ui.ctx());
        let blocked = self.navigation_blocked();
        ui.allocate_ui_with_layout(
            Vec2::new(width, 36.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.spacing_mut().interact_size.y = 24.0;
                for (icon, forward, label) in [
                    (Icon::ArrowLeft, false, "Back · mouse back"),
                    (Icon::ArrowRight, true, "Forward · mouse forward"),
                ] {
                    let button = ui
                        .add_enabled_ui(!blocked && self.navigation.can_go(forward), |ui| {
                            icons::button(ui, icon, label)
                        })
                        .inner;
                    #[cfg(test)]
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(egui::Id::new(("view-history", forward)), button.rect)
                    });
                    if button.clicked() {
                        self.navigate_history(forward);
                    }
                }
                ui.add_space(5.0);
                if width >= 250.0 && (location.page != Page::Chat || location.terminal.is_some()) {
                    if ui
                        .add_enabled(
                            !blocked,
                            egui::Button::new(theme::small(ui, "Chat")).frame(false),
                        )
                        .on_hover_text(&location.chat.title)
                        .clicked()
                    {
                        let mut chat = location.clone();
                        chat.page = Page::Chat;
                        chat.terminal = None;
                        chat.task = None;
                        chat.file_target = None;
                        self.restore_location(&chat);
                    }
                    ui.label(RichText::new("/").size(12.0).color(colors.dim));
                }
                if let Page::Settings(page) = &location.page {
                    if ui
                        .add_enabled(
                            !blocked,
                            egui::Button::new(theme::small(ui, "Settings")).frame(false),
                        )
                        .clicked()
                    {
                        self.settings_view.select("General");
                    }
                    ui.label(RichText::new("/").size(12.0).color(colors.dim));
                    ui.add(
                        egui::Label::new(RichText::new(page.replace('/', " / ")).size(12.0))
                            .truncate(),
                    );
                } else {
                    let title = match &location.page {
                        Page::Chat if location.terminal.is_some() => "Terminal".to_owned(),
                        Page::Chat => location.chat.title.clone(),
                        Page::Board => "Agents board".into(),
                        Page::Scheduled => "Scheduled tasks".into(),
                        Page::Task { .. } => self
                            .tasks
                            .current()
                            .map(|t| t.objective.clone())
                            .filter(|t| !t.is_empty())
                            .unwrap_or_else(|| "Workflow".into()),
                        Page::Files => "Files".into(),
                        Page::Browser => "Browser".into(),
                        Page::Changes => "Changes".into(),
                        Page::Settings(_) => unreachable!(),
                    };
                    ui.add(
                        egui::Label::new(RichText::new(&title).size(13.0).color(colors.text))
                            .truncate(),
                    );
                }
                // Only blank titlebar space drags the window, never breadcrumb links.
                let (_, drag) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width().max(0.0), 30.0),
                    Sense::click_and_drag(),
                );
                if drag.drag_started() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if drag.double_clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(
                        !ui.input(|i| i.viewport().maximized.unwrap_or(false)),
                    ));
                }
            },
        );
    }

    pub(super) fn compact_view_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Views", |ui| {
            theme::menu_controls(ui);
            let mut selected = None;
            for (label, page) in [
                ("Chat", Page::Chat),
                ("Agents board", Page::Board),
                ("Scheduled tasks", Page::Scheduled),
                ("Files", Page::Files),
                ("Browser", Page::Browser),
                ("Changes", Page::Changes),
                ("Settings", Page::Settings("General".into())),
            ] {
                if ui.button(label).clicked() {
                    selected = Some(page);
                    ui.close();
                }
            }
            if let Some(page) = selected {
                let mut location = self.navigation_location();
                location.page = page;
                location.terminal = None;
                location.task = None;
                location.file_target = None;
                self.restore_location(&location);
            }
            if ui.button("Terminal").clicked() {
                self.settings_open = false;
                self.kanban.open = false;
                self.automation.open = false;
                self.terminals.toggle(&self.ctx, &self.prefs.cwd, self.demo);
                ui.close();
            }
            self.prefs.transcript_view.menu(ui);
            if ui.button("Pull request").clicked() {
                self.settings_open = false;
                self.kanban.open = false;
                self.automation.open = false;
                self.files.close();
                self.inspector_panel = false;
                self.background_tasks.open = false;
                self.background_tasks.expanded = false;
                self.pr_pane.open = !self.pr_pane.open;
                if !self.pr_pane.open {
                    self.pr_pane.expanded = false;
                }
                ui.close();
            }
            if ui.button("Background tasks").clicked() {
                self.settings_open = false;
                self.kanban.open = false;
                self.automation.open = false;
                self.files.close();
                self.inspector_panel = false;
                self.pr_pane.close();
                self.background_tasks.open = !self.background_tasks.open;
                ui.close();
            }
            if ui.button("Session details").clicked() {
                self.files.close();
                self.inspector_panel = false;
                self.background_tasks.open = false;
                self.pr_pane.close();
                self.prefs.hide_inspector = !self.prefs.hide_inspector;
                ui.close();
            }
        });
    }

    pub(super) fn settings_navigation_header(&mut self, ctx: &egui::Context) {
        let colors = theme::palette(ctx);
        egui::TopBottomPanel::top("settings-breadcrumbs")
            .exact_height(64.0)
            .frame(
                egui::Frame::new()
                    .fill(colors.settings_bg)
                    .inner_margin(egui::Margin {
                        left: 20,
                        right: 20,
                        top: 24,
                        bottom: 0,
                    }),
            )
            .show(ctx, |ui| {
                self.navigation_header(ui, ui.available_width());
            });
    }
}
