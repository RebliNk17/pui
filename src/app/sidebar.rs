use super::*;
use std::collections::BTreeMap;

mod activity;
#[path = "sidebar_dates.rs"]
mod dates;
#[cfg(test)]
mod disclosure_tests;
pub(super) mod sections;
const ROW_HEIGHT: f32 = 28.0;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct ChatPrefs {
    pub(super) title: Option<String>,
    pub(super) project: Option<PathBuf>,
    pub(super) pinned: bool,
    pub(super) archived: bool,
    pub(super) hidden: bool,
    pub(super) section: Option<String>,
}
#[derive(Clone)]
enum Action {
    Rename(PathBuf),
    Move(PathBuf),
    Export(PathBuf),
    Hide(PathBuf),
    Restore(PathBuf),
    Delete(deletion::Target),
    Open(usize),
    Agent(String),
    NewChat(PathBuf),
    Task(String, Option<String>),
    Saved(PathBuf, PathBuf),
}

impl Pui {
    pub(super) fn sidebar(&mut self, ctx: &egui::Context) {
        let colors = crate::theme::palette(ctx);
        let mut action = None;
        let tasks_loading = self.tasks.loading_history();
        let (mut pinned, branches) = sidebar_tree::listing(self);
        let fresh_chat = self.new_chat.then(|| self.agents.active.clone());
        let fresh_id = egui::Id::new("sidebar-fresh-chat");
        let reveal_new_chat = fresh_chat.is_some()
            && ctx.data(|data| data.get_temp::<String>(fresh_id)) != fresh_chat;
        ctx.data_mut(|data| {
            if let Some(id) = fresh_chat {
                data.insert_temp(fresh_id, id);
            } else {
                data.remove::<String>(fresh_id);
            }
        });
        egui::SidePanel::left("sessions-v4")
            .default_width(254.0)
            .width_range(220.0..=480.0)
            .resizable(true)
            .frame(egui::Frame::new().fill(colors.sidebar).inner_margin(12))
            .show(ctx, |ui| {
                // Native traffic lights occupy the top-left strip on macOS.
                // Keep app content below them instead of offsetting only the name.
                #[cfg(target_os = "macos")]
                ui.add_space((32.0 / ctx.zoom_factor() - 12.0).max(12.0));
                ui.horizontal(|ui| {
                    ui.set_height(theme::CONTROL_HEIGHT);
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new("Pui").size(13.0).strong().color(colors.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        self.attention_button(ui);
                        let new_chat = icons::button(ui, Icon::Plus, "New chat · ⌘N");
                        self.attention_reverse_from(new_chat.id);
                        if new_chat.clicked() {
                            action = Some(Action::NewChat(self.prefs.cwd.clone()));
                        }
                    });
                });
                ui.add_space(6.0);
                self.sidebar_navigation(ui);
                ui.add_space(6.0);
                // Reserve exactly the footer, not a 95px estimate followed by extra layout padding.
                egui::TopBottomPanel::bottom("account-footer-v2")
                    .exact_height(43.0)
                    .show_separator_line(false)
                    .frame(egui::Frame::new().fill(colors.sidebar))
                    .show_inside(ui, |ui| {
                        ui.add(egui::Separator::default().spacing(1.0));
                        self.usage.footer(ui, self.demo);
                    });
                ui.horizontal(|ui| {
                    ui.label(theme::caption(
                        ui,
                        if self.show_hidden {
                            "HIDDEN CHATS"
                        } else if self.show_archived {
                            "ARCHIVED"
                        } else if self.projects {
                            "PROJECTS"
                        } else {
                            "SESSIONS"
                        },
                    ));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        self.session_list_controls(ui);
                    });
                });
                // Spotlight is an overlay, never part of the sidebar layout.
                let query = String::new();
                let mut visible: Vec<_> = branches
                    .iter()
                    .enumerate()
                    .filter(|(_, branch)| {
                        !(tasks_loading
                            && matches!(branch.target, sidebar_tree::Target::Session(_)))
                            && (self.show_hidden || branch.archived == self.show_archived)
                            && branch.matches(&query)
                    })
                    .map(|(index, _)| index)
                    .collect();
                let order = self.prefs.session_order;
                pinned.sort_by(|a, b| order.compare(a, b));
                visible.sort_by(|a, b| order.compare(&branches[*a], &branches[*b]));
                let section_groups = sections::partition(self, &branches, &mut visible);
                let mut groups: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
                if self.projects {
                    for index in &visible {
                        groups
                            .entry(branches[*index].project.clone())
                            .or_default()
                            .push(*index);
                    }
                }
                session_scroll(reveal_new_chat)
                    .id_salt((
                        "session-list-v4",
                        self.projects,
                        self.show_archived,
                        self.show_hidden,
                    ))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        if !pinned.is_empty() {
                            ui.label(theme::caption(ui, "PINNED"));
                            for branch in &pinned {
                                if let Some(next) = self.branch_row(ui, branch, 0, "") {
                                    action = Some(next);
                                }
                            }
                            ui.add_space(6.0);
                            ui.separator();
                        }
                        if let Some(next) =
                            self.section_rows(ui, &branches, &section_groups, &query)
                        {
                            action = Some(next);
                        }
                        if visible.is_empty() && pinned.is_empty() && section_groups.is_empty() {
                            ui.label(theme::small(
                                ui,
                                if self.session_rx.is_some() {
                                    "Loading sessions…"
                                } else if tasks_loading {
                                    "Loading workflows…"
                                } else if self.show_hidden {
                                    "No hidden chats."
                                } else {
                                    "No sessions found."
                                },
                            ));
                        }
                        if self.projects {
                            let mut groups: Vec<_> = groups.into_iter().collect();
                            groups.sort_by(|a, b| {
                                order.compare_groups((&a.0, &a.1), (&b.0, &b.1), &branches)
                            });
                            for (project, indices) in groups {
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
                                    let expanded = self.prefs.sidebar_view.view_all
                                        || self.expanded_projects.contains(&project);
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
                                            self.prefs.sidebar_view.view_all = false;
                                            self.expanded_projects.remove(&project);
                                        } else {
                                            self.expanded_projects.insert(project.clone());
                                        }
                                    }
                                }
                                ui.add_space(6.0);
                            }
                        } else {
                            let today = chrono::Local::now().date_naive();
                            let group = self.prefs.sidebar_view.group;
                            if matches!(
                                group,
                                sidebar_view::Group::Status | sidebar_view::Group::Environment
                            ) {
                                let mut grouped = BTreeMap::<String, Vec<usize>>::new();
                                for index in visible {
                                    let label = if group == sidebar_view::Group::Status {
                                        self.sidebar_status(&branches[index])
                                    } else {
                                        "Local workspace".into()
                                    };
                                    grouped.entry(label).or_default().push(index);
                                }
                                for (label, indices) in grouped {
                                    date_separator(ui, &label, false);
                                    for index in indices {
                                        if let Some(next) =
                                            self.branch_row(ui, &branches[index], 0, &query)
                                        {
                                            action = Some(next);
                                        }
                                    }
                                }
                            } else {
                                let mut previous_date = None;
                                for index in visible {
                                    let branch = &branches[index];
                                    let date = (group != sidebar_view::Group::None)
                                        .then(|| {
                                            dates::Bucket::for_branch(branch, order.sort, today)
                                        })
                                        .flatten();
                                    if date != previous_date {
                                        if let Some(date) = date {
                                            date_separator(
                                                ui,
                                                date.label(),
                                                previous_date.is_none(),
                                            );
                                        }
                                        previous_date = date;
                                    }
                                    if let Some(next) = self.branch_row(ui, branch, 0, &query) {
                                        action = Some(next);
                                    }
                                }
                            }
                        }
                        if self.prefs.sidebar_view.empty_groups && self.projects {
                            for project in &self.prefs.projects {
                                if !branches.iter().any(|branch| &branch.project == project) {
                                    ui.label(
                                        theme::small(ui, sessions::project_name(project)).strong(),
                                    );
                                    ui.label(theme::small(ui, "No chats"));
                                }
                            }
                        }
                        if !self.show_hidden
                            && (self.show_archived || self.prefs.chats.values().any(|m| m.archived))
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
                self.finish_chat_section_drag(ctx);
            });
        if let Some(action) = action {
            match action {
                Action::Agent(id) => self.view_agent(&id),
                Action::NewChat(folder) => self.start_chat_in(folder),
                Action::Delete(target) => self.request_delete(target),
                Action::Task(task, run) => {
                    self.kanban.open = false;
                    self.open_task_from_board(task, run);
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
                        created: None,
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
                Action::Restore(path) => {
                    self.prefs.chats.entry(path).or_default().hidden = false;
                }
                _ => {}
            }
        }
    }
    fn session_list_controls(&mut self, ui: &mut egui::Ui) -> [egui::Response; 2] {
        ui.scope(|ui| {
            theme::compact_controls(ui);
            ui.spacing_mut().item_spacing.x = 2.0;
            let view = self.sidebar_view_options(ui);
            let search = icons::button(
                ui,
                Icon::Search,
                "Search chats, messages and settings · ⌘K / Ctrl+K",
            );
            if search.clicked() {
                self.open_search();
            }
            [view, search]
        })
        .inner
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
        if !branch.pinned && branch.hidden != self.show_hidden {
            return None;
        }
        let colors = theme::palette(ui.ctx());
        let selected = branch.active(self);
        let unread = self.sidebar_unread(branch);
        let deleting = branch
            .path
            .as_deref()
            .is_some_and(|p| self.deletion.deleting_chat(p));
        let expanded = !self.prefs.closed_threads.contains(&branch.key) || !query.is_empty();
        let has_children = branch
            .children
            .iter()
            .any(|c| c.pinned || c.hidden == self.show_hidden);
        let compact = !self.prefs.sidebar_view.metadata || self.projects || depth > 0;
        let task_row = matches!(branch.target, sidebar_tree::Target::Task(_, None));
        let row_height = if compact { ROW_HEIGHT } else { 44.0 };
        if depth > 0 {
            let x = ui.cursor().left() + ((depth - 1) as f32 * 12.0).min(48.0) + 7.0;
            ui.painter().line_segment(
                [
                    egui::pos2(x, ui.cursor().top()),
                    egui::pos2(x, ui.cursor().top() + row_height),
                ],
                Stroke::new(1.0, colors.border),
            );
        }
        let indent = (depth as f32 * 12.0).min(48.0);
        let mut action = None;
        ui.push_id((&branch.key, "tree-row"), |ui| {
            let origin = ui.cursor().min;
            let rect =
                egui::Rect::from_min_size(origin, Vec2::new(ui.available_width(), row_height));
            let left = origin.x + indent;
            let center_y = origin.y
                + if compact {
                    row_height / 2.0
                } else if matches!(branch.target, sidebar_tree::Target::Session(_)) && depth == 0 {
                    15.0
                } else {
                    13.0
                };
            let toggle_rect =
                egui::Rect::from_center_size(egui::pos2(left + 8.0, center_y), Vec2::splat(20.0));
            let can_disclose = has_children
                && self.prefs.sidebar_view.side_chats
                && !deleting
                && branch
                    .path
                    .as_ref()
                    .is_none_or(|path| self.rename_chat.as_ref() != Some(path));
            let toggle_id = ui.id().with("disclosure");
            let mut show_disclosure = can_disclose
                && (ui.rect_contains_pointer(rect)
                    || ui.memory(|memory| memory.has_focus(toggle_id)));
            #[cfg(test)]
            if can_disclose {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new(("sidebar-disclosure-rect", &branch.key)),
                        toggle_rect,
                    )
                });
            }
            let child_left = left;
            let row_response = ui.scope_builder(
                egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                    egui::pos2(child_left, origin.y),
                    egui::pos2(rect.right(), ui.max_rect().bottom()),
                )),
                |ui| match &branch.target {
                    sidebar_tree::Target::Session(index) if depth == 0 => {
                        action = self.chat_row(
                            ui,
                            *index,
                            branch.activity,
                            can_disclose.then_some(&mut show_disclosure),
                        )
                    }
                    target => {
                        let (row, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), row_height),
                            if depth==0 && matches!(target,sidebar_tree::Target::Agent(_)){Sense::click_and_drag()}else{Sense::click()},
                        );
                        let response = if unread {
                            response.on_hover_text("Unread observed update · this chat only. Viewing the sidebar does not mark it read.")
                        } else {
                            response
                        };
                        if selected || response.hovered() || task_row {
                            ui.painter().rect_filled(
                                row,
                                7,
                                if selected {
                                    colors.accent_bg
                                } else if response.hovered() {
                                    colors.hover
                                } else {
                                    colors.surface
                                },
                            );
                        }
                        let tint = if crate::theme::status::is_working(&branch.status) {
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
                        let icon_rect = egui::Rect::from_center_size(
                            row.min + egui::vec2(8.0, title_y),
                            Vec2::splat(16.0),
                        );
                        show_disclosure |= can_disclose && response.has_focus();
                        if deleting {
                            deletion_spinner(ui, icon_rect.center());
                        } else if !show_disclosure {
                            theme::status_indicator(
                                ui,
                                icon_rect.center(),
                                &branch.status,
                                tint,
                                2.5,
                            );
                            ui.interact(icon_rect, ui.id().with("branch-status"), Sense::hover())
                                .on_hover_text(&branch.status);
                        }
                        attention::paint_unread(ui, row, title_y, unread);
                        let title_elided = paint_text(
                            ui,
                            &branch.title,
                            row.min + egui::vec2(32.0, title_y),
                            (row.width() - 107.0).max(1.0),
                            FontId::proportional(if compact { 11.5 } else { 12.0 }),
                            colors.text,
                        );
                        activity::paint(
                            ui,
                            egui::Rect::from_min_size(
                                row.min,
                                Vec2::new(row.width(), if compact { row.height() } else { 26.0 }),
                            ),
                            branch.activity,
                        );
                        if !compact {
                            paint_text(
                                ui,
                                if deleting {
                                    "Deleting…"
                                } else {
                                    &branch.status
                                },
                                row.min + egui::vec2(32.0, 30.0),
                                row.width() - 43.0,
                                FontId::proportional(10.0),
                                colors.dim,
                            );
                        }
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::SelectableLabel,
                                true,
                                selected,
                                format!("{} · {}{}", branch.title, branch.status, if unread { " · Unread observed update" } else { "" }),
                            )
                        });
                        if let sidebar_tree::Target::Task(id,None)=target
                            && let Some(summary)=self.tasks.records.iter().find(|t|&t.id==id).and_then(|t|t.checkpoint_summary()){
                                response.clone().on_hover_text(summary);
                        }
                        if deleting {
                            response.on_hover_text(
                                "Deleting chat… You can keep working in other chats.",
                            );
                            return;
                        }
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
                        let unsaved_root = match target {
                            sidebar_tree::Target::Agent(id) if depth == 0 => self
                                .agents
                                .nodes
                                .get(id)
                                .filter(|node| node.parent.is_none() && node.path.is_none())
                                .map(|_| id.clone()),
                            _ => None,
                        };
                        let has_menu = delete_target.is_some()
                            || branch.path.is_some()
                            || unsaved_root.is_some();
                        if unsaved_root.is_some()
                            && ui.memory(|memory| memory.has_focus(ui.id().with("delete-menu")))
                        {
                            ui.input_mut(|input| {
                                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
                            });
                        }
                        let menu = has_menu.then(|| {
                            ui.interact(menu_rect, ui.id().with("delete-menu"), Sense::click())
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text("More options")
                        });
                        if has_menu
                            && (selected
                                || response.hovered()
                                || menu.as_ref().is_some_and(|m| m.hovered()))
                        {
                            for dx in [-4.0, 0.0, 4.0] {
                                ui.painter().circle_filled(
                                    menu_rect.center() + egui::vec2(dx, 0.0),
                                    1.0,
                                    colors.muted,
                                );
                            }
                        }
                        #[cfg(test)]
                        if let Some(menu) = &menu {
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(
                                    egui::Id::new(("sidebar-unsaved-menu", &branch.key)),
                                    menu.rect,
                                );
                                data.insert_temp(
                                    egui::Id::new(("sidebar-unsaved-menu-id", &branch.key)),
                                    menu.id,
                                )
                            });
                        }
                        if let Some(menu) = &menu {
                            if unsaved_root.is_some() {
                                let popup = if menu.clicked() {
                                    egui::Popup::default_response_id(menu)
                                } else {
                                    response.id.with("popup")
                                };
                                if menu.clicked() || response.secondary_clicked() {
                                    ui.input_mut(|input| {
                                        input.consume_key(
                                            egui::Modifiers::COMMAND,
                                            egui::Key::Enter,
                                        );
                                        input.consume_key(
                                            egui::Modifiers::COMMAND,
                                            egui::Key::Backspace,
                                        );
                                    });
                                    self.section_view.unsaved_popup = Some(popup);
                                }
                            }
                            let mut contents = |ui: &mut egui::Ui| {
                                theme::menu_controls(ui);
                                if let Some(path) = &branch.path {
                                    let meta = self.prefs.chats.entry(path.clone()).or_default();
                                    if ui
                                        .button(if meta.pinned { "Unpin" } else { "Pin" })
                                        .clicked()
                                    {
                                        meta.pinned = !meta.pinned;
                                        ui.close();
                                    }
                                    if depth == 0
                                        && !matches!(
                                            branch.target,
                                            sidebar_tree::Target::Task(_, _)
                                        )
                                    {
                                        sections::assignment_menu(
                                            ui,
                                            &self.prefs.sidebar_view.sections,
                                            meta,
                                        );
                                    }
                                    if meta.hidden && ui.button("Restore chat").clicked() {
                                        action = Some(Action::Restore(path.clone()));
                                        ui.close();
                                    }
                                }
                                if let Some(id) = &unsaved_root {
                                    self.unsaved_section_menu(ui, id);
                                }
                                if let Some(target) = &delete_target {
                                    let label = if matches!(target, deletion::Target::Task(_)) {
                                        "Delete workflow history…"
                                    } else {
                                        "Delete chat…"
                                    };
                                    if ui.button(RichText::new(label).color(colors.red)).clicked() {
                                        action = Some(Action::Delete(target.clone()));
                                        ui.close();
                                    }
                                }
                            };
                            egui::Popup::menu(menu).show(&mut contents);
                            response.context_menu(&mut contents);
                        }
                        if let Some(id)=&unsaved_root{self.runtime_chat_section_source(ui,id,&response,menu_rect,toggle_rect);}
                        let keyboard = response.has_focus()
                            && ui.input_mut(|i| {
                                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                            });
                        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                        if title_elided {
                            ui.interact(
                                egui::Rect::from_min_max(
                                    row.min + egui::vec2(32.0, 0.0),
                                    egui::pos2(row.right() - 75.0, row.bottom()),
                                ),
                                ui.id().with("branch-title"),
                                Sense::hover(),
                            )
                            .on_hover_text(&branch.title);
                        }
                        if (response.clicked()
                            && !(can_disclose
                                && toggle_rect.contains(
                                    ui.input(|i| i.pointer.interact_pos()).unwrap_or_default(),
                                ))
                            && !(has_menu
                                && menu_rect.contains(
                                    ui.input(|i| i.pointer.interact_pos()).unwrap_or_default(),
                                )))
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
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::new(("sidebar-row-rect", &branch.key)),
                    row_response.response.rect,
                )
            });
            #[cfg(not(test))]
            let _ = row_response;
            // scope_builder already advances the parent cursor. Allocating again
            // here used to add a second spacing gap to every root and child.
            // Register after the row so the disclosure owns its independent hit lane.
            let toggle = can_disclose.then(|| ui.interact(toggle_rect, toggle_id, Sense::click()));
            if let Some(toggle) = toggle {
                if toggle.clicked() {
                    toggle.request_focus();
                }
                let highlighted = toggle.hovered() || toggle.has_focus();
                if show_disclosure {
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
                }
                let children = branch
                    .children
                    .iter()
                    .filter(|child| child.pinned || child.hidden == self.show_hidden)
                    .count();
                let status = self.sidebar_status(branch);
                toggle.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::CollapsingHeader,
                        true,
                        expanded,
                        format!("{} children · {children} · {status}", branch.title),
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
                        "{} {children} children · {}",
                        if expanded { "Hide" } else { "Show" },
                        status
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
        if expanded && self.prefs.sidebar_view.side_chats {
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
    pub(super) fn session_title(&self, session: &Session) -> String {
        let live_title = || {
            if self
                .state
                .session_file
                .as_deref()
                .is_some_and(|path| std::path::Path::new(path) == session.path)
            {
                self.state.chat_title()
            } else {
                self.agents
                    .parked
                    .values()
                    .find(|chat| {
                        chat.state
                            .session_file
                            .as_deref()
                            .is_some_and(|path| std::path::Path::new(path) == session.path)
                    })
                    .and_then(|chat| chat.state.chat_title())
            }
        };
        self.prefs
            .chats
            .get(&session.path)
            .and_then(|meta| meta.title.clone())
            .or_else(live_title)
            .unwrap_or_else(|| session.title.clone())
    }

    fn chat_row(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        activity: Option<std::time::SystemTime>,
        mut disclosure: Option<&mut bool>,
    ) -> Option<Action> {
        let colors = crate::theme::palette(ui.ctx());
        let s = &self.sessions[index];
        let path = s.path.clone();
        let unread = self.saved_unread(&path);
        let deleting = self.deletion.deleting_chat(&path);
        let selected = !self.kanban.open
            && !self.automation.open
            && !self.tasks.open
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
        let (status, status_color) = if deleting {
            ("Deleting…", colors.red)
        } else if selected {
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
        let title = self.session_title(s);
        let meta = self.prefs.chats.entry(path.clone()).or_default();
        let editing = !deleting && self.rename_chat.as_ref() == Some(&path);
        let compact = !self.prefs.sidebar_view.metadata || self.projects;
        let (_, rect) = ui.allocate_space(Vec2::new(
            ui.available_width(),
            if compact { ROW_HEIGHT } else { 44.0 },
        ));
        // Session identity, not layout position: filtering, pinning, regrouping or
        // asynchronous refresh must not move a pointer press to another chat.
        let response = ui.interact(
            rect,
            ui.make_persistent_id(("chat-row", &path)),
            if editing || deleting {
                Sense::hover()
            } else {
                Sense::click_and_drag()
            },
        );
        let response = if deleting {
            response.on_hover_text("Deleting chat… You can keep working in other chats.")
        } else {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        };
        let response = if unread {
            response.on_hover_text("Unread observed update · this chat only. Viewing the sidebar does not mark it read.")
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
        let indicator =
            rect.min + egui::vec2(8.0, if compact { rect.height() / 2.0 } else { 15.0 });
        if let Some(replace) = &mut disclosure {
            **replace |= response.has_focus();
        }
        if deleting {
            deletion_spinner(ui, indicator);
        } else if !disclosure.as_deref().copied().unwrap_or(false) {
            theme::status_indicator(ui, indicator, status, status_color, 3.0);
            ui.interact(
                egui::Rect::from_center_size(indicator, Vec2::splat(16.0)),
                ui.make_persistent_id(("chat-status", &path)),
                Sense::hover(),
            )
            .on_hover_text(status);
        }
        attention::paint_unread(ui, rect, indicator.y - rect.top(), unread);
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
                    rect.min + egui::vec2(32.0, 3.0),
                    egui::vec2((rect.width() - 62.0).max(20.0), 24.0),
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
            let elided = paint_text(
                ui,
                &title,
                rect.min + egui::vec2(32.0, if compact { rect.height() / 2.0 } else { 15.0 }),
                (rect.width() - 107.0).max(1.0),
                FontId::proportional(12.0),
                colors.text,
            );
            if elided && !deleting {
                ui.interact(
                    egui::Rect::from_min_max(
                        rect.min + egui::vec2(32.0, 0.0),
                        egui::pos2(rect.right() - 75.0, rect.bottom()),
                    ),
                    ui.make_persistent_id(("chat-title", &path)),
                    Sense::hover(),
                )
                .on_hover_text(&title);
            }
        }
        if !editing {
            activity::paint(
                ui,
                egui::Rect::from_min_size(
                    rect.min,
                    Vec2::new(rect.width(), if compact { rect.height() } else { 30.0 }),
                ),
                activity,
            );
        }
        if !compact {
            paint_text(
                ui,
                &format!(
                    "{} · {}",
                    sessions::project_name(&s.project),
                    if deleting || selected || node.is_some() || shared.is_some() {
                        status.to_owned()
                    } else {
                        format!("Saved · {}", sessions::age(s.modified))
                    }
                ),
                rect.min + egui::vec2(32.0, 33.0),
                rect.width() - 44.0,
                FontId::proportional(10.5),
                colors.dim,
            );
        }
        if deleting {
            return None;
        }
        let menu_rect = egui::Rect::from_center_size(
            rect.right_center() - egui::vec2(12.0, if compact { 0.0 } else { 6.0 }),
            Vec2::splat(24.0),
        );
        let menu = ui
            .interact(
                menu_rect,
                ui.make_persistent_id(("chat-menu", &path)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("More options");
        if selected || response.hovered() || menu.hovered() {
            for dx in [-4.0, 0.0, 4.0] {
                ui.painter().circle_filled(
                    menu_rect.center() + egui::vec2(dx, 0.0),
                    1.0,
                    colors.muted,
                );
            }
        }
        let sections = &self.prefs.sidebar_view.sections;
        let section_root = node.is_none_or(|node| node.parent.is_none());
        let mut action = None;
        let mut menu_contents = |ui: &mut egui::Ui| {
            theme::menu_controls(ui);
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
            if section_root {
                sections::assignment_menu(ui, sections, meta);
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
            if meta.hidden {
                if ui.button("Restore chat").clicked() {
                    action = Some(Action::Restore(path.clone()));
                    ui.close();
                }
            } else if ui
                .add_enabled(
                    !meta.pinned,
                    egui::Button::new(egui::RichText::new("Hide from Pui").color(colors.red)),
                )
                .on_hover_text("Does not delete your Pi session file")
                .on_disabled_hover_text("Unpin this chat before hiding it")
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
        let disclosure_rect = egui::Rect::from_center_size(
            egui::pos2(
                rect.left() + 8.0,
                rect.top() + if compact { rect.height() / 2.0 } else { 16.0 },
            ),
            Vec2::splat(20.0),
        );
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new(("chat-section-source", &path)),
                response.interact_rect,
            )
        });
        self.chat_section_source(ui, &path, &response, menu_rect, disclosure_rect);
        if response.clicked()
            && !(disclosure.is_some()
                && disclosure_rect
                    .contains(ui.input(|i| i.pointer.interact_pos()).unwrap_or_default()))
            && !menu_rect.contains(ui.input(|i| i.pointer.interact_pos()).unwrap_or_default())
        {
            action = Some(Action::Open(index));
        }
        action
    }
}
fn session_scroll(reveal_new_chat: bool) -> egui::ScrollArea {
    let scroll = egui::ScrollArea::vertical();
    if reveal_new_chat {
        scroll.vertical_scroll_offset(0.0)
    } else {
        scroll
    }
}

fn date_separator(ui: &mut egui::Ui, label: &str, first: bool) {
    ui.add_space(if first { 4.0 } else { 8.0 });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(theme::small(ui, label).strong());
        ui.add(egui::Separator::default().horizontal());
    });
}

fn deletion_spinner(ui: &egui::Ui, center: egui::Pos2) {
    egui::Spinner::new()
        .color(theme::palette(ui.ctx()).red)
        .paint_at(ui, egui::Rect::from_center_size(center, Vec2::splat(12.0)));
}
fn paint_text(
    ui: &egui::Ui,
    text: &str,
    pos: egui::Pos2,
    width: f32,
    font: FontId,
    color: Color32,
) -> bool {
    let mut job = egui::text::LayoutJob::simple_singleline(text.into(), font, color);
    job.wrap.max_width = width.max(20.0);
    job.wrap.max_rows = 1;
    let galley = ui.painter().layout_job(job);
    let elided = galley.elided;
    ui.painter()
        .galley(pos - egui::vec2(0.0, galley.size().y / 2.0), galley, color);
    elided
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
    fn sidebar_edge_resizes_in_both_views_and_clamps_width() {
        for projects in [false, true] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.projects = projects;
            let panel = egui::Id::new("sessions-v4");
            let width = || {
                egui::containers::panel::PanelState::load(&ctx, panel)
                    .unwrap()
                    .rect
                    .width()
            };
            for _ in 0..2 {
                frame(&ctx, &mut app, vec![]);
            }
            assert_eq!(width(), 254.0);

            for (requested, expected) in [(340.0, 340.0), (100.0, 220.0), (800.0, 480.0)] {
                let start = egui::pos2(width(), 400.0);
                let output = frame(&ctx, &mut app, vec![egui::Event::PointerMoved(start)]);
                assert!(matches!(
                    output.platform_output.cursor_icon,
                    egui::CursorIcon::ResizeHorizontal
                        | egui::CursorIcon::ResizeEast
                        | egui::CursorIcon::ResizeWest
                ));
                frame(
                    &ctx,
                    &mut app,
                    vec![egui::Event::PointerButton {
                        pos: start,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
                let end = egui::pos2(requested, start.y);
                frame(&ctx, &mut app, vec![egui::Event::PointerMoved(end)]);
                frame(&ctx, &mut app, vec![]);
                frame(
                    &ctx,
                    &mut app,
                    vec![egui::Event::PointerButton {
                        pos: end,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
                assert_eq!(width(), expected);
                frame(&ctx, &mut app, vec![]);
                assert_eq!(width(), expected, "Width survives subsequent frames");
            }
        }
    }

    #[test]
    fn sidebar_width_survives_restoring_egui_memory_and_switching_views() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let panel = egui::Id::new("sessions-v4");
        ctx.data_mut(|data| {
            data.insert_persisted(
                panel,
                egui::containers::panel::PanelState {
                    rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(360.0, 800.0)),
                },
            );
        });
        frame(&ctx, &mut app, vec![]);
        let memory = ctx.data(|data| serde_json::to_string(data).unwrap());
        let restored = egui::Context::default();
        restored.data_mut(|data| *data = serde_json::from_str(&memory).unwrap());
        app.projects = !app.projects;
        frame(&restored, &mut app, vec![]);
        assert_eq!(
            egui::containers::panel::PanelState::load(&restored, panel)
                .unwrap()
                .rect
                .width(),
            360.0
        );
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
    fn running_chat_title_updates_before_discovery_and_survives_background_navigation() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.demo = false;
        app.connected = true;
        app.new_chat = false;
        app.state = State {
            session_file: Some("/fixture/running.jsonl".into()),
            ..Default::default()
        };
        app.sessions = vec![Session {
            path: PathBuf::from("/fixture/running.jsonl"),
            cwd: app.prefs.cwd.clone(),
            project: app.prefs.cwd.clone(),
            title: "Untitled session".into(),
            created: None,
            modified: std::time::SystemTime::now(),
        }];
        let (client, events) = rpc::Client::stub_with_events();
        app.client = Some(client);
        let id = app.ensure_agent();
        events.send(rpc::Event::Record(json!({"type":"message_start","message":{"role":"user","content":"Fix the sidebar name"}}))).unwrap();
        events
            .send(rpc::Event::Record(json!({"type":"agent_start"})))
            .unwrap();
        events.send(rpc::Event::Record(json!({"type":"response","command":"get_state","success":true,"data":{"sessionFile":"/fixture/running.jsonl","isStreaming":true}}))).unwrap();
        app.poll_rpc();
        app.poll_other_agents();
        assert_eq!(app.session_title(&app.sessions[0]), "Fix the sidebar name");
        assert_eq!(app.agents.nodes[&id].name, "Fix the sidebar name");
        assert_eq!(app.agents.nodes[&id].status, "Working…");

        app.new_session();
        events
            .send(rpc::Event::Record(
                json!({"type":"session_info_changed","name":"Sidebar naming fix"}),
            ))
            .unwrap();
        app.poll_other_agents();
        assert_eq!(app.session_title(&app.sessions[0]), "Sidebar naming fix");
        assert_eq!(app.agents.nodes[&id].name, "Sidebar naming fix");
        app.projects = false;
        app.search = "Sidebar naming fix".into();
        for _ in 0..2 {
            frame(&ctx, &mut app, vec![]);
        }
        let output = frame(&ctx, &mut app, vec![]);
        fn contains(shape: &egui::Shape, label: &str) -> bool {
            match shape {
                egui::Shape::Text(text) => text.galley.job.text == label,
                egui::Shape::Vec(shapes) => shapes.iter().any(|shape| contains(shape, label)),
                _ => false,
            }
        }
        assert!(
            output
                .shapes
                .iter()
                .any(|shape| contains(&shape.shape, "Sidebar naming fix"))
        );
        assert!(
            !output
                .shapes
                .iter()
                .any(|shape| contains(&shape.shape, "Untitled session"))
        );

        app.prefs
            .chats
            .entry(app.sessions[0].path.clone())
            .or_default()
            .title = Some("My custom name".into());
        assert_eq!(app.session_title(&app.sessions[0]), "My custom name");
        app.shutdown_agents();
    }

    fn label_rect(output: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
        fn find(shape: &egui::Shape, label: &str) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Text(t) if t.galley.job.text == label => {
                    Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, label)),
                _ => None,
            }
        }
        output.shapes.iter().find_map(|s| find(&s.shape, label))
    }
    fn controls_frame(
        ctx: &egui::Context,
        app: &mut Pui,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, [egui::Rect; 2]) {
        controls_frame_sized(ctx, app, events, egui::vec2(1000.0, 800.0))
    }
    fn controls_frame_sized(
        ctx: &egui::Context,
        app: &mut Pui,
        events: Vec<egui::Event>,
        size: Vec2,
    ) -> (egui::FullOutput, [egui::Rect; 2]) {
        let mut rects = [egui::Rect::NOTHING; 2];
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ctx| {
                app.search_inputs(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(230.0_f32.min(ui.available_width()), 32.0),
                        Layout::right_to_left(Align::Center),
                        |ui| rects = app.session_list_controls(ui).map(|r| r.rect),
                    );
                });
                app.spotlight(ctx);
            },
        );
        (output, rects)
    }
    fn click_control(ctx: &egui::Context, app: &mut Pui, index: usize) {
        let (_, rects) = controls_frame(ctx, app, vec![]);
        click_controls_at(ctx, app, rects[index].center());
    }
    fn click_controls_at(ctx: &egui::Context, app: &mut Pui, pos: egui::Pos2) {
        for pressed in [true, false] {
            controls_frame(
                ctx,
                app,
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
    }
    fn click_label(ctx: &egui::Context, app: &mut Pui, label: &str) {
        let (output, _) = controls_frame(ctx, app, vec![]);
        let pos = label_rect(&output, label)
            .unwrap_or_else(|| panic!("Missing {label}"))
            .center();
        click_controls_at(ctx, app, pos);
    }
    #[test]
    fn combined_view_menu_is_right_anchored_and_does_not_execute_actions() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.draft = "Keep my draft".into();
        for _ in 0..3 {
            controls_frame(&ctx, &mut app, vec![]);
        }
        let (_, controls) = controls_frame(&ctx, &mut app, vec![]);
        click_control(&ctx, &mut app, 0);
        let (output, _) = controls_frame(&ctx, &mut app, vec![]);
        let group = label_rect(&output, "Group byFolder").expect("group and selected value");
        assert!(
            group.left() >= controls[0].right(),
            "Menu must open beside—not cover—the arrows: group={group:?}, controls={controls:?}"
        );
        for label in [
            "Sort byLast updated",
            "Show",
            "New section",
            "Reset to default",
            "View all",
        ] {
            assert!(label_rect(&output, label).is_some(), "Missing {label}");
        }
        click_label(&ctx, &mut app, "View all");
        assert!(app.prefs.sidebar_view.view_all);
        assert_eq!(app.draft, "Keep my draft");
        controls_frame(&ctx, &mut app, vec![key(egui::Key::Escape)]);
        assert!(!egui::Popup::is_any_open(&ctx));
    }

    #[test]
    fn search_icon_and_shortcut_reveal_focus_and_close_spotlight() {
        for theme in [theme::Theme::Light, theme::Theme::Dark] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.prefs.theme = theme;
            app.prefs.color_styles.apply(&ctx, theme);
            for _ in 0..3 {
                controls_frame(&ctx, &mut app, vec![]);
            }
            assert!(
                label_rect(&controls_frame(&ctx, &mut app, vec![]).0, "Search sessions").is_none()
            );
            click_control(&ctx, &mut app, 1);
            assert!(app.search_open && !app.focus_search);
            let (output, _) = controls_frame(&ctx, &mut app, vec![]);
            assert!(label_rect(&output, "Search everything…").is_some());
            controls_frame(&ctx, &mut app, vec![egui::Event::Text("sidebar".into())]);
            assert_eq!(app.search, "sidebar");
            controls_frame(&ctx, &mut app, vec![key(egui::Key::Escape)]);
            assert!(!app.search_open && app.search.is_empty());
            assert!(
                label_rect(&controls_frame(&ctx, &mut app, vec![]).0, "Search sessions").is_none()
            );

            // Native Search and the keyboard shortcut both request focus this way.
            app.focus_search = true;
            app.prefs.hide_sidebar = false;
            controls_frame(&ctx, &mut app, vec![]);
            assert!(app.search_open && !app.focus_search);
            controls_frame(&ctx, &mut app, vec![egui::Event::Text("shortcut".into())]);
            assert_eq!(app.search, "shortcut");
            click_label(&ctx, &mut app, "Close");
            assert!(!app.search_open && app.search.is_empty());
        }
    }

    #[test]
    fn list_controls_and_search_fit_narrow_widths_in_both_themes() {
        for theme in [theme::Theme::Light, theme::Theme::Dark] {
            for zoom in [0.8, 1.0, 1.4] {
                let ctx = egui::Context::default();
                let mut app = app(&ctx);
                app.prefs.color_styles.apply(&ctx, theme);
                ctx.set_zoom_factor(zoom);
                let size = egui::vec2(220.0, 600.0);
                for _ in 0..3 {
                    controls_frame_sized(&ctx, &mut app, vec![], size);
                }
                let (_, controls) = controls_frame_sized(&ctx, &mut app, vec![], size);
                assert!(
                    controls
                        .iter()
                        .all(|rect| rect.left() >= 0.0 && rect.right() <= 220.0),
                    "{controls:?} at zoom {zoom}"
                );
                for (index, heading, last) in [(0, "Group byFolder", "View all")] {
                    let pos = controls[index].center();
                    for pressed in [true, false] {
                        controls_frame_sized(
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
                            size,
                        );
                    }
                    for _ in 0..2 {
                        controls_frame_sized(&ctx, &mut app, vec![], size);
                    }
                    let (output, _) = controls_frame_sized(&ctx, &mut app, vec![], size);
                    for label in [heading, last] {
                        let rect = label_rect(&output, label).unwrap();
                        assert!(
                            rect.left() >= 0.0 && rect.right() <= 220.0,
                            "{label} at zoom {zoom}"
                        );
                    }
                    controls_frame_sized(&ctx, &mut app, vec![key(egui::Key::Escape)], size);
                    assert!(!egui::Popup::is_any_open(&ctx));
                }
            }
        }
    }

    #[test]
    fn search_escape_does_not_stop_a_run_or_leave_the_board() {
        for board in [false, true] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.state.busy = true;
            let (client, _events, commands) = rpc::Client::stub_with_records();
            app.client = Some(client);
            app.kanban.open = board;
            app.focus_search = true;
            app.focus_composer = false;
            let mut window = eframe::Frame::_new_kittest();
            let mut full_frame = |app: &mut Pui, events| {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1500.0, 950.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(app, ctx, &mut window),
                );
            };
            for _ in 0..3 {
                full_frame(&mut app, vec![]);
            }
            full_frame(&mut app, vec![egui::Event::Text("sidebar".into())]);
            assert_eq!(app.search, "sidebar");
            let _ = commands.try_iter().collect::<Vec<_>>();
            full_frame(&mut app, vec![key(egui::Key::Escape)]);
            assert!(!app.search_open && app.search.is_empty());
            assert!(app.state.busy);
            assert_eq!(app.kanban.open, board);
            assert!(
                commands
                    .try_iter()
                    .all(|command| command["type"] != "abort")
            );
        }
    }

    fn sidebar_click(
        ctx: &egui::Context,
        app: &mut Pui,
        pos: egui::Pos2,
        button: egui::PointerButton,
    ) {
        for pressed in [true, false] {
            frame(
                ctx,
                app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    #[test]
    fn hidden_chats_are_shown_and_restored_individually_from_their_menu() {
        for projects in [false, true] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.projects = projects;
            app.sessions.truncate(2);
            app.sessions[0].title = "Hidden first".into();
            app.sessions[1].title = "Hidden second".into();
            app.state.session_file = None;
            app.new_chat = true;
            for session in &app.sessions {
                app.prefs
                    .chats
                    .entry(session.path.clone())
                    .or_default()
                    .hidden = true;
            }
            frame(&ctx, &mut app, vec![]);
            let output = frame(&ctx, &mut app, vec![]);
            assert!(label_rect(&output, "Hidden first").is_none());
            assert!(label_rect(&output, "Restore hidden chats").is_none());
            app.show_hidden = true;
            for _ in 0..2 {
                frame(&ctx, &mut app, vec![]);
            }
            let output = frame(&ctx, &mut app, vec![]);
            let pos = label_rect(&output, "Hidden first").unwrap().center();
            assert!(label_rect(&output, "Hidden second").is_some());
            sidebar_click(&ctx, &mut app, pos, egui::PointerButton::Secondary);
            let output = frame(&ctx, &mut app, vec![]);
            let restore = label_rect(&output, "Restore chat").unwrap().center();
            sidebar_click(&ctx, &mut app, restore, egui::PointerButton::Primary);
            assert!(!app.prefs.chats[&app.sessions[0].path].hidden);
            assert!(app.prefs.chats[&app.sessions[1].path].hidden);
            let output = frame(&ctx, &mut app, vec![]);
            assert!(label_rect(&output, "Hidden first").is_none());
            app.show_hidden = false;
            frame(&ctx, &mut app, vec![]);
            assert!(label_rect(&frame(&ctx, &mut app, vec![]), "Hidden first").is_some());
        }
    }
    #[test]
    fn pins_have_a_dedicated_section_above_groups_and_survive_filters_until_unpinned() {
        for projects in [false, true] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.projects = projects;
            app.sessions.truncate(2);
            app.sessions[0].title = "Permanent favorite".into();
            app.sessions[1].title = "Ordinary chat".into();
            app.sessions[0].project = PathBuf::from("/fixture/favorite-project");
            app.sessions[1].project = PathBuf::from("/fixture/ordinary-project");
            app.state.session_file = None;
            app.new_chat = true;
            let path = app.sessions[0].path.clone();
            app.prefs.chats.entry(path.clone()).or_default().pinned = true;
            for _ in 0..2 {
                frame(&ctx, &mut app, vec![]);
            }
            let output = frame(&ctx, &mut app, vec![]);
            let pinned = label_rect(&output, "PINNED").unwrap();
            let favorite = label_rect(&output, "Permanent favorite").unwrap();
            let ordinary = label_rect(&output, "Ordinary chat").unwrap();
            assert!(pinned.bottom() < favorite.top() && favorite.bottom() < ordinary.top());
            if projects {
                assert!(
                    label_rect(&output, "favorite-project").is_none(),
                    "Pinned chats must not be duplicated inside project groups"
                );
                assert!(favorite.bottom() < label_rect(&output, "ordinary-project").unwrap().top());
            }
            app.search = "no matches".into();
            app.show_archived = true;
            app.show_hidden = true;
            app.prefs.chats.get_mut(&path).unwrap().hidden = true;
            frame(&ctx, &mut app, vec![]);
            let output = frame(&ctx, &mut app, vec![]);
            let favorite = label_rect(&output, "Permanent favorite").unwrap().center();
            assert!(label_rect(&output, "PINNED").is_some());
            sidebar_click(&ctx, &mut app, favorite, egui::PointerButton::Secondary);
            let output = frame(&ctx, &mut app, vec![]);
            let unpin = label_rect(&output, "Unpin").unwrap().center();
            sidebar_click(&ctx, &mut app, unpin, egui::PointerButton::Primary);
            assert!(!app.prefs.chats[&path].pinned);
            let output = frame(&ctx, &mut app, vec![]);
            assert!(label_rect(&output, "PINNED").is_none());
            assert!(
                label_rect(&output, "Permanent favorite").is_some(),
                "Unpinned hidden chats remain in the explicit Hidden chats view; Spotlight no longer filters the sidebar"
            );
        }
    }
    #[test]
    fn sort_applies_to_visible_rows_in_recent_and_project_views() {
        for projects in [false, true] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.projects = projects;
            let template = app.sessions[0].clone();
            app.sessions = [("Zebra", 30, 10), ("Alpha", 10, 30), ("Beta", 20, 20)]
                .into_iter()
                .enumerate()
                .map(|(i, (title, created, modified))| Session {
                    path: PathBuf::from(format!("/fixture/{i}.jsonl")),
                    title: title.into(),
                    created: Some(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(created)),
                    modified: std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(modified),
                    ..template.clone()
                })
                .collect();
            for (sort, expected) in [
                (sidebar_order::Sort::Name, ["Alpha", "Beta", "Zebra"]),
                (sidebar_order::Sort::Activity, ["Alpha", "Beta", "Zebra"]),
                (sidebar_order::Sort::Created, ["Zebra", "Beta", "Alpha"]),
            ] {
                for reverse in [false, true] {
                    app.prefs.session_order = sidebar_order::Options { sort, reverse };
                    frame(&ctx, &mut app, vec![]);
                    let output = frame(&ctx, &mut app, vec![]);
                    let mut positions =
                        expected.map(|s| label_rect(&output, s).unwrap().center().y);
                    if reverse {
                        positions.reverse();
                    }
                    assert!(positions[0] < positions[1] && positions[1] < positions[2]);
                }
            }
        }
    }
    #[test]
    fn date_separators_follow_sort_and_do_not_split_pins_projects_or_name_sort() {
        use chrono::{Days, Local, NaiveTime};
        let today = Local::now().date_naive();
        let local_noon = |date: chrono::NaiveDate| {
            date.and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap())
                .and_local_timezone(Local)
                .earliest()
                .unwrap()
                .into()
        };
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            for zoom in [0.8, 1.4] {
                let ctx = egui::Context::default();
                let mut app = app(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                app.projects = false;
                app.state.session_file = None;
                app.new_chat = true;
                let template = app.sessions[0].clone();
                app.sessions = [
                    ("Recent one", today),
                    ("Recent two", today),
                    ("Previous day", today - Days::new(1)),
                    ("Old conversation", today - Days::new(400)),
                ]
                .into_iter()
                .enumerate()
                .map(|(i, (title, date))| Session {
                    path: format!("/fixture/date-{i}.jsonl").into(),
                    title: title.into(),
                    created: None,
                    modified: local_noon(date),
                    ..template.clone()
                })
                .collect();
                for reverse in [false, true] {
                    app.prefs.session_order.reverse = reverse;
                    for _ in 0..3 {
                        frame(&ctx, &mut app, vec![]);
                    }
                    let output = frame(&ctx, &mut app, vec![]);
                    let mut headings = ["Today", "Yesterday", "Older"]
                        .map(|label| label_rect(&output, label).unwrap());
                    if reverse {
                        headings.reverse();
                    }
                    assert!(headings[0].bottom() < headings[1].top());
                    assert!(headings[1].bottom() < headings[2].top());
                    assert!(headings.iter().all(|rect| rect.right() <= 254.0));
                    let today_heading = label_rect(&output, "Today").unwrap();
                    for title in ["Recent one", "Recent two"] {
                        assert!(today_heading.bottom() < label_rect(&output, title).unwrap().top());
                    }
                    assert!(label_rect(&output, "This week").is_none());
                    assert!(label_rect(&output, "Unknown date").is_none());
                }
                app.prefs.session_order.reverse = false;
                app.search = "Recent".into();
                frame(&ctx, &mut app, vec![]);
                let output = frame(&ctx, &mut app, vec![]);
                assert!(label_rect(&output, "Today").is_some());
                assert!(label_rect(&output, "Yesterday").is_some());
                assert!(label_rect(&output, "Older").is_some());
                app.search.clear();

                // Creation sorting uses creation time, never activity fallback.
                app.prefs.session_order.sort = sidebar_order::Sort::Created;
                app.sessions[0].created = Some(local_noon(today - Days::new(1)));
                frame(&ctx, &mut app, vec![]);
                let output = frame(&ctx, &mut app, vec![]);
                assert!(label_rect(&output, "Today").is_none());
                assert!(label_rect(&output, "Yesterday").is_some());
                assert!(label_rect(&output, "Unknown date").is_some());

                app.prefs.session_order.sort = sidebar_order::Sort::Activity;
                for session in &app.sessions[..2] {
                    app.prefs
                        .chats
                        .entry(session.path.clone())
                        .or_default()
                        .pinned = true;
                }
                frame(&ctx, &mut app, vec![]);
                let output = frame(&ctx, &mut app, vec![]);
                assert!(label_rect(&output, "PINNED").is_some());
                assert!(label_rect(&output, "Today").is_none());
                assert!(label_rect(&output, "Yesterday").is_some());

                for projects in [false, true] {
                    app.projects = projects;
                    app.prefs.session_order.sort = if projects {
                        sidebar_order::Sort::Activity
                    } else {
                        sidebar_order::Sort::Name
                    };
                    frame(&ctx, &mut app, vec![]);
                    let output = frame(&ctx, &mut app, vec![]);
                    for label in ["Today", "Yesterday", "Older", "Unknown date"] {
                        assert!(label_rect(&output, label).is_none());
                    }
                }
            }
        }
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
            assert_eq!(labels.iter().filter(|(s, _)| s == "Pui").count(), 1);
            assert!(!labels.iter().any(|(s, _)| s == "New task"));
            let title = labels.iter().find(|(s, _)| s == "Agents board").unwrap().1;
            let navigation = ["Agents board", "Scheduled tasks", "Settings"]
                .map(|label| labels.iter().find(|(s, _)| s == label).unwrap().1);
            assert!(!labels.iter().any(|(s, _)| s == "Search sessions"));
            let heading = labels.iter().find(|(s, _)| s == "SESSIONS").unwrap().1;
            assert!(!labels.iter().any(|(s, _)| matches!(
                s.as_str(),
                "View" | "Filter" | "Group" | "Recent" | "Projects"
            )));
            assert!(navigation.iter().all(|rect| rect.left() < 70.0));
            if cfg!(target_os = "macos") {
                assert!(
                    title.top() * zoom >= 30.0,
                    "Native traffic-light overlap at scale {zoom}"
                );
            }
            assert_eq!(title, navigation[0]);
            assert!(navigation[0].bottom() < navigation[1].top());
            assert!(navigation[1].bottom() < navigation[2].top());
            assert!(navigation[1].center().y - navigation[0].center().y <= 31.0);
            assert!(navigation[2].center().y - navigation[1].center().y <= 31.0);
            assert!(navigation[2].bottom() < heading.top());
            let (controls, [view, search]) = controls_frame(&ctx, &mut app, vec![]);
            assert!(search.right() <= view.left());
            assert!((search.center().y - view.center().y).abs() <= 1.0 / zoom);
            assert_eq!(search.size(), view.size());
            assert!(
                controls
                    .shapes
                    .iter()
                    .all(|s| !matches!(&s.shape, egui::Shape::Text(_))),
                "Buttons must be icons only"
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
                app.chat_row(ui, 0, None, None);
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
                    app.chat_row(ui, 0, None, None);
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
                        app.chat_row(ui, 0, None, None);
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
                            app.chat_row(ui, index, None, None);
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
                            if let Some(Action::Open(i)) = app.chat_row(ui, index, None, None) {
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
            created: None,
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
        // Expanded projects may extend below the viewport; keep collapse reachable.
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(egui::pos2(150.0, 600.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -400.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..8 {
            frame(&ctx, &mut app, vec![]);
        }
        text.clear();
        for s in frame(&ctx, &mut app, vec![]).shapes {
            collect(&s.shape, &mut text);
        }
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
