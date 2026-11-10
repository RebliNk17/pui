use super::*;
use std::collections::BTreeMap;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct ChatPrefs {
    pub(super) title: Option<String>,
    pub(super) project: Option<PathBuf>,
    pub(super) pinned: bool,
    pub(super) archived: bool,
    pub(super) hidden: bool,
}
#[derive(Clone)]
enum Action {
    Rename(PathBuf),
    Move(PathBuf),
    Export(PathBuf),
    Hide(PathBuf),
    Delete(deletion::Target),
    Open(usize),
    Agent(String),
    NewChat(PathBuf),
    NewTask,
    Task(String, Option<String>),
    Saved(PathBuf, PathBuf),
}

impl Pui {
    pub(super) fn sidebar(&mut self, ctx: &egui::Context) {
        let colors = crate::theme::palette(ctx);
        let mut action = None;
        let tasks_loading = self.tasks.loading_history();
        let branches = sidebar_tree::forest(self);
        egui::SidePanel::left("sessions-v4")
            .exact_width(254.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(colors.sidebar).inner_margin(12))
            .show(ctx, |ui| {
                // Native traffic lights occupy the top-left strip on macOS.
                // Keep app content below them instead of offsetting only the name.
                #[cfg(target_os = "macos")]
                ui.add_space((32.0 / ctx.zoom_factor() - 12.0).max(12.0));
                ui.horizontal(|ui| {
                    ui.set_height(theme::CONTROL_HEIGHT);
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if icons::button(ui, Icon::Sidebar, "Hide sessions · ⌘B").clicked() {
                        self.prefs.hide_sidebar = true;
                    }
                    self.app_menu(ui);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icons::button(ui, Icon::Plus, "New chat · ⌘N").clicked() {
                            action = Some(Action::NewChat(self.prefs.cwd.clone()));
                        }
                        if ui.button("New task").clicked() {
                            action = Some(Action::NewTask);
                        }
                    });
                });
                ui.add_space(10.0);
                egui::Frame::new()
                    .fill(colors.bg)
                    .stroke(Stroke::new(1.0, colors.border))
                    .corner_radius(7)
                    .inner_margin(7)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            icons::inline(ui, Icon::Search, colors.dim);
                            let search = ui.add(
                                egui::TextEdit::singleline(&mut self.search)
                                    .id_salt("session-search")
                                    .hint_text("Search sessions")
                                    .font(FontId::proportional(12.0))
                                    .frame(false)
                                    .desired_width((ui.available_width() - 34.0).max(20.0)),
                            );
                            if self.focus_search {
                                search.request_focus();
                                self.focus_search = false;
                            }
                            ui.label(
                                RichText::new(if cfg!(target_os = "macos") {
                                    "⌘K"
                                } else {
                                    "^K"
                                })
                                .size(10.0)
                                .color(colors.dim),
                            );
                        });
                    });
                ui.add_space(10.0);
                egui::Frame::new()
                    .fill(colors.bg)
                    .corner_radius(8)
                    .inner_margin(3)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        let width = (ui.available_width() - 3.0) / 2.0;
                        ui.horizontal(|ui| {
                            for (index, label) in ["Recent", "Projects"].iter().enumerate() {
                                let selected = self.projects == (index == 1);
                                if ui
                                    .add_sized(
                                        [width, theme::CONTROL_HEIGHT],
                                        egui::Button::new(RichText::new(*label).size(11.5).color(
                                            if selected { colors.text } else { colors.muted },
                                        ))
                                        .fill(if selected {
                                            colors.hover
                                        } else {
                                            Color32::TRANSPARENT
                                        })
                                        .stroke(Stroke::NONE)
                                        .corner_radius(6),
                                    )
                                    .clicked()
                                {
                                    self.projects = index == 1;
                                }
                            }
                        });
                    });
                ui.add_space(10.0);
                // Reserve exactly the footer, not a 95px estimate followed by extra layout padding.
                egui::TopBottomPanel::bottom("account-footer-v2")
                    .exact_height(55.0)
                    .show_separator_line(false)
                    .frame(egui::Frame::new().fill(colors.sidebar))
                    .show_inside(ui, |ui| {
                        ui.add(egui::Separator::default().spacing(1.0));
                        self.usage.footer(ui, self.demo);
                    });
                ui.horizontal(|ui| {
                    ui.label(theme::caption(
                        ui,
                        if self.show_archived {
                            "ARCHIVED"
                        } else if self.projects {
                            "PROJECTS"
                        } else if self.demo {
                            "PREVIEW SESSIONS"
                        } else {
                            "RECENT SESSIONS"
                        },
                    ));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icons::button(ui, Icon::Refresh, "Refresh sessions").clicked() {
                            self.refresh_sessions();
                        }
                    });
                });
                let query = self.search.trim().to_lowercase();
                let visible: Vec<_> = branches
                    .iter()
                    .enumerate()
                    .filter(|(_, branch)| {
                        !(branch.hidden
                            || tasks_loading
                                && matches!(branch.target, sidebar_tree::Target::Session(_)))
                            && branch.archived == self.show_archived
                            && branch.matches(&query)
                    })
                    .map(|(index, _)| index)
                    .collect();
                let mut groups: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
                if self.projects {
                    for index in &visible {
                        groups
                            .entry(branches[*index].project.clone())
                            .or_default()
                            .push(*index);
                    }
                }
                egui::ScrollArea::vertical()
                    .id_salt(("session-list-v4", self.projects, self.show_archived))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        if self.prefs.chats.values().any(|m| m.hidden)
                            && ui.button("Restore hidden chats").clicked()
                        {
                            for meta in self.prefs.chats.values_mut() {
                                meta.hidden = false;
                            }
                        }
                        if visible.is_empty() {
                            ui.label(theme::small(
                                ui,
                                if self.session_rx.is_some() {
                                    "Loading sessions…"
                                } else if tasks_loading {
                                    "Loading workflows…"
                                } else {
                                    "No sessions found."
                                },
                            ));
                        }
                        if self.projects {
                            let mut groups: Vec<_> = groups.into_iter().collect();
                            groups.sort_by_key(|(_, indices)| indices[0]);
                            for (project, mut indices) in groups {
                                indices.sort_by_key(|i| std::cmp::Reverse(branches[*i].modified));
                                let open = !self.closed_projects.contains(&project);
                                let (_, rect) = ui.allocate_space(Vec2::new(
                                    ui.available_width(),
                                    theme::CONTROL_HEIGHT,
                                ));
                                let plus_rect = egui::Rect::from_center_size(
                                    rect.right_center() - egui::vec2(16.0, 0.0),
                                    Vec2::splat(theme::CONTROL_HEIGHT),
                                );
                                let header_rect = egui::Rect::from_min_max(
                                    rect.min,
                                    egui::pos2(plus_rect.left() - 4.0, rect.bottom()),
                                );
                                let response = ui.interact(
                                    header_rect,
                                    ui.make_persistent_id(("project-heading", &project)),
                                    Sense::click(),
                                );
                                let plus = ui
                                    .interact(
                                        plus_rect,
                                        ui.make_persistent_id(("new-project-chat", &project)),
                                        Sense::click(),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text("New chat in this project");
                                plus.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        true,
                                        format!("New chat in {}", sessions::project_name(&project)),
                                    )
                                });
                                if plus.hovered() {
                                    ui.painter().rect_filled(plus_rect, 6, colors.hover);
                                }
                                icons::draw(
                                    ui,
                                    plus_rect,
                                    Icon::Plus,
                                    if plus.hovered() {
                                        colors.text
                                    } else {
                                        colors.dim
                                    },
                                );
                                if plus.clicked() {
                                    action = Some(Action::NewChat(project.clone()));
                                }
                                let response = response
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text(project.display().to_string());
                                icons::chevron(
                                    ui,
                                    egui::Rect::from_center_size(
                                        rect.left_center() + egui::vec2(6.0, 0.0),
                                        Vec2::splat(12.0),
                                    ),
                                    open,
                                    colors.dim,
                                );
                                icons::draw(
                                    ui,
                                    egui::Rect::from_center_size(
                                        rect.left_center() + egui::vec2(25.0, 0.0),
                                        Vec2::splat(14.0),
                                    ),
                                    Icon::Folder,
                                    colors.muted,
                                );
                                paint_text(
                                    ui,
                                    &sessions::project_name(&project),
                                    rect.left_center() + egui::vec2(40.0, 0.0),
                                    rect.width() - 102.0,
                                    FontId::proportional(12.0),
                                    colors.text,
                                );
                                ui.painter().text(
                                    rect.right_center() - egui::vec2(34.0, 0.0),
                                    egui::Align2::RIGHT_CENTER,
                                    indices.len().to_string(),
                                    FontId::proportional(10.0),
                                    colors.dim,
                                );
                                if response.clicked() {
                                    if open {
                                        self.closed_projects.insert(project.clone());
                                    } else {
                                        self.closed_projects.remove(&project);
                                    }
                                }
                                if open {
                                    let expanded = self.expanded_projects.contains(&project);
                                    let count = indices.len();
                                    for index in
                                        indices.into_iter().take(if expanded { count } else { 5 })
                                    {
                                        if let Some(next) =
                                            self.branch_row(ui, &branches[index], 0, &query)
                                        {
                                            action = Some(next);
                                        }
                                    }
                                    if count > 5
                                        && ui
                                            .add(
                                                egui::Button::new(theme::small(
                                                    ui,
                                                    if expanded {
                                                        "Show less".into()
                                                    } else {
                                                        format!("Show {} more", count - 5)
                                                    },
                                                ))
                                                .small()
                                                .frame(false),
                                            )
                                            .clicked()
                                    {
                                        if expanded {
                                            self.expanded_projects.remove(&project);
                                        } else {
                                            self.expanded_projects.insert(project.clone());
                                        }
                                    }
                                }
                                ui.add_space(6.0);
                            }
                        } else {
                            let mut indices = visible;
                            indices.sort_by_key(|i| !branches[*i].pinned);
                            for index in indices {
                                if let Some(next) = self.branch_row(ui, &branches[index], 0, &query)
                                {
                                    action = Some(next);
                                }
                            }
                        }
                        if (self.show_archived || self.prefs.chats.values().any(|m| m.archived))
                            && ui
                                .add(
                                    egui::Button::new(theme::small(
                                        ui,
                                        if self.show_archived {
                                            "‹ Back to sessions"
                                        } else {
                                            "Archived"
                                        },
                                    ))
                                    .frame(false),
                                )
                                .clicked()
                        {
                            self.show_archived = !self.show_archived;
                        }
                    });
            });
        if let Some(action) = action {
            match action {
                Action::Agent(id) => self.view_agent(&id),
                Action::NewChat(folder) => self.start_chat_in(folder),
                Action::NewTask => self.new_task(),
                Action::Delete(target) => self.request_delete(target),
                Action::Task(task, run) => {
                    self.tasks.open = true;
                    self.tasks.selected = Some(task);
                    self.tasks.selected_run = run;
                    self.tasks.transcript = Default::default();
                    self.settings_open = false;
                    self.files.close();
                    self.inspector_panel = false;
                }
                Action::Saved(path, cwd) => {
                    self.open_chat(Session {
                        path,
                        cwd: cwd.clone(),
                        project: cwd,
                        title: "Parent chat".into(),
                        modified: std::time::SystemTime::UNIX_EPOCH,
                    });
                }
                Action::Open(index) if !self.demo => {
                    self.open_chat(self.sessions[index].clone());
                }
                Action::Rename(path) => self.begin_rename(path),
                Action::Move(path) => {
                    if let Some(folder) = rfd::FileDialog::new()
                        .set_title("Organize under project (does not change working directory)")
                        .pick_folder()
                    {
                        self.prefs.chats.entry(path).or_default().project = Some(folder);
                    }
                }
                Action::Export(path) => {
                    if let Some(dest) = rfd::FileDialog::new()
                        .set_file_name("session.md")
                        .save_file()
                    {
                        match sessions::export_markdown(&path, &dest) {
                            Ok(()) => {
                                self.state.notice = Some("Exported session as Markdown.".into())
                            }
                            Err(error) => self.state.error = Some(error),
                        }
                    }
                }
                Action::Hide(path) => {
                    self.prefs.chats.entry(path).or_default().hidden = true;
                }
                _ => {}
            }
        }
    }
    fn begin_rename(&mut self, path: PathBuf) {
        self.rename_text = self
            .prefs
            .chats
            .get(&path)
            .and_then(|m| m.title.clone())
            .or_else(|| {
                self.sessions
                    .iter()
                    .find(|s| s.path == path)
                    .map(|s| s.title.clone())
            })
            .unwrap_or_default();
        let id = egui::Id::new(("chat-rename", &path));
        let mut state = egui::text_edit::TextEditState::default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(self.rename_text.chars().count()),
            )));
        state.store(&self.ctx, id);
        self.ctx.memory_mut(|m| m.request_focus(id));
        self.focus_composer = false;
        self.rename_chat = Some(path);
        self.ctx.request_repaint();
    }
    fn branch_row(
        &mut self,
        ui: &mut egui::Ui,
        branch: &sidebar_tree::Branch,
        depth: usize,
        query: &str,
    ) -> Option<Action> {
        if branch.hidden {
            return None;
        }
        let colors = theme::palette(ui.ctx());
        let selected = branch.active(self);
        let expanded = !self.prefs.closed_threads.contains(&branch.key) || !query.is_empty();
        let has_children = branch.children.iter().any(|c| !c.hidden);
        let compact = depth > 0;
        let row_height = if compact {
            28.0
        } else if matches!(branch.target, sidebar_tree::Target::Session(_)) {
            48.0
        } else {
            44.0
        };
        let indent = (depth as f32 * 14.0).min(70.0);
        let mut action = None;
        ui.push_id((&branch.key, "tree-row"), |ui| {
            let origin = ui.cursor().min;
            let (_, rect) = ui.allocate_space(Vec2::new(ui.available_width(), 0.0));
            let left = origin.x + indent;
            let center_y = origin.y + if compact { row_height / 2.0 } else { 16.0 };
            let toggle_rect =
                egui::Rect::from_center_size(egui::pos2(left + 9.0, center_y), Vec2::splat(20.0));
            if compact {
                let x = left - 7.0;
                ui.painter().line_segment(
                    [egui::pos2(x, origin.y), egui::pos2(x, center_y)],
                    Stroke::new(1.0, colors.border),
                );
                ui.painter().line_segment(
                    [egui::pos2(x, center_y), egui::pos2(left + 1.0, center_y)],
                    Stroke::new(1.0, colors.border),
                );
            }
            let toggle = has_children
                .then(|| ui.interact(toggle_rect, ui.id().with("disclosure"), Sense::click()));
            let child_left = left + 20.0;
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                    egui::pos2(child_left, origin.y),
                    egui::pos2(rect.right(), ui.max_rect().bottom()),
                )),
                |ui| match &branch.target {
                    sidebar_tree::Target::Session(index) if !compact => {
                        action = self.chat_row(ui, *index)
                    }
                    target => {
                        let (row, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), row_height),
                            Sense::click(),
                        );
                        if selected || response.hovered() {
                            ui.painter().rect_filled(
                                row,
                                7,
                                if selected {
                                    colors.hover
                                } else {
                                    colors.surface
                                },
                            );
                        }
                        let tint =
                            if branch.status.contains("Working") || branch.status == "Connecting" {
                                colors.accent
                            } else if branch.status.contains("Blocked")
                                || branch.status.contains("Failed")
                            {
                                colors.red
                            } else if branch.status == "Idle" || branch.status == "Finished" {
                                colors.green
                            } else {
                                colors.dim
                            };
                        let title_y = if compact { row.height() / 2.0 } else { 13.0 };
                        theme::status_dot(ui, row.min + egui::vec2(8.0, title_y), tint, 2.5);
                        paint_text(
                            ui,
                            &branch.title,
                            row.min + egui::vec2(19.0, title_y),
                            row.width() - 48.0,
                            FontId::proportional(if compact { 11.0 } else { 11.5 }),
                            if compact && !selected && !response.hovered() {
                                colors.muted
                            } else {
                                colors.text
                            },
                        );
                        if !compact {
                            paint_text(
                                ui,
                                &branch.status,
                                row.min + egui::vec2(19.0, 30.0),
                                row.width() - 24.0,
                                FontId::proportional(10.0),
                                colors.dim,
                            );
                        }
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::SelectableLabel,
                                true,
                                selected,
                                format!("{} · {}", branch.title, branch.status),
                            )
                        });
                        let delete_target = match target {
                            sidebar_tree::Target::Task(id, None) => {
                                Some(deletion::Target::Task(id.clone()))
                            }
                            sidebar_tree::Target::Task(_, Some(_)) => None,
                            _ => branch.path.clone().map(deletion::Target::Chat),
                        };
                        let menu_rect = egui::Rect::from_center_size(
                            row.right_center() - egui::vec2(12.0, 0.0),
                            Vec2::splat(24.0),
                        );
                        let menu =
                            ui.interact(menu_rect, ui.id().with("delete-menu"), Sense::click());
                        if delete_target.is_some()
                            && (selected || response.hovered() || menu.hovered())
                        {
                            for dx in [-4.0, 0.0, 4.0] {
                                ui.painter().circle_filled(
                                    menu_rect.center() + egui::vec2(dx, 0.0),
                                    1.0,
                                    colors.muted,
                                );
                            }
                        }
                        if let Some(target) = delete_target {
                            let mut contents = |ui: &mut egui::Ui| {
                                let label = if matches!(target, deletion::Target::Task(_)) {
                                    "Delete task…"
                                } else {
                                    "Delete chat…"
                                };
                                if ui.button(RichText::new(label).color(colors.red)).clicked() {
                                    action = Some(Action::Delete(target.clone()));
                                    ui.close();
                                }
                            };
                            egui::Popup::menu(&menu).show(&mut contents);
                            response.context_menu(&mut contents);
                        }
                        let keyboard = response.has_focus()
                            && ui.input_mut(|i| {
                                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                            });
                        if (response
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(format!("{}\n{}", branch.title, branch.status))
                            .clicked()
                            && !menu_rect.contains(
                                ui.input(|i| i.pointer.interact_pos()).unwrap_or_default(),
                            ))
                            || keyboard
                        {
                            action = Some(match target {
                                sidebar_tree::Target::Agent(id) => Action::Agent(id.clone()),
                                sidebar_tree::Target::Task(task, run) => {
                                    Action::Task(task.clone(), run.clone())
                                }
                                sidebar_tree::Target::Saved(path) => {
                                    Action::Saved(path.clone(), branch.cwd.clone())
                                }
                                sidebar_tree::Target::Session(index) => Action::Open(*index),
                            });
                        }
                    }
                },
            );
            ui.advance_cursor_after_rect(egui::Rect::from_min_size(
                origin,
                Vec2::new(rect.width(), row_height),
            ));
            if let Some(toggle) = toggle {
                if toggle.clicked() {
                    toggle.request_focus();
                }
                let highlighted = toggle.hovered() || toggle.has_focus();
                if highlighted {
                    ui.painter().rect_filled(toggle_rect, 5, colors.hover);
                }
                let center = toggle_rect.center();
                let points = if expanded {
                    [
                        center + egui::vec2(-3.5, -1.5),
                        center + egui::vec2(0.0, 2.0),
                        center + egui::vec2(3.5, -1.5),
                    ]
                } else {
                    [
                        center + egui::vec2(-1.5, -3.5),
                        center + egui::vec2(2.0, 0.0),
                        center + egui::vec2(-1.5, 3.5),
                    ]
                };
                ui.painter().add(egui::Shape::line(
                    points.to_vec(),
                    Stroke::new(
                        1.4,
                        if highlighted {
                            colors.text
                        } else {
                            colors.muted
                        },
                    ),
                ));
                let children = branch.children.iter().filter(|child| !child.hidden).count();
                toggle.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::CollapsingHeader,
                        true,
                        expanded,
                        format!("{} children · {children}", branch.title),
                    )
                });
                let keyboard = toggle.has_focus()
                    && ui.input_mut(|i| {
                        i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                            || i.consume_key(egui::Modifiers::NONE, egui::Key::Space)
                            || i.consume_key(
                                egui::Modifiers::NONE,
                                if expanded {
                                    egui::Key::ArrowLeft
                                } else {
                                    egui::Key::ArrowRight
                                },
                            )
                    });
                if toggle
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(format!(
                        "{} {children} children",
                        if expanded { "Hide" } else { "Show" }
                    ))
                    .clicked()
                    || keyboard
                {
                    if expanded {
                        self.prefs.closed_threads.insert(branch.key.clone());
                    } else {
                        self.prefs.closed_threads.remove(&branch.key);
                    }
                }
            }
        });
        if expanded {
            for child in &branch.children {
                if child.matches(query) {
                    if let Some(next) = self.branch_row(ui, child, depth + 1, query) {
                        action = Some(next);
                    }
                }
            }
        }
        action
    }
    fn chat_row(&mut self, ui: &mut egui::Ui, index: usize) -> Option<Action> {
        let colors = crate::theme::palette(ui.ctx());
        let s = &self.sessions[index];
        let path = s.path.clone();
        let selected = !self.tasks.open
            && (self
                .state
                .session_file
                .as_deref()
                .is_some_and(|p| std::path::Path::new(p) == path)
                || self.demo && index == 0);
        let node = self
            .agents
            .nodes
            .values()
            .find(|n| n.path.as_ref() == Some(&path));
        let shared = self.other_window_status(&path).map(str::to_owned);
        let (status, status_color) = if selected {
            self.run_label()
        } else if let Some(node) = node {
            (node.status.as_str(), node.color(colors))
        } else if let Some(status) = shared.as_deref() {
            (
                status,
                if status == "Idle" {
                    colors.green
                } else {
                    colors.accent
                },
            )
        } else {
            ("Saved · status unknown", colors.dim)
        };
        let meta = self.prefs.chats.entry(path.clone()).or_default();
        let title = meta.title.as_deref().unwrap_or(&s.title);
        let editing = self.rename_chat.as_ref() == Some(&path);
        let (_, rect) = ui.allocate_space(Vec2::new(ui.available_width(), 48.0));
        // Session identity, not layout position: filtering, pinning, regrouping or
        // asynchronous refresh must not move a pointer press to another chat.
        let response = ui.interact(
            rect,
            ui.make_persistent_id(("chat-row", &path)),
            if editing {
                Sense::hover()
            } else {
                Sense::click()
            },
        );
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let response = if self.loading_chat && !selected {
            response.on_hover_text("Open this chat instead")
        } else {
            response
        };
        if selected || response.hovered() {
            ui.painter().rect_filled(
                rect,
                8,
                if selected {
                    colors.hover
                } else {
                    colors.surface
                },
            );
        }
        theme::status_dot(ui, rect.min + egui::vec2(10.0, 16.0), status_color, 3.0);
        if editing {
            let id = egui::Id::new(("chat-rename", &path));
            let focused = ui.memory(|m| m.has_focus(id));
            // Consume before TextEdit handles Enter/Escape, and before the composer renders.
            let cancel = focused
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            let save =
                focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            let edit = ui.put(
                egui::Rect::from_min_size(
                    rect.min + egui::vec2(22.0, 3.0),
                    egui::vec2((rect.width() - 52.0).max(20.0), 24.0),
                ),
                egui::TextEdit::singleline(&mut self.rename_text)
                    .id(id)
                    .font(FontId::proportional(12.0))
                    .margin(egui::vec2(2.0, 3.0)),
            );
            if cancel {
                edit.surrender_focus();
                self.rename_chat = None;
            } else if save {
                if self.rename_text.trim().is_empty() {
                    edit.request_focus();
                } else {
                    meta.title = Some(self.rename_text.trim().into());
                    edit.surrender_focus();
                    self.rename_chat = None;
                }
            } else if edit.lost_focus() {
                // Only Enter saves; clicking elsewhere leaves the original title intact.
                self.rename_chat = None;
            }
        } else {
            paint_text(
                ui,
                &format!("{}{}", if meta.pinned { "• " } else { "" }, title),
                rect.min + egui::vec2(24.0, 15.0),
                rect.width() - 54.0,
                FontId::proportional(12.0),
                colors.text,
            );
        }
        paint_text(
            ui,
            &format!(
                "{} · {}",
                sessions::project_name(&s.project),
                if selected || node.is_some() || shared.is_some() {
                    status.to_owned()
                } else {
                    format!("Saved · {}", sessions::age(s.modified))
                }
            ),
            rect.min + egui::vec2(24.0, 33.0),
            rect.width() - 36.0,
            FontId::proportional(10.5),
            colors.dim,
        );
        let menu_rect = egui::Rect::from_center_size(
            rect.right_center() - egui::vec2(12.0, 6.0),
            Vec2::splat(24.0),
        );
        let menu = ui
            .interact(
                menu_rect,
                ui.make_persistent_id(("chat-menu", &path)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if selected || response.hovered() || menu.hovered() {
            for dx in [-4.0, 0.0, 4.0] {
                ui.painter().circle_filled(
                    menu_rect.center() + egui::vec2(dx, 0.0),
                    1.0,
                    colors.muted,
                );
            }
        }
        let mut action = None;
        let mut menu_contents = |ui: &mut egui::Ui| {
            ui.set_min_width(200.0);
            if ui.button("Rename").clicked() {
                action = Some(Action::Rename(path.clone()));
                ui.close();
            }
            if ui
                .button(if meta.pinned { "Unpin" } else { "Pin" })
                .clicked()
            {
                meta.pinned = !meta.pinned;
                ui.close();
            }
            if ui
                .button("Move to project…")
                .on_hover_text("Organize in Pui; does not change Pi's working directory")
                .clicked()
            {
                action = Some(Action::Move(path.clone()));
                ui.close();
            }
            if ui.button("Copy session path").clicked() {
                ui.ctx().copy_text(path.display().to_string());
                ui.close();
            }
            if ui.button("Export as Markdown").clicked() {
                action = Some(Action::Export(path.clone()));
                ui.close();
            }
            ui.separator();
            if ui
                .button(if meta.archived {
                    "Unarchive"
                } else {
                    "Archive"
                })
                .clicked()
            {
                meta.archived = !meta.archived;
                ui.close();
            }
            if ui
                .button(egui::RichText::new("Hide from Pui").color(colors.red))
                .on_hover_text("Does not delete your Pi session file")
                .clicked()
            {
                action = Some(Action::Hide(path.clone()));
                ui.close();
            }
            ui.separator();
            if ui
                .button(RichText::new("Delete chat…").color(colors.red))
                .clicked()
            {
                action = Some(Action::Delete(deletion::Target::Chat(path.clone())));
                ui.close();
            }
        };
        egui::Popup::menu(&menu).show(&mut menu_contents);
        response.context_menu(&mut menu_contents);
        if response.clicked()
            && !menu_rect.contains(ui.input(|i| i.pointer.interact_pos()).unwrap_or_default())
        {
            action = Some(Action::Open(index));
        }
        action
    }
}
fn paint_text(
    ui: &egui::Ui,
    text: &str,
    pos: egui::Pos2,
    width: f32,
    font: FontId,
    color: Color32,
) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.into(), font, color);
    job.wrap.max_width = width.max(20.0);
    job.wrap.max_rows = 1;
    let galley = ui.painter().layout_job(job);
    ui.painter()
        .galley(pos - egui::vec2(0.0, galley.size().y / 2.0), galley, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app(ctx: &egui::Context) -> Pui {
        Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        )
    }
    fn frame(ctx: &egui::Context, app: &mut Pui, events: Vec<egui::Event>) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| app.sidebar(ctx),
        )
    }
    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }
    #[test]
    fn inline_rename_selects_title_and_enter_saves_trimmed_text_in_both_views() {
        for projects in [false, true] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.projects = projects;
            app.sessions.truncate(1);
            let path = app.sessions[0].path.clone();
            app.prefs.chats.entry(path.clone()).or_default().title = Some("Custom title".into());
            for _ in 0..2 {
                frame(&ctx, &mut app, vec![]);
            }
            app.begin_rename(path.clone());
            frame(&ctx, &mut app, vec![]);
            let id = egui::Id::new(("chat-rename", &path));
            assert!(ctx.memory(|m| m.has_focus(id)));
            let state = egui::text_edit::TextEditState::load(&ctx, id).unwrap();
            assert_eq!(
                state.cursor.char_range().unwrap().sorted_cursors()[1].index,
                "Custom title".len()
            );
            frame(
                &ctx,
                &mut app,
                vec![egui::Event::Text("  New title  ".into())],
            );
            assert_eq!(app.rename_text, "  New title  ");
            assert_eq!(
                app.prefs.chats[&path].title.as_deref(),
                Some("Custom title")
            );
            frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
            assert!(app.rename_chat.is_none());
            assert_eq!(app.prefs.chats[&path].title.as_deref(), Some("New title"));
            assert!(!ctx.memory(|m| m.has_focus(id)));
        }
    }
    #[test]
    fn rename_menu_opens_focused_inline_editor_without_modal() {
        fn label(shape: &egui::Shape, text: &str) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Text(t) if t.galley.job.text == text => {
                    Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
                }
                egui::Shape::Vec(v) => v.iter().find_map(|s| label(s, text)),
                _ => None,
            }
        }
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.sessions.truncate(1);
        let path = app.sessions[0].path.clone();
        for _ in 0..2 {
            frame(&ctx, &mut app, vec![]);
        }
        let output = frame(&ctx, &mut app, vec![]);
        let title = output
            .shapes
            .iter()
            .find_map(|s| label(&s.shape, &app.sessions[0].title))
            .unwrap();
        let pos = egui::pos2(230.0, title.center().y + 3.0);
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        let output = frame(&ctx, &mut app, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|s| label(&s.shape, "Rename"))
            .expect("Rename menu item")
            .center();
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        let output = frame(&ctx, &mut app, vec![]);
        assert_eq!(app.rename_chat.as_ref(), Some(&path));
        assert!(ctx.memory(|m| m.has_focus(egui::Id::new(("chat-rename", &path)))));
        assert!(
            !output
                .shapes
                .iter()
                .any(|s| label(&s.shape, "Rename chat").is_some())
        );
        frame(&ctx, &mut app, vec![egui::Event::Text("From menu".into())]);
        frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
        assert_eq!(app.prefs.chats[&path].title.as_deref(), Some("From menu"));
    }
    #[test]
    fn inline_rename_escape_and_focus_loss_discard_changes() {
        for escape in [true, false] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.sessions.truncate(1);
            let path = app.sessions[0].path.clone();
            let title = app.sessions[0].title.clone();
            for _ in 0..2 {
                frame(&ctx, &mut app, vec![]);
            }
            app.begin_rename(path.clone());
            frame(&ctx, &mut app, vec![]);
            frame(&ctx, &mut app, vec![egui::Event::Text("Discard me".into())]);
            if escape {
                frame(&ctx, &mut app, vec![key(egui::Key::Escape)]);
            } else {
                let pos = egui::pos2(100.0, 115.0);
                frame(
                    &ctx,
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
                frame(&ctx, &mut app, vec![]);
            }
            assert!(app.rename_chat.is_none());
            assert!(app.prefs.chats[&path].title.is_none());
            assert_eq!(app.sessions[0].title, title);
        }
    }
    #[test]
    fn inline_rename_rejects_blank_title_and_follows_path_after_reorder() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.sessions.truncate(2);
        let path = app.sessions[0].path.clone();
        for _ in 0..2 {
            frame(&ctx, &mut app, vec![]);
        }
        app.begin_rename(path.clone());
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![egui::Event::Text("   ".into())]);
        frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
        assert_eq!(app.rename_chat.as_ref(), Some(&path));
        assert!(app.prefs.chats[&path].title.is_none());
        app.sessions.swap(0, 1);
        app.rename_text = "Still the same chat".into();
        frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
        assert_eq!(
            app.prefs.chats[&path].title.as_deref(),
            Some("Still the same chat")
        );
        assert!(app.prefs.chats[&app.sessions[0].path].title.is_none());
    }
    #[test]
    fn inline_rename_escape_does_not_stop_run_and_enter_does_not_send_draft() {
        for key_code in [egui::Key::Escape, egui::Key::Enter] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.sessions.truncate(1);
            app.state.busy = true;
            app.draft = "Do not send this".into();
            app.client = Some(rpc::Client::stub());
            let mut window = eframe::Frame::_new_kittest();
            let mut full_frame = |app: &mut Pui, events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1500.0, 950.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(app, ctx, &mut window),
                )
            };
            full_frame(&mut app, vec![]);
            full_frame(&mut app, vec![]);
            app.begin_rename(app.sessions[0].path.clone());
            full_frame(&mut app, vec![]);
            app.rename_text = "Updated title".into();
            let before = app
                .client
                .as_mut()
                .unwrap()
                .enqueue(protocol::command("get_state"))
                .unwrap();
            full_frame(&mut app, vec![key(key_code)]);
            assert!(app.rename_chat.is_none());
            assert_eq!(app.draft, "Do not send this");
            assert!(app.state.busy);
            let after = app
                .client
                .as_mut()
                .unwrap()
                .enqueue(protocol::command("get_state"))
                .unwrap();
            let sequence = |id: &str| id.strip_prefix("pui-").unwrap().parse::<u64>().unwrap();
            assert_eq!(
                sequence(&after),
                sequence(&before) + 1,
                "Rename keys must not send any RPC commands"
            );
        }
    }
    #[test]
    fn live_prompt_and_pi_names_replace_stale_discovery_titles_without_changing_local_rename() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.sessions.truncate(1);
        let path = app.sessions[0].path.clone();
        app.sessions[0].title = "Untitled session".into();
        app.state.session_file = Some(path.display().to_string());
        app.state.session_name = "Untitled session".into();
        app.state.messages = vec![Message {
            role: "user".into(),
            text: "Fix\n the sidebar".into(),
            ..Default::default()
        }];
        app.prefs.chats.entry(path.clone()).or_default().title = Some("My local name".into());
        app.update_session_title();
        assert_eq!(app.state.session_name, "Fix the sidebar");
        assert_eq!(app.sessions[0].title, "Fix the sidebar");
        app.state.session_name = "Pi named this chat".into();
        app.update_session_title();
        assert_eq!(app.sessions[0].title, "Pi named this chat");
        assert_eq!(
            app.prefs.chats[&path].title.as_deref(),
            Some("My local name")
        );
    }
    #[test]
    fn header_has_aligned_controls_below_native_traffic_light_clearance() {
        fn text(shape: &egui::Shape, labels: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(t) => labels.push((
                    t.galley.job.text.clone(),
                    egui::Rect::from_min_size(t.pos, t.galley.size()),
                )),
                egui::Shape::Vec(v) => {
                    for s in v {
                        text(s, labels)
                    }
                }
                _ => {}
            }
        }
        for zoom in [0.8, 1.0, 1.4] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            ctx.set_zoom_factor(zoom);
            for _ in 0..3 {
                frame(&ctx, &mut app, vec![]);
            }
            let output = frame(&ctx, &mut app, vec![]);
            let mut labels = Vec::new();
            for shape in output.shapes {
                text(&shape.shape, &mut labels);
            }
            let title = labels.iter().find(|(s, _)| s == "Pui").unwrap().1;
            let search = labels
                .iter()
                .find(|(s, _)| s == "Search sessions")
                .unwrap()
                .1;
            let recent = labels.iter().find(|(s, _)| s == "Recent").unwrap().1;
            let projects = labels.iter().find(|(s, _)| s == "Projects").unwrap().1;
            assert!(
                title.left() < 70.0,
                "Title should align with sidebar controls, not be displaced by traffic lights"
            );
            if cfg!(target_os = "macos") {
                assert!(
                    title.top() * zoom >= 30.0,
                    "Native traffic-light overlap at scale {zoom}"
                );
            }
            assert!(title.bottom() < search.top());
            assert!(search.bottom() < recent.top());
            assert!(
                (recent.center().y - projects.center().y).abs() <= 1.0 / zoom,
                "Segment labels must share a baseline within pixel rounding at scale {zoom}: {recent:?} / {projects:?}"
            );
        }
    }
    #[test]
    fn project_plus_opens_blank_chat_without_finder_or_stopping_current_work() {
        fn project_text(shape: &egui::Shape, name: &str) -> Option<egui::Pos2> {
            match shape {
                egui::Shape::Text(t) if t.galley.job.text == name => {
                    Some(t.pos + egui::vec2(0.0, t.galley.size().y / 2.0))
                }
                egui::Shape::Vec(v) => v.iter().find_map(|s| project_text(s, name)),
                _ => None,
            }
        }
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.projects = true;
        app.sessions.truncate(1);
        let project = app.prefs.cwd.clone();
        app.sessions[0].project = project.clone();
        app.demo = false;
        app.client = Some(rpc::Client::stub());
        app.connected = true;
        app.state.busy = true;
        app.draft = "preserved".into();
        let original = app.ensure_agent();
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|s| project_text(&s.shape, &sessions::project_name(&project)))
            .expect("Project heading");
        // Header text starts 40 px from the left; plus is 12 px from the right.
        let plus = egui::pos2(254.0 - 12.0 - 12.0, pos.y);
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut app,
                vec![
                    egui::Event::PointerMoved(plus),
                    egui::Event::PointerButton {
                        pos: plus,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(app.chat_folder_rx.is_none());
        assert!(app.new_chat);
        assert!(app.draft.is_empty());
        assert_eq!(app.prefs.cwd, sync::identity(&project));
        assert!(
            !app.closed_projects.contains(&project),
            "Plus must not collapse the project"
        );
        app.view_agent(&original);
        assert!(app.state.busy);
        assert_eq!(app.draft, "preserved");
    }
    #[test]
    fn chat_rows_use_pointer_and_overflow_opens_menu() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        for _ in 0..2 {
            frame(&ctx, &mut app, vec![]);
        }
        let mut y = 0.0;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let before = ui.cursor().top();
                app.chat_row(ui, 0);
                y = before + 15.0;
            });
        });
        let pos = egui::pos2(65.0, y);
        let output = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::PointerMoved(pos)],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.chat_row(ui, 0);
                });
            },
        );
        assert_eq!(
            output.platform_output.cursor_icon,
            egui::CursorIcon::PointingHand
        );
        let menu = egui::pos2(1000.0 - 8.0 - 12.0, y + 3.0);
        for pressed in [true, false] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 800.0),
                    )),
                    events: vec![
                        egui::Event::PointerMoved(menu),
                        egui::Event::PointerButton {
                            pos: menu,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.chat_row(ui, 0);
                    });
                },
            );
        }
        assert!(
            egui::Popup::is_any_open(&ctx),
            "Chat overflow should open a menu"
        );
    }
    #[test]
    fn every_visible_chat_row_can_be_opened() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let template = app.sessions[0].clone();
        app.sessions = (0..150)
            .map(|i| Session {
                path: PathBuf::from(format!("/fixture/chat-{i}.jsonl")),
                title: format!("Clickable chat {i}"),
                ..template.clone()
            })
            .collect();
        app.demo = false;
        app.connected = true;
        app.client = Some(rpc::Client::stub());
        app.state = State::default();
        app.state.messages = (0..30)
            .map(|i| crate::protocol::Message {
                role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
                text: format!("Message {i} {}", "Some text ".repeat(30)),
                ..Default::default()
            })
            .collect();
        let messages = app.state.messages.clone();
        let mut window = eframe::Frame::_new_kittest();
        let mut full_frame = |app: &mut Pui, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1500.0, 950.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(app, ctx, &mut window),
            )
        };
        let output = full_frame(&mut app, vec![]);
        full_frame(&mut app, vec![]);
        fn positions(shape: &egui::Shape, out: &mut Vec<egui::Pos2>) {
            match shape {
                egui::Shape::Text(t) if t.galley.job.text.starts_with("Clickable chat ") => {
                    out.push(t.pos + egui::vec2(12.0, 6.0))
                }
                egui::Shape::Vec(v) => {
                    for s in v {
                        positions(s, out)
                    }
                }
                _ => {}
            }
        }
        let mut rows = Vec::new();
        for shape in output.shapes {
            if shape.clip_rect.top() < 900.0 {
                positions(&shape.shape, &mut rows);
            }
        }
        assert!(rows.len() > 8, "Need a multi-row interaction test");
        for (index, mut pos) in rows.into_iter().enumerate().take(12) {
            pos.x = 150.0;
            app.state.messages = messages.clone();
            full_frame(&mut app, vec![]);
            full_frame(&mut app, vec![]);
            for pressed in [true, false] {
                full_frame(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert_eq!(
                app.pending_session.as_ref(),
                Some(&app.sessions[index].path),
                "Row {index} did not open"
            );
        }
    }
    #[test]
    fn row_identity_survives_refresh_and_reordering_between_press_and_release() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.demo = false;
        let mut row = egui::Rect::NOTHING;
        let mut render = |app: &mut Pui, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        for index in 0..app.sessions.len() {
                            let before = ui.cursor().top();
                            app.chat_row(ui, index);
                            if index == 0 {
                                row = egui::Rect::from_min_size(
                                    egui::pos2(8.0, before),
                                    egui::vec2(900.0, 48.0),
                                );
                            }
                        }
                    });
                },
            )
        };
        render(&mut app, vec![]);
        render(&mut app, vec![]);
        // Start on row zero, then swap the session data as a refresh/pin can do.
        let pos = row.left_center() + egui::vec2(80.0, 0.0);
        let mut clicked = Vec::new();
        for (stage, pressed) in [true, false].into_iter().enumerate() {
            if stage == 1 {
                app.sessions.swap(0, 1);
            }
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 800.0),
                    )),
                    events: vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        for index in 0..app.sessions.len() {
                            if let Some(Action::Open(i)) = app.chat_row(ui, index) {
                                clicked.push(app.sessions[i].path.clone());
                            }
                        }
                    });
                },
            );
        }
        assert!(
            clicked.iter().all(|p| p == &app.sessions[1].path),
            "Reordering opened a different chat than the pressed one: {clicked:?}"
        );
    }
    #[test]
    fn filtered_and_grouped_chat_rows_remain_clickable() {
        for projects in [false, true] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.demo = false;
            app.connected = true;
            app.state = State::default();
            app.client = Some(rpc::Client::stub());
            app.projects = projects;
            app.search = "snake".into();
            let output = frame(&ctx, &mut app, vec![]);
            frame(&ctx, &mut app, vec![]);
            fn find(shape: &egui::Shape) -> Option<egui::Pos2> {
                match shape {
                    egui::Shape::Text(t)
                        if t.galley.job.text.starts_with("we have more models") =>
                    {
                        Some(t.pos + egui::vec2(40.0, 0.0))
                    }
                    egui::Shape::Vec(v) => v.iter().find_map(find),
                    _ => None,
                }
            }
            let pos = output
                .shapes
                .iter()
                .find_map(|s| find(&s.shape))
                .expect("Filtered target visible");
            let path = app
                .sessions
                .iter()
                .find(|s| s.title.starts_with("we have more models"))
                .unwrap()
                .path
                .clone();
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert_eq!(
                app.pending_session,
                Some(path),
                "Filtered chat did not open; projects={projects}"
            );
        }
    }
    #[test]
    fn working_chat_can_switch_by_pointer_without_registered_live_ownership() {
        for projects in [false, true] {
            for phase in ["working", "pending", "compacting", "approval"] {
                let ctx = egui::Context::default();
                let mut app = app(&ctx);
                app.demo = false;
                app.connected = true;
                app.client = Some(rpc::Client::stub());
                app.projects = projects;
                let first = app.sessions[0].clone();
                let second = app.sessions[1].clone();
                app.state = State {
                    session_file: Some(first.path.display().to_string()),
                    session_name: first.title.clone(),
                    busy: phase == "working" || phase == "approval",
                    pending: phase == "pending",
                    compacting: phase == "compacting",
                    ..Default::default()
                };
                if phase == "approval" {
                    app.state
                        .dialogs
                        .push_back(json!({"id":"approval","method":"confirm"}));
                }
                app.state.messages.push(Message {
                    role: "assistant".into(),
                    text: "still running".into(),
                    ..Default::default()
                });
                app.draft = "keep my draft".into();
                assert!(!app.sync_live_owner());
                fn find(shape: &egui::Shape, title: &str) -> Option<egui::Pos2> {
                    match shape {
                        egui::Shape::Text(t) if t.galley.job.text == title => {
                            Some(t.pos + egui::vec2(30.0, 4.0))
                        }
                        egui::Shape::Vec(v) => v.iter().find_map(|s| find(s, title)),
                        _ => None,
                    }
                }
                frame(&ctx, &mut app, vec![]);
                let output = frame(&ctx, &mut app, vec![]);
                let pos = output
                    .shapes
                    .iter()
                    .find_map(|s| find(&s.shape, &second.title))
                    .expect("target chat is visible");
                for pressed in [true, false] {
                    frame(
                        &ctx,
                        &mut app,
                        vec![
                            egui::Event::PointerMoved(pos),
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    );
                }
                assert_eq!(
                    app.pending_session.as_ref(),
                    Some(&second.path),
                    "blocked {phase} chat; projects={projects}"
                );
                app.open_chat(first.clone());
                assert_eq!(app.state.session_file.as_deref(), first.path.to_str());
                assert_eq!(app.state.messages[0].text, "still running");
                assert_eq!(app.draft, "keep my draft");
                assert!(
                    app.client.is_some(),
                    "the running connection must stay alive"
                );
                assert!(
                    !app.state.ready(),
                    "navigation must not stop/settle the old run"
                );
                if phase == "approval" {
                    assert_eq!(app.state.dialogs.len(), 1);
                }
                app.shutdown_agents();
            }
        }
    }
    #[test]
    fn another_chat_remains_clickable_while_previous_chat_is_connecting() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.demo = false;
        app.connected = true;
        app.client = Some(rpc::Client::stub());
        app.state = State::default();
        let first = app.sessions[0].clone();
        let second = app.sessions[1].clone();
        app.prefs.cwd = first.cwd.clone();
        app.open_chat(first.clone());
        assert_eq!(app.pending_session.as_ref(), Some(&first.path));
        app.open_chat(second.clone());
        assert_eq!(
            app.pending_session.as_ref(),
            Some(&second.path),
            "A slow connection must not lock out other chats"
        );
        if let Some(client) = app.client.take() {
            client.retire();
        }
    }
    #[test]
    fn completed_idle_switch_allows_the_next_chat() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.demo = false;
        app.connected = true;
        app.state = State::default();
        let (client, events) = rpc::Client::stub_with_events();
        app.client = Some(client);
        let first = app.sessions[0].clone();
        let second = app.sessions[1].clone();
        app.prefs.cwd = first.cwd.clone();
        app.open_chat(first.clone());
        events.send(rpc::Event::Record(json!({"type":"response","command":"switch_session","success":true,"data":{"cancelled":false}}))).unwrap();
        events.send(rpc::Event::Record(json!({"type":"response","command":"get_state","success":true,"data":{"sessionFile":first.path,"isStreaming":false,"isCompacting":false}}))).unwrap();
        events.send(rpc::Event::History(Vec::new())).unwrap();
        app.poll();
        assert!(!app.loading_chat && app.state.ready());
        app.open_chat(second.clone());
        assert_eq!(app.pending_session.as_ref(), Some(&second.path));
    }
    #[test]
    fn abandoned_client_events_cannot_reselect_the_previous_chat() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.demo = false;
        app.connected = true;
        app.state = State::default();
        let (client, events) = rpc::Client::stub_with_events();
        app.client = Some(client);
        let first = app.sessions[0].clone();
        let second = app.sessions[1].clone();
        app.prefs.cwd = first.cwd.clone();
        app.open_chat(first.clone());
        events.send(rpc::Event::Record(json!({"type":"response","command":"get_state","success":true,"data":{"sessionFile":first.path}}))).unwrap();
        events
            .send(rpc::Event::History(vec![Message {
                text: "stale old history".into(),
                ..Default::default()
            }]))
            .unwrap();
        app.open_chat(second.clone());
        app.preview_rx = None;
        app.poll();
        assert_eq!(app.pending_session.as_ref(), Some(&second.path));
        assert_eq!(
            app.state.session_file.as_deref(),
            Some(second.path.to_str().unwrap())
        );
        assert!(app.state.messages.is_empty());
    }
    #[test]
    fn same_folder_chat_switch_reuses_pi_instead_of_restarting() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let cwd = app.prefs.cwd.clone();
        let client = rpc::Client::stub();
        app.client = Some(client);
        app.connected = true;
        app.demo = false;
        let session = Session {
            path: PathBuf::from("/missing/session-preview.jsonl"),
            cwd: cwd.clone(),
            project: cwd,
            title: "Target chat".into(),
            modified: std::time::SystemTime::now(),
        };
        app.open_chat(session.clone());
        assert!(app.loading_chat);
        assert_eq!(app.pending_session, Some(session.path));
        assert_eq!(app.state.session_name, "Target chat");
        if let Some(client) = app.client.take() {
            client.retire();
        }
    }
    #[test]
    fn session_scroll_area_reaches_actual_footer() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let template = app.sessions[0].clone();
        app.sessions = (0..60)
            .map(|i| Session {
                path: PathBuf::from(format!("fixture-{i}")),
                ..template.clone()
            })
            .collect();
        let output = frame(&ctx, &mut app, vec![]);
        fn inspect(shape: &egui::Shape, last: &mut f32, footer: &mut f32) {
            match shape {
                egui::Shape::Text(t) => {
                    if t.galley.job.text.starts_with("make sure") {
                        *last = last.max(t.pos.y);
                    }
                    if t.galley.job.text == "Local account" {
                        *footer = t.pos.y;
                    }
                }
                egui::Shape::Vec(v) => {
                    for s in v {
                        inspect(s, last, footer);
                    }
                }
                _ => {}
            }
        }
        let mut last = 0.0;
        let mut footer = 0.0;
        for shape in output.shapes {
            if shape.clip_rect.bottom() > 700.0 {
                inspect(&shape.shape, &mut last, &mut footer);
            }
        }
        assert!(footer > 700.0, "Footer should stay at the bottom: {footer}");
        assert!(
            last > footer - 90.0,
            "Session list stops too early: last {last}, footer {footer}"
        );
    }
    #[test]
    fn project_shows_five_latest_chats_and_expands_on_demand() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.projects = true;
        let template = app.sessions[0].clone();
        app.sessions = (0..9)
            .map(|i| Session {
                path: PathBuf::from(format!("fixture-{i}")),
                title: format!("Project chat {i}"),
                modified: std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(i),
                ..template.clone()
            })
            .collect();
        fn collect(shape: &egui::Shape, text: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => text.push(t.galley.job.text.clone()),
                egui::Shape::Vec(v) => {
                    for s in v {
                        collect(s, text)
                    }
                }
                _ => {}
            }
        }
        let mut text = Vec::new();
        for s in frame(&ctx, &mut app, vec![]).shapes {
            collect(&s.shape, &mut text);
        }
        assert_eq!(
            text.iter()
                .filter(|s| s.starts_with("Project chat "))
                .count(),
            5
        );
        assert!(text.iter().any(|s| s == "Project chat 8"));
        assert!(!text.iter().any(|s| s == "Project chat 0"));
        assert!(text.iter().any(|s| s == "Show 4 more"));
        app.expanded_projects.insert(template.project);
        text.clear();
        for s in frame(&ctx, &mut app, vec![]).shapes {
            collect(&s.shape, &mut text);
        }
        assert_eq!(
            text.iter()
                .filter(|s| s.starts_with("Project chat "))
                .count(),
            9
        );
        assert!(text.iter().any(|s| s == "Show less"));
    }
    #[test]
    fn projects_show_all_repository_folders() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.projects = true;
        assert!(
            app.sessions
                .iter()
                .map(|s| &s.project)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                > 1
        );
        let output = frame(&ctx, &mut app, vec![]);
        fn collect(shape: &egui::Shape, text: &mut String) {
            match shape {
                egui::Shape::Text(s) => text.push_str(&s.galley.job.text),
                egui::Shape::Vec(v) => {
                    for s in v {
                        collect(s, text);
                    }
                }
                _ => {}
            }
        }
        let mut text = String::new();
        for shape in output.shapes {
            collect(&shape.shape, &mut text);
        }
        for name in ["remote-control-agents", "snake", "pui"] {
            assert!(text.contains(name), "Missing project {name}");
        }
    }
}
