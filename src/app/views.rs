use super::*;

impl Pui {
    pub(super) fn top_bar(&mut self, ctx: &egui::Context) {
        let colors = crate::theme::palette(ctx);
        let mut notifications_button = None;
        egui::TopBottomPanel::top("workspace-bar-v2")
            .exact_height(if cfg!(target_os = "macos") && self.prefs.hide_sidebar {
                64.0
            } else {
                54.0
            })
            .frame(
                egui::Frame::new()
                    .fill(colors.bg)
                    .inner_margin(egui::Margin::symmetric(20, 0)),
            )
            .show(ctx, |ui| {
                if cfg!(target_os = "macos") && self.prefs.hide_sidebar {
                    ui.add_space(24.0 / ctx.zoom_factor());
                }
                ui.horizontal_centered(|ui| {
                    if icons::button(
                        ui,
                        Icon::Sidebar,
                        if self.prefs.hide_sidebar {
                            "Show sessions · ⌘B"
                        } else {
                            "Hide sessions · ⌘B"
                        },
                    )
                    .clicked()
                    {
                        self.prefs.hide_sidebar = !self.prefs.hide_sidebar;
                    }
                    if self.prefs.hide_sidebar {
                        self.attention_button(ui);
                    }
                    let narrow = ui.available_width() < 520.0;
                    let width =
                        (ui.available_width() - if narrow { 68.0 } else { 266.0 }).max(40.0);
                    self.navigation_header(ui, width);
                    if narrow {
                        self.compact_view_menu(ui);
                        return;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.loading_chat {
                            ui.label(
                                RichText::new(if self.preview_partial {
                                    "Loading full chat…"
                                } else {
                                    "Connecting…"
                                })
                                .size(10.0)
                                .color(colors.dim),
                            );
                        }
                        notifications_button = Some(self.notifications_button(ui));
                        let response =
                            icons::button(ui, Icon::Terminal, "Terminal · current project");
                        #[cfg(test)]
                        ctx.data_mut(|data| {
                            data.insert_temp(
                                egui::Id::new("terminal-toolbar-button"),
                                response.rect,
                            )
                        });
                        if self.terminals.is_open() {
                            ui.painter().rect_stroke(
                                response.rect,
                                7,
                                Stroke::new(1.5, colors.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if response.clicked() {
                            self.kanban.open = false;
                            self.automation.open = false;
                            self.terminals.toggle(ctx, &self.prefs.cwd, self.demo);
                            self.focus_composer = !self.terminals.is_open();
                        }
                        let response = icons::button(ui, Icon::Globe, "Browser · Chromium");
                        #[cfg(test)]
                        ctx.data_mut(|data| {
                            data.insert_temp(egui::Id::new("browser-toolbar-button"), response.rect)
                        });
                        if self.files.browser_open() {
                            ui.painter().rect_stroke(
                                response.rect,
                                7,
                                Stroke::new(1.5, colors.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if response.clicked() {
                            self.kanban.open = false;
                            self.automation.open = false;
                            self.diff_rx = None; // explicit navigation abandons the pending review open
                            self.files.toggle_browser(ctx, &self.prefs.cwd);
                            self.inspector_panel = false;
                            self.background_tasks.open = false;
                        }
                        let response = icons::button(ui, Icon::Folder, "Browse workspace files");
                        #[cfg(test)]
                        ctx.data_mut(|data| {
                            data.insert_temp(egui::Id::new("files-toolbar-button"), response.rect)
                        });
                        if self.files.files_open() {
                            ui.painter().rect_stroke(
                                response.rect,
                                7,
                                Stroke::new(1.5, colors.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if response.clicked() {
                            self.kanban.open = false;
                            self.automation.open = false;
                            self.diff_rx = None;
                            self.files.toggle(ctx, &self.prefs.cwd);
                            self.inspector_panel = false;
                            self.background_tasks.open = false;
                        }
                        let response = icons::button(
                            ui,
                            Icon::File,
                            "Changes · current Git and recorded edits",
                        );
                        if self.inspector_panel {
                            ui.painter().rect_filled(response.rect, 7, colors.accent_bg);
                            icons::draw(ui, response.rect, Icon::File, colors.text);
                        }
                        if response.clicked() {
                            self.kanban.open = false;
                            self.automation.open = false;
                            self.files.close();
                            self.background_tasks.open = false;
                            self.inspector_panel = !self.inspector_panel;
                            self.prefs.hide_inspector = false;
                            if self.inspector_panel {
                                self.changes.invalidate();
                                self.refresh_workspace();
                            }
                        }
                        let (bg_running, bg_finished) = self.active_background_counts();
                        let bg_tip = if bg_running > 0 {
                            format!("Background tasks · {bg_running} running")
                        } else if bg_finished > 0 {
                            format!("Background tasks · {bg_finished} finished")
                        } else {
                            "Background tasks".into()
                        };
                        let response = icons::button(ui, Icon::Agents, &bg_tip);
                        if self.background_tasks.open {
                            ui.painter().rect_filled(response.rect, 7, colors.accent_bg);
                            icons::draw(ui, response.rect, Icon::Agents, colors.text);
                        } else if bg_running > 0 {
                            ui.painter().rect_stroke(
                                response.rect,
                                7,
                                Stroke::new(1.5, colors.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if response.clicked() {
                            self.kanban.open = false;
                            self.automation.open = false;
                            self.files.close();
                            self.inspector_panel = false;
                            self.background_tasks.open = !self.background_tasks.open;
                            if !self.background_tasks.open {
                                self.background_tasks.expanded = false;
                            }
                        }
                        let active = !self.prefs.hide_inspector
                            && !self.inspector_panel
                            && !self.background_tasks.open
                            && !self.files.is_open();
                        let response = icons::button(ui, Icon::Details, "Session details · ⌘I");
                        if active {
                            ui.painter().rect_stroke(
                                response.rect,
                                7,
                                Stroke::new(1.0, colors.hover),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if response.clicked() {
                            self.kanban.open = false;
                            self.automation.open = false;
                            if self.files.is_open() {
                                self.files.close();
                                self.prefs.hide_inspector = false;
                            } else if self.inspector_panel {
                                self.inspector_panel = false;
                                self.prefs.hide_inspector = false;
                            } else if self.background_tasks.open {
                                self.background_tasks.open = false;
                                self.background_tasks.expanded = false;
                                self.prefs.hide_inspector = false;
                            } else {
                                self.prefs.hide_inspector = !self.prefs.hide_inspector;
                            }
                        }
                        response.context_menu(|ui| self.prefs.cards.menu(ui));
                    });
                });
            });
        if let Some(button) = notifications_button {
            self.notifications_popup(&button);
        }
    }

    pub(super) fn inspector(&mut self, ctx: &egui::Context) {
        let colors = crate::theme::palette(ctx);
        egui::SidePanel::right("floating-inspector-v2")
            .default_width(300.0)
            .width_range(220.0..=(ctx.available_rect().width() - 320.0).clamp(220.0, 600.0))
            .resizable(true)
            .show_separator_line(true)
            .frame(
                egui::Frame::new()
                    .fill(colors.bg)
                    .inner_margin(egui::Margin {
                        left: 14,
                        right: 10,
                        top: 10,
                        bottom: 12,
                    }),
            )
            .show(ctx, |ui| self.inspector_contents(ui));
    }

    pub(super) fn inspector_contents(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let colors = theme::palette(&ctx);
        egui::ScrollArea::vertical()
                    .id_salt("floating-cards")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Keep floating scrollbar handles clear of section contents.
                        ui.set_width((ui.available_width() - 12.0).max(0.0));
                        let task = self
                            .tasks
                            .open
                            .then(|| self.tasks.current().cloned())
                            .flatten();
                        let status = task
                            .as_ref()
                            .map(|t| t.phase.label())
                            .unwrap_or_else(|| self.run_label().0);
                        let task_outputs = task
                            .as_ref()
                            .and_then(|t| t.latest())
                            .map(|a| a.changes.clone())
                            .unwrap_or_default();
                        let workflow_summary = self
                            .inspector_workflow()
                            .map(|t| t.phase.label())
                            .unwrap_or("")
                            .to_owned();
                        let connectors = load_mcp_connectors(&self.prefs.cwd, &self.state.diagnostics);
                        let (active_bg_count, finished_bg_count) = self.active_background_counts();
                        let summaries = [
                            status.to_owned(),
                            crate::context::usage_summary(&self.state),
                            if task.is_some() {
                                task_outputs.len()
                            } else {
                                self.outputs.len()
                            }
                            .to_string(),
                            if self.demo { "4" } else { "—" }.into(),
                            self.workspace.files.len().to_string(),
                            connectors.len().to_string(),
                            if active_bg_count > 0 {
                                format!("{active_bg_count} running")
                            } else if finished_bg_count > 0 {
                                format!("{finished_bg_count} finished")
                            } else {
                                "Idle".into()
                            },
                        ];
                        let mut layout = std::mem::take(&mut self.prefs.cards);
                        let original_hidden = layout.hidden.clone();
                        if task.is_some() {
                            layout.hidden.extend([
                                Card::Context,
                                Card::Connectors,
                                Card::Pulls,
                                Card::Changes,
                                Card::BackgroundTasks,
                            ]);
                        }
                        let workflow_id = self.inspector_workflow().map(|t| t.id.clone());
                        if workflow_id.is_none() {
                            layout.hidden.insert(Card::Workflow);
                        }
                        let header_action = workflow_id
                            .as_ref()
                            .map(|_| (Card::Workflow, "Open"))
                            .or_else(|| {
                                (active_bg_count > 0 || finished_bg_count > 0)
                                    .then_some((Card::BackgroundTasks, "Open"))
                            });
                        let action = inspector::stack(
                            ui,
                            &mut layout,
                            |card| {
                                if card == Card::Workflow {
                                    return workflow_summary.clone();
                                }
                                summaries[match card {
                                    Card::Workflow => unreachable!(),
                                    Card::Environment => 0,
                                    Card::Context => 1,
                                    Card::Outputs => 2,
                                    Card::Pulls => 3,
                                    Card::Changes => 4,
                                    Card::Connectors => 5,
                                    Card::BackgroundTasks => 6,
                                }]
                                .clone()
                            },
                            header_action,
                            crate::context::usage_fraction(&self.state),
                            |ui, card| match card {
                                Card::Workflow => self.task_workflow(ui),
                                Card::Environment => {
                                    if let Some(task) = &task {
                                        ui.label(
                                            RichText::new(if task.isolated {
                                                "Isolated workflow workspace"
                                            } else {
                                                "Workflow workspace"
                                            })
                                            .size(12.0),
                                        );
                                        inspector::detail(ui, "Status", task.phase.label());
                                        inspector::detail(
                                            ui,
                                            "Cost",
                                            &format!("$ {:.3} reported", task.cost()),
                                        );
                                        let cwd = task.workspace.as_ref().unwrap_or(&task.source);
                                        ui.add(
                                            egui::Label::new(theme::small(
                                                ui,
                                                cwd.display().to_string(),
                                            ))
                                            .wrap()
                                            .selectable(true),
                                        );
                                        ui.label(theme::small(
                                            ui,
                                            "Linked to your conversation. Workflow agents use their own permission-controlled sessions.",
                                        ));
                                        return;
                                    }
                                    let active_cloud = self.active_cloud_host();
                                    let cloud_meta = self.active_cloud_meta().cloned();
                                    ui.horizontal(|ui| {
                                        if active_cloud.is_some() {
                                            icons::inline(
                                                ui,
                                                Icon::Cloud,
                                                if self.connected || self.demo {
                                                    colors.accent
                                                } else if self.loading_chat {
                                                    colors.amber
                                                } else {
                                                    colors.dim
                                                },
                                            );
                                        } else {
                                            dot(
                                                ui,
                                                if self.connected || self.demo {
                                                    colors.green
                                                } else {
                                                    colors.dim
                                                },
                                            );
                                        }
                                        ui.vertical(|ui| {
                                            ui.spacing_mut().item_spacing.y = 2.0;
                                            let env_title = match active_cloud.as_ref() {
                                                Some(cfg) => format!("Cloud · {}", cfg.name),
                                                None => "Local computer".to_owned(),
                                            };
                                            let env_sub = if self.demo {
                                                "Design preview · no agent connected".to_owned()
                                            } else if let Some(cfg) = active_cloud.as_ref() {
                                                if self.connected {
                                                    format!(
                                                        "{}:{} · Cloud RPC connected",
                                                        cfg.host, cfg.port
                                                    )
                                                } else if self.loading_chat {
                                                    format!("Connecting to {}…", cfg.name)
                                                } else {
                                                    format!("{}:{} · Disconnected", cfg.host, cfg.port)
                                                }
                                            } else if self.connected {
                                                "Pi RPC connected".to_owned()
                                            } else {
                                                "Not connected".to_owned()
                                            };
                                            ui.label(
                                                RichText::new(env_title)
                                                    .size(12.0)
                                                    .strong()
                                                    .color(colors.text),
                                            );
                                            ui.label(
                                                RichText::new(env_sub)
                                                    .size(10.0)
                                                    .color(colors.muted),
                                            );
                                        });
                                    });
                                    ui.add_space(4.0);
                                    let repo_label = self
                                        .agents
                                        .nodes
                                        .get(&self.agents.active)
                                        .and_then(|n| n.cloud_repo.as_ref())
                                        .map(|r| r.full_name.clone())
                                        .or_else(|| {
                                            cloud_meta
                                                .as_ref()
                                                .map(|m| m.repo_name.clone())
                                                .filter(|s| !s.is_empty())
                                        })
                                        .unwrap_or_else(|| sessions::project_name(&self.prefs.cwd));
                                    inspector::detail(ui, "Status", status);
                                    if active_cloud.is_some() {
                                        inspector::detail(ui, "Mode", "Auto · Isolated worktree");
                                    }
                                    inspector::detail(ui, "Repository", &repo_label);
                                    if active_cloud.is_some() {
                                        let cloud_branch = cloud_meta
                                            .as_ref()
                                            .and_then(|m| m.branch.clone())
                                            .or_else(|| {
                                                self.state.session_file.as_deref().and_then(|p| {
                                                    // Fallback: extract `pui_agent/<slug>` from session folder name if metadata not yet polled.
                                                    p.split("pui_agent-").nth(1).and_then(|tail| {
                                                        let slug = tail.split("--").next()?;
                                                        (!slug.is_empty())
                                                            .then(|| format!("pui_agent/{slug}"))
                                                    })
                                                })
                                            });
                                        if let Some(branch) = cloud_branch {
                                            inspector::detail(ui, "Branch", &branch);
                                        }
                                        if let Some(cwd) = cloud_meta
                                            .as_ref()
                                            .map(|m| m.cwd.as_str())
                                            .filter(|s| !s.is_empty())
                                        {
                                            let short_wt =
                                                crate::remote::cloud::compact_cloud_path(cwd);
                                            inspector::detail(ui, "Worktree", &short_wt);
                                        }
                                    } else if !self.workspace.branch.is_empty() {
                                        inspector::detail(ui, "Branch", &self.workspace.branch);
                                    }
                                    if let Some(timestamp) = self
                                        .state
                                        .messages
                                        .iter()
                                        .map(|m| m.timestamp)
                                        .find(|t| *t > 0)
                                    {
                                        inspector::detail(
                                            ui,
                                            "Started",
                                            &crate::transcript::time_label(timestamp),
                                        );
                                    }
                                    inspector::detail(
                                        ui,
                                        "Cost",
                                        &format!("$ {:.2}", self.state.cost),
                                    );
                                    if !self.demo
                                        && self.client.is_none()
                                        && ui.button("Connect to Pi").clicked()
                                        && !self.resume_agent()
                                    {
                                        self.connect(
                                            &ctx,
                                            self.state.session_file.as_ref().map(PathBuf::from),
                                        );
                                    }
                                }
                                Card::Context => {
                                    crate::context::show_with_cwd(
                                        ui,
                                        &self.state,
                                        &self.prefs.cwd,
                                        self.demo,
                                    );
                                    if !self.demo
                                        && ui
                                            .add_enabled(
                                                self.connected && self.state.ready(),
                                                egui::Button::new(theme::small(
                                                    ui,
                                                    "Compact context",
                                                ))
                                                .frame(false),
                                            )
                                            .clicked()
                                    {
                                        self.send(protocol::command("compact"));
                                    }
                                }
                                Card::Connectors => {
                                    self.connectors_card(ui, &connectors);
                                }
                                Card::Outputs => {
                                    let outputs = if task.is_some() {
                                        &task_outputs
                                    } else {
                                        &self.outputs
                                    };
                                    if outputs.is_empty() {
                                        ui.label(theme::small(ui, "No recorded file changes."));
                                    }
                                    let mut selected_diff = None;
                                    let mut selected_preview = None;
                                    for (index, file) in outputs.iter().enumerate() {
                                        let previewable = is_previewable_output(&file.path);
                                        let resp = output_row(
                                            ui,
                                            &file.path,
                                            &file.added.to_string(),
                                            &file.removed.to_string(),
                                        );
                                        if resp.clicked() {
                                            if previewable && task.is_none() {
                                                selected_preview = Some(file.path.clone());
                                            } else {
                                                selected_diff = Some(index);
                                            }
                                        }
                                        if task.is_none() {
                                            resp.context_menu(|ui| {
                                                theme::menu_controls(ui);
                                                if ui.button("Open preview").clicked() {
                                                    selected_preview = Some(file.path.clone());
                                                    ui.close();
                                                }
                                                if ui.button("View diff").clicked() {
                                                    selected_diff = Some(index);
                                                    ui.close();
                                                }
                                            });
                                        }
                                    }
                                    if let Some(path) = selected_preview {
                                        if let Err(err) =
                                            self.files.open_target(&ctx, &self.prefs.cwd, &path)
                                        {
                                            self.state.notice = Some(err);
                                        } else {
                                            self.inspector_panel = false;
                                            self.files.hide_web();
                                        }
                                    } else if let Some(index) = selected_diff {
                                        if let Some(task) = &task {
                                            self.open_work_item_tab(task.id.clone(), tasks::work_item::Tab::Changes);
                                        } else {
                                            let file = &self.outputs[index];
                                            let document = (file.path.clone(), file.diff.clone()).into();
                                            self.open_diff_review(document, true);
                                        }
                                    }
                                }
                                Card::Pulls => {
                                    self.pulls.show(ui);
                                }
                                Card::Changes => self.working_tree(ui),
                                Card::BackgroundTasks => self.background_tasks_card(ui),
                            },
                        );
                        if task.is_some() {
                            for card in [
                                Card::Context,
                                Card::Connectors,
                                Card::Pulls,
                                Card::Changes,
                                Card::BackgroundTasks,
                            ] {
                                if !original_hidden.contains(&card) {
                                    layout.hidden.remove(&card);
                                }
                            }
                        }
                        if workflow_id.is_none() && !original_hidden.contains(&Card::Workflow) {
                            layout.hidden.remove(&Card::Workflow);
                        }
                        self.prefs.cards = layout;
                        if action == Some(Card::Workflow) {
                            if let Some(id) = workflow_id {
                                self.open_task_from_board(id, None);
                            }
                        } else if action == Some(Card::BackgroundTasks) {
                            self.open_background_tasks_panel(None, None);
                        } else if action == Some(Card::Connectors) {
                            self.settings_view.select("Resources");
                            self.settings_open = true;
                        }
                    });
    }

    fn working_tree(&mut self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        let (total_added, total_removed) =
            self.workspace
                .files
                .iter()
                .fold((0usize, 0usize), |acc, f| {
                    (
                        acc.0 + f.added.parse::<usize>().unwrap_or(0),
                        acc.1 + f.removed.parse::<usize>().unwrap_or(0),
                    )
                });
        if self.workspace.is_git {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if total_added > 0 || total_removed > 0 {
                    ui.label(
                        RichText::new(format!("+{total_added}"))
                            .size(11.0)
                            .strong()
                            .color(colors.green),
                    );
                    ui.label(
                        RichText::new(format!("-{total_removed}"))
                            .size(11.0)
                            .strong()
                            .color(colors.red),
                    );
                } else {
                    ui.label(RichText::new("Clean").size(11.0).color(colors.dim));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .small_button("Create PR")
                        .on_hover_text("Open GitHub PR creation in browser (`gh pr create --web`)")
                        .clicked()
                    {
                        let cwd = self.prefs.cwd.clone();
                        std::thread::spawn(move || {
                            let _ = std::process::Command::new("gh")
                                .args(["pr", "create", "--web"])
                                .current_dir(&cwd)
                                .status();
                        });
                        self.state.notice = Some("Opening PR creation in browser…".into());
                    }
                    if ui
                        .small_button("Commit")
                        .on_hover_text("Open Changes panel to stage and commit")
                        .clicked()
                    {
                        self.changes.set_expanded(true);
                    }
                });
            });
            ui.add_space(4.0);
        }
        ui.label(
            RichText::new("Working tree vs HEAD, not recorded session outputs.")
                .size(10.0)
                .color(colors.dim),
        );
        let mut selected = None;
        for (index, file) in self.workspace.files.iter().enumerate() {
            if output_row(ui, &file.path, &file.added, &file.removed).clicked() {
                selected = Some(index);
            }
        }
        if self.workspace.files.is_empty() {
            ui.label(theme::small(
                ui,
                if let Some(error) = &self.workspace.error {
                    error
                } else if self.workspace.is_git {
                    "Working tree is clean."
                } else {
                    "Not a Git repository."
                },
            ));
        }
        if let Some(index) = selected {
            let file = self.workspace.files[index].clone();
            self.diff_rx = Some(changes::git_files::start(&self.ctx, &self.workspace, file));
        }
    }

    fn connectors_card(&mut self, ui: &mut egui::Ui, connectors: &[McpConnector]) {
        let colors = crate::theme::palette(ui.ctx());
        if let Some(cfg) = self.active_cloud_host() {
            ui.horizontal(|ui| {
                icons::inline(ui, Icon::Cloud, colors.accent);
                ui.label(
                    RichText::new(format!("Managed on {}", cfg.name))
                        .size(11.5)
                        .color(colors.text),
                );
            });
            ui.add_space(3.0);
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "Cloud agents use synced MCP servers (such as playwright-research) instead of local Mac connectors.",
                    )
                    .size(10.5)
                    .color(colors.muted),
                )
                .wrap(),
            );
            return;
        }
        if connectors.is_empty() {
            ui.label(theme::small(ui, "No MCP connectors configured."));
        } else {
            let blocked = self.connector_reconnect_blocked();
            let mut reconnect = None;
            for conn in connectors {
                ui.push_id(&conn.name, |ui| {
                    // Allocate the action first, from the shared right edge. A
                    // long connector name may elide but can never move Reconnect.
                    let width = ui.available_width();
                    let height = ui.spacing().interact_size.y.max(28.0);
                    let (row, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
                    let action_width = 66.0_f32.min(width * 0.45);
                    let action_rect = egui::Rect::from_min_max(
                        egui::pos2(row.right() - action_width, row.top()), row.max);
                    let reconnected = self.resource_reload.connector_reconnected(&conn.name);
                    let pending = self.resource_reload.connector_connecting(&conn.name);
                    let action_label = if pending { "Connecting…" } else { "Reconnect" };
                    let action = ui.scope_builder(egui::UiBuilder::new().max_rect(action_rect).layout(Layout::right_to_left(Align::Center)), |ui| {
                        ui.add_enabled(blocked.is_none(),
                            egui::Button::new(RichText::new(action_label).size(10.5)
                                .color(if conn.error.is_some() && !reconnected { colors.amber } else { colors.muted }))
                                .frame(false))
                    }).inner.on_hover_text(blocked.unwrap_or("Reconnect only this connector without restarting the chat. Active requests using it may fail; configuration is not reloaded."))
                        .on_disabled_hover_text(blocked.unwrap_or("Reconnect unavailable."));
                    if action.clicked() { reconnect = Some(conn.name.clone()); }
                    let dot_center = row.left_center() + egui::vec2(4.0, 0.0);
                    ui.painter().circle_filled(dot_center, 3.0,
                        if pending || (conn.error.is_some() && !reconnected) { colors.amber } else if self.connected { colors.green } else { colors.dim });
                    let name_rect = egui::Rect::from_min_max(
                        egui::pos2(row.left() + 16.0, row.top()),
                        egui::pos2((action_rect.left() - 8.0).max(row.left() + 16.0), row.bottom()));
                    let mut job = egui::text::LayoutJob::simple_singleline(conn.name.clone(), FontId::proportional(11.5), colors.text);
                    job.wrap.max_width = name_rect.width();
                    job.wrap.max_rows = 1;
                    job.wrap.break_anywhere = true;
                    let galley = ui.painter().layout_job(job);
                    let elided = galley.elided;
                    ui.painter().with_clip_rect(name_rect).galley(
                        name_rect.left_center() - egui::vec2(0.0, galley.size().y / 2.0), galley, colors.text);
                    let label = ui.interact(name_rect, ui.id().with("connector-name"), Sense::hover());
                    if let Some(error) = &conn.error && !reconnected { label.on_hover_text(error); }
                    else if elided { label.on_hover_text(&conn.name); }
                    #[cfg(test)]
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(egui::Id::new(("connector-row", conn.name.as_str())), (row, name_rect, action.rect, elided));
                    });
                });
            }
            if let Some(name) = reconnect {
                self.request_connector_reconnect(&name);
            }
            ui.label(theme::small(ui, "Reconnect affects one connector, not the chat. Use /reload when idle to load configuration changes."));
        }
    }

    pub(super) fn conversation(&mut self, ctx: &egui::Context) {
        let colors = crate::theme::palette(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(colors.bg))
            .show(ctx, |ui| {
                if let Some(cloud_cfg) = self.active_cloud_host() {
                    let cloud_meta = self.active_cloud_meta().cloned();
                    let repo_label = self
                        .agents
                        .nodes
                        .get(&self.agents.active)
                        .and_then(|n| n.cloud_repo.as_ref())
                        .map(|r| r.full_name.clone())
                        .or_else(|| {
                            cloud_meta
                                .as_ref()
                                .map(|m| m.repo_name.clone())
                                .filter(|s| !s.is_empty())
                        })
                        .unwrap_or_else(|| "workspace".to_owned());
                    let branch_label =
                        cloud_meta
                            .as_ref()
                            .and_then(|m| m.branch.clone())
                            .or_else(|| {
                                self.state.session_file.as_deref().and_then(|p| {
                                    p.split("pui_agent-").nth(1).and_then(|tail| {
                                        let slug = tail.split("--").next()?;
                                        (!slug.is_empty()).then(|| format!("pui_agent/{slug}"))
                                    })
                                })
                            });
                    egui::TopBottomPanel::top("cloud-chat-banner")
                        .show_separator_line(false)
                        .frame(
                            egui::Frame::new()
                                .fill(colors.surface)
                                .stroke(Stroke::new(1.0_f32, colors.border.gamma_multiply(0.6)))
                                .inner_margin(egui::Margin::symmetric(16, 6)),
                        )
                        .show_inside(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                icons::inline(ui, Icon::Cloud, colors.accent);
                                ui.label(
                                    RichText::new(format!("Cloud · {}", cloud_cfg.name))
                                        .size(11.5)
                                        .strong()
                                        .color(colors.text),
                                );
                                ui.label(RichText::new("·").size(11.5).color(colors.dim));
                                ui.label(RichText::new(&repo_label).size(11.5).color(colors.muted));
                                if let Some(ref br) = branch_label {
                                    ui.label(RichText::new("·").size(11.5).color(colors.dim));
                                    icons::inline(ui, Icon::Branch, colors.muted);
                                    let br_resp = ui.label(
                                        RichText::new(br).size(11.0).monospace().color(colors.text),
                                    );
                                    if let Some(wt) = cloud_meta
                                        .as_ref()
                                        .map(|m| m.cwd.as_str())
                                        .filter(|s| !s.is_empty())
                                    {
                                        br_resp.on_hover_text(format!(
                                            "Isolated cloud worktree: {}",
                                            crate::remote::cloud::compact_cloud_path(wt)
                                        ));
                                    }
                                }
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    egui::Frame::new()
                                        .fill(colors.accent.gamma_multiply(0.14))
                                        .corner_radius(99)
                                        .inner_margin(egui::Margin::symmetric(8, 2))
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new("Auto · Worktree")
                                                    .size(10.0)
                                                    .color(colors.accent),
                                            );
                                        });
                                });
                            });
                        });
                }
                egui::TopBottomPanel::bottom("composer-v2")
                    .show_separator_line(false)
                    .frame(
                        egui::Frame::new()
                            .fill(colors.bg)
                            .inner_margin(egui::Margin {
                                left: 24,
                                right: 24,
                                top: 8,
                                bottom: 3,
                            }),
                    )
                    .show_inside(ui, |ui| {
                        let width = (ui.available_width() - 24.0).min(760.0);
                        let padding = ((ui.available_width() - width) / 2.0).max(0.0);
                        ui.horizontal(|ui| {
                            ui.add_space(padding);
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                self.composer(ui, ctx);
                            });
                        });
                    });
                let pane_rect = ui.max_rect();
                if let Some(idx) = self
                    .chat_find
                    .sync(&self.state.messages, self.prefs.transcript_view)
                {
                    self.transcript.jump_to_message(idx);
                }
                if let Some(idx) = self.take_quote_jump(ctx) {
                    self.transcript.jump_to_message(idx);
                }
                self.transcript.find = self.chat_find.target();
                self.transcript.task_output = matches!(
                    self.task_session(&self.state),
                    Some(crate::tasks::Role::Proposal | crate::tasks::Role::Verifier)
                );
                self.transcript.view = self.prefs.transcript_view;
                self.transcript.pending_messages = self.state.pending_messages.clone();
                self.transcript.message_actions_enabled = self.history_ready();
                self.transcript.continue_enabled = self.can_continue_after_error();
                self.transcript.progress = self.transcript_progress();
                self.transcript.cwd =
                    (!self.prefs.cwd.as_os_str().is_empty()).then(|| self.prefs.cwd.clone());
                self.files
                    .set_page_snapshots(self.transcript.visuals.page_snapshots.clone());
                let has_tasks = self.tasks.records.iter().any(|t| self.task_in_chat(t));
                if has_tasks {
                    let cards = self.chat_task_cards();
                    let mut selected = None;
                    self.transcript.show_with_footer(
                        ui,
                        &self.state.messages,
                        &self.state.model,
                        &self.state.thinking,
                        !self.state.ready(),
                        self.state.session_file.as_deref().unwrap_or("new"),
                        |ui| selected = tasks::chat::task_chat_cards(ui, &cards),
                    );
                    if let Some(id) = selected {
                        self.open_task_from_board(id, None);
                    }
                } else if self.state.messages.is_empty()
                    && (!self.new_chat || !self.state.pending_messages.is_empty())
                    && self.transcript.progress.is_some()
                {
                    self.transcript.show(
                        ui,
                        &[],
                        &self.state.model,
                        &self.state.thinking,
                        !self.state.ready(),
                        self.state.session_file.as_deref().unwrap_or("new"),
                    );
                } else if self.state.messages.is_empty() && self.state.pending_messages.is_empty() {
                    let width = ui.available_width();
                    let pad = ((width - 720.0) / 2.0).max(30.0);
                    egui::ScrollArea::vertical()
                        .id_salt("new-conversation-welcome")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add_space(pad);
                                ui.vertical(|ui| {
                                    ui.set_width((width - pad * 2.0).max(180.0));
                                    self.welcome(ui);
                                });
                            });
                        });
                } else {
                    self.transcript.show(
                        ui,
                        &self.state.messages,
                        &self.state.model,
                        &self.state.thinking,
                        !self.state.ready(),
                        self.state.session_file.as_deref().unwrap_or(if self.demo {
                            "preview"
                        } else {
                            "new"
                        }),
                    );
                }
                self.attention_rendered(ui);
                if let Some(action) = self.transcript.action.take() {
                    match action {
                        crate::transcript::TranscriptAction::Continue(index) => {
                            self.continue_after_error(index);
                        }
                        crate::transcript::TranscriptAction::Open(target) => {
                            if let Err(error) =
                                self.files.open_target(ctx, &self.prefs.cwd, &target)
                            {
                                self.state.notice = Some(error);
                            } else {
                                self.settings_open = false;
                                self.inspector_panel = false;
                            }
                        }
                        crate::transcript::TranscriptAction::ViewSource(target) => {
                            let candidate = std::path::Path::new(&target);
                            let resolved = if candidate.is_absolute() {
                                candidate.to_path_buf()
                            } else {
                                self.prefs
                                    .cwd
                                    .join(target.strip_prefix("./").unwrap_or(&target))
                            };
                            let result = self
                                .files
                                .open_workspace_source(ctx, &self.prefs.cwd, &resolved, None)
                                .or_else(|_| self.files.open_target(ctx, &self.prefs.cwd, &target));
                            if let Err(error) = result {
                                self.state.notice = Some(error);
                            } else {
                                self.settings_open = false;
                                self.inspector_panel = false;
                            }
                        }
                        crate::transcript::TranscriptAction::OpenVisual { id, title, html } => {
                            self.files
                                .open_visual_html(ctx, &self.prefs.cwd, &id, &title, &html);
                            self.settings_open = false;
                            self.inspector_panel = false;
                        }
                    }
                    ctx.request_repaint();
                }
                if self
                    .transcript
                    .find
                    .as_ref()
                    .is_some_and(|f| !f.scroll_to_active)
                {
                    self.chat_find.scroll_to_active = false;
                }
                self.show_chat_find_bar(ctx, pane_rect);
            });
        self.history_action();
    }
}

struct McpConnector {
    name: String,
    error: Option<String>,
}

fn load_mcp_connectors(cwd: &std::path::Path, diagnostics: &[String]) -> Vec<McpConnector> {
    let mut names = std::collections::BTreeSet::new();
    let mut candidates = Vec::new();
    if let Some(agent) = crate::runtime::agent_dir() {
        candidates.push(agent.join("mcp.json"));
    }
    if !cwd.as_os_str().is_empty() {
        candidates.push(cwd.join(".pi/mcp.json"));
        candidates.push(cwd.join(".mcp.json"));
    }
    for path in candidates {
        if let Ok(raw) = std::fs::read_to_string(&path)
            && let Ok(val) = serde_json::from_str::<Value>(&raw)
            && let Some(servers) = val.get("mcpServers").and_then(Value::as_object)
        {
            for key in servers.keys() {
                names.insert(key.clone());
            }
        }
    }
    names
        .into_iter()
        .map(|name| {
            let lower = name.to_ascii_lowercase();
            let error = diagnostics
                .iter()
                .find(|d| d.to_ascii_lowercase().contains(&lower))
                .cloned();
            McpConnector { name, error }
        })
        .collect()
}

fn is_previewable_output(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "md" | "markdown" | "csv"
            )
        })
}

fn output_row(ui: &mut egui::Ui, path: &str, added: &str, removed: &str) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4, colors.hover);
    }
    let (file_icon, file_tint) = files::file_icon(std::path::Path::new(path), colors);
    icons::draw(
        ui,
        egui::Rect::from_center_size(rect.left_center() + egui::vec2(8.0, 0.0), Vec2::splat(14.0)),
        file_icon,
        file_tint,
    );
    let name = std::path::Path::new(path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let added_galley = (!added.is_empty() && added != "0").then(|| {
        ui.painter().layout_no_wrap(
            format!("+{added}"),
            FontId::proportional(10.0),
            colors.green,
        )
    });
    let removed_galley = (!removed.is_empty() && removed != "0").then(|| {
        ui.painter().layout_no_wrap(
            format!("−{removed}"),
            FontId::proportional(10.0),
            colors.red,
        )
    });
    let counts_width: f32 = [&added_galley, &removed_galley]
        .into_iter()
        .flatten()
        .map(|galley| galley.size().x + 8.0)
        .sum();
    let mut job = egui::text::LayoutJob::simple_singleline(
        name.into_owned(),
        FontId::proportional(11.0),
        colors.text,
    );
    job.wrap.max_width = (rect.width() - 26.0 - 15.0 - counts_width).max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(job);
    let elided = galley.elided;
    ui.painter().galley(
        rect.left_center() + egui::vec2(26.0, -galley.size().y / 2.0),
        galley,
        colors.text,
    );
    let mut x = rect.right() - 15.0;
    for (galley, color) in [(removed_galley, colors.red), (added_galley, colors.green)] {
        if let Some(galley) = galley {
            ui.painter().galley(
                egui::pos2(x - galley.size().x, rect.center().y - galley.size().y / 2.0),
                galley.clone(),
                color,
            );
            x -= galley.size().x + 8.0;
        }
    }
    icons::chevron(
        ui,
        egui::Rect::from_center_size(
            rect.right_center() - egui::vec2(3.0, 0.0),
            Vec2::splat(10.0),
        ),
        false,
        colors.dim,
    );
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if elided {
        response.on_hover_text(path)
    } else {
        response
    }
}

#[cfg(test)]
#[path = "inspector_layout_tests.rs"]
mod inspector_layout_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_is_below_output_and_stop_is_next_to_send_not_model() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        app.connected = true;
        app.client = Some(rpc::Client::stub());
        app.state = State {
            busy: true,
            session_file: Some("/fixture/progress.jsonl".into()),
            model_name: "Fixture model".into(),
            messages: vec![Message {
                role: "assistant".into(),
                text: "Latest output text".into(),
                images: vec![
                    crate::media::Attachment::from_bytes(include_bytes!("../../assets/icon.png"))
                        .unwrap(),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let render = |app: &mut Pui, time: f64| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1000.0, 800.0),
                    )),
                    time: Some(time),
                    ..Default::default()
                },
                |ctx| app.conversation(ctx),
            )
        };
        render(&mut app, 0.0);
        let out = render(&mut app, 0.1);
        fn inspect(
            shape: &egui::Shape,
            text: &mut Vec<(String, egui::Pos2)>,
            stops: &mut Vec<egui::Rect>,
            sends: &mut Vec<egui::Rect>,
            images: &mut Vec<egui::Rect>,
        ) {
            match shape {
                egui::Shape::Text(t) => text.push((t.galley.job.text.clone(), t.pos)),
                egui::Shape::Rect(r) if r.rect.size() == Vec2::splat(8.0) => stops.push(r.rect),
                egui::Shape::Rect(r) if r.rect.size() == Vec2::splat(28.0) => sends.push(r.rect),
                egui::Shape::Rect(r) if r.rect.size() == Vec2::new(112.0, 76.0) => {
                    images.push(r.rect)
                }
                egui::Shape::Vec(v) => {
                    for s in v {
                        inspect(s, text, stops, sends, images);
                    }
                }
                _ => {}
            }
        }
        let mut text = vec![];
        let mut stops = vec![];
        let mut sends = vec![];
        let mut images = vec![];
        for shape in out.shapes {
            inspect(&shape.shape, &mut text, &mut stops, &mut sends, &mut images);
        }
        let progress = text
            .iter()
            .find(|(t, _)| t == "Thinking…")
            .expect("working verb rendered")
            .1;
        let output = text
            .iter()
            .find(|(t, _)| t == "Latest output text")
            .unwrap()
            .1;
        let composer = text
            .iter()
            .find(|(t, _)| t == "Ask a follow-up…")
            .unwrap()
            .1;
        assert!(
            progress.y > output.y + 76.0 && progress.y < composer.y,
            "status belongs after image outputs and before composer: {progress:?}"
        );
        if let Some(image) = images.first() {
            assert!(progress.y > image.bottom());
        }
        assert!(!text.iter().any(|(t, _)| t == "Working…" || t == "Idle"));
        let stop = stops
            .iter()
            .find(|r| r.center().x > 800.0)
            .expect("stop on right");
        let send = sends
            .iter()
            .find(|r| {
                r.center().x > stop.center().x && (r.center().y - stop.center().y).abs() < 1.0
            })
            .expect("adjacent send button");
        assert!((send.center().x - stop.center().x - 32.0).abs() < 1.0);
        let total = app.transcript.total;
        render(&mut app, 4.1);
        assert!(
            (app.transcript.total - total).abs() < 0.1,
            "changing verbs must not move the transcript"
        );
        app.state.busy = false;
        render(&mut app, 5.0);
        assert!(app.transcript.progress.is_none());
    }
    #[test]
    fn session_details_divider_spans_the_pane_even_with_collapsed_content() {
        fn has_divider(shape: &egui::Shape, bounds: egui::Rect) -> bool {
            match shape {
                egui::Shape::LineSegment { points, stroke } => {
                    stroke.width > 0.0
                        && (points[0].x - bounds.right()).abs() < 1.0
                        && (points[1].x - points[0].x).abs() < 0.01
                        && (points[0].y - bounds.top()).abs() < 1.0
                        && (points[1].y - bounds.bottom()).abs() < 1.0
                }
                egui::Shape::Vec(v) => v.iter().any(|s| has_divider(s, bounds)),
                _ => false,
            }
        }
        for theme in [theme::Theme::Dark, theme::Theme::Light] {
            let ctx = egui::Context::default();
            let mut app = Pui::new(
                &eframe::CreationContext::_new_kittest(ctx.clone()),
                Options {
                    demo: true,
                    ..Default::default()
                },
            );
            theme.apply(&ctx);
            app.prefs.cards.collapsed.extend(Card::ALL);
            for zoom in [1.0, 1.5] {
                ctx.set_zoom_factor(zoom);
                let mut remaining = egui::Rect::NOTHING;
                for _ in 0..2 {
                    let out = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(1000.0, 800.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::TopBottomPanel::top("divider-test-toolbar")
                                .exact_height(40.0)
                                .show(ctx, |_| {});
                            app.inspector(ctx);
                            remaining = ctx.available_rect();
                        },
                    );
                    assert!(
                        out.shapes.iter().any(|s| has_divider(&s.shape, remaining)),
                        "Divider must span the full available pane height"
                    );
                }
            }
        }
    }

    #[test]
    fn mcp_warning_does_not_change_conversation_viewport() {
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        let mut frame = eframe::Frame::_new_kittest();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1480.0, 960.0),
            )),
            ..Default::default()
        };
        for _ in 0..3 {
            let _ = ctx.run(input(), |ctx| {
                eframe::App::update(&mut app, ctx, &mut frame)
            });
        }
        let height = app.transcript.viewport;
        app.state.apply(json!({"type":"extension_ui_request","method":"notify","message":"MCP servers need attention:\nLaunchDarkly: HTTP 503 Service Unavailable\nConnection closed\nRun /mcp to fix."}));
        let _ = ctx.run(input(), |ctx| {
            eframe::App::update(&mut app, ctx, &mut frame)
        });
        assert_eq!(app.transcript.viewport, height);
        assert_eq!(app.state.diagnostics.len(), 1);
        assert!(app.state.notice.is_none());
    }

    #[test]
    fn background_tasks_card_excludes_foreground_tools_and_shows_only_true_background_work() {
        fn collect_texts(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(t) => out.push((
                    t.galley.job.text.clone(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                )),
                egui::Shape::Vec(shapes) => {
                    for s in shapes {
                        collect_texts(s, out);
                    }
                }
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.prefs.cards.collapsed.extend(Card::ALL);
        app.prefs.cards.collapsed.remove(&Card::BackgroundTasks);
        // Ordinary foreground tool calls must never appear in Background tasks.
        app.state.messages = vec![protocol::Message {
            role: "assistant".into(),
            tools: vec![
                protocol::Tool {
                    id: "t1".into(),
                    name: "bash".into(),
                    command: "git status -s".into(),
                    finished: true,
                    duration_ms: Some(120),
                    ..Default::default()
                },
                protocol::Tool {
                    id: "t2".into(),
                    name: "read".into(),
                    path: "src/app.rs".into(),
                    finished: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }];

        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| {
                app.inspector(ctx);
            },
        );
        let mut texts = Vec::new();
        for clipped in &out.shapes {
            collect_texts(&clipped.shape, &mut texts);
        }
        assert!(
            texts
                .iter()
                .any(|(t, _)| t == "No active background tasks."),
            "foreground tools should not populate Background tasks: {texts:?}"
        );
        assert!(
            !texts
                .iter()
                .any(|(t, _)| t.contains("git status") || t.contains("src/app.rs")),
            "foreground tool labels must never leak into Background tasks: {texts:?}"
        );
    }
}
