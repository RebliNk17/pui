//! Quiet Mac UI for Background Tasks: both the compact Inspector card and the
//! dedicated right-side Background tasks pane (list + subagent detail view).
use super::*;

#[derive(Clone)]
struct AgentSnapshot {
    id: String,
    name: String,
    task: String,
    status: String,
    model_label: String,
    running: bool,
    failed: bool,
    stopped: bool,
    elapsed: Duration,
    created: SystemTime,
    child_total: usize,
    child_running: usize,
    child_failed: usize,
}

#[derive(Clone)]
struct ShellSnapshot {
    id: String,
    title: String,
    command: String,
    status: ShellStatus,
    elapsed: Duration,
    started: SystemTime,
    output: String,
    truncated: bool,
    error: Option<String>,
}

enum PanelAction {
    None,
    OpenAgent(String),
    BackToList,
    ToggleShell(String),
    ToggleChildren(String),
    ToggleBrief(String),
    ToggleTools(String),
    StopAgent(String),
    StopShell(String),
    StopDynamicWorkflow(String),
    DismissWorkflowBudget(String),
    ToggleWorkflowPhase(String, usize),
    OpenWorkflow(String),
    OpenDynamicWorkflow,
    OpenAutomation,
    PauseJob(String),
    ClearFinished,
    ToggleExpand,
    Close,
    CopyText(String),
}

fn map_workflow_action(action: workflow_ui::WorkflowUiAction) -> PanelAction {
    match action {
        workflow_ui::WorkflowUiAction::None => PanelAction::None,
        workflow_ui::WorkflowUiAction::StopWorkflow(id) => PanelAction::StopDynamicWorkflow(id),
        workflow_ui::WorkflowUiAction::DismissBudget(id) => PanelAction::DismissWorkflowBudget(id),
        workflow_ui::WorkflowUiAction::TogglePhase(wf, idx) => {
            PanelAction::ToggleWorkflowPhase(wf, idx)
        }
        workflow_ui::WorkflowUiAction::OpenAgent(id) => PanelAction::OpenAgent(id),
        workflow_ui::WorkflowUiAction::OpenShell(id) => PanelAction::ToggleShell(id),
    }
}

fn stop_square_button(ui: &mut egui::Ui, tooltip: &str) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
    let fill = if response.hovered() {
        colors.border
    } else {
        colors.surface
    };
    ui.painter().rect_filled(rect, 6.0, fill);
    ui.painter().rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0_f32, colors.border),
        egui::StrokeKind::Inside,
    );
    let inner = egui::Rect::from_center_size(rect.center(), Vec2::splat(7.5));
    ui.painter().rect_filled(
        inner,
        1.5,
        if response.hovered() {
            colors.text
        } else {
            colors.muted
        },
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tooltip)
}

fn compact_stop_dot(ui: &mut egui::Ui, tooltip: &str) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, colors.hover);
    }
    let inner = egui::Rect::from_center_size(rect.center(), Vec2::splat(7.5));
    ui.painter().rect_filled(
        inner,
        1.5,
        if response.hovered() {
            colors.text
        } else {
            colors.muted
        },
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tooltip)
}

impl Pui {
    fn snapshot_agent(&self, node: &agents::Node) -> AgentSnapshot {
        let created = node.created.unwrap_or_else(SystemTime::now);
        let end = node.finished_at.unwrap_or_else(SystemTime::now);
        let elapsed = end.duration_since(created).unwrap_or_default();
        let (child_total, child_running, child_failed) = self.background_child_counts(&node.id);
        let model_label = self
            .agents
            .parked
            .get(&node.id)
            .map(|c| {
                if !c.state.model_name.is_empty() {
                    c.state.model_name.clone()
                } else {
                    c.state.model.clone()
                }
            })
            .filter(|s| !s.is_empty())
            .or_else(|| node.model.clone())
            .unwrap_or_default();
        AgentSnapshot {
            id: node.id.clone(),
            name: node.name.clone(),
            task: node.task.clone(),
            status: node.status.clone(),
            model_label,
            running: is_node_running(node),
            failed: is_node_failed(node),
            stopped: node.stopped,
            elapsed,
            created,
            child_total,
            child_running,
            child_failed,
        }
    }

    fn snapshot_shell(shell: &ShellTask) -> ShellSnapshot {
        ShellSnapshot {
            id: shell.id.clone(),
            title: shell.title.clone(),
            command: shell.command.clone(),
            status: shell.status,
            elapsed: shell.elapsed(),
            started: shell.started,
            output: shell.output.clone(),
            truncated: shell.truncated,
            error: shell.error.clone(),
        }
    }

    /// Compact Inspector card matching Claude Desktop's right-rail Background tasks card:
    /// ONLY running background tasks for the active chat (shells, background subagents,
    /// active monitors/crons, active workflow). Clicking any item opens the full pane.
    pub(in crate::app) fn background_tasks_card(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        let active = self.agents.active.clone();

        let running_shells: Vec<ShellSnapshot> = self
            .background_tasks
            .shells
            .iter()
            .filter(|s| {
                !active.is_empty()
                    && s.owner == active
                    && s.workflow_id.is_none()
                    && s.status.is_running()
            })
            .map(Self::snapshot_shell)
            .collect();
        let running_agents: Vec<AgentSnapshot> = self
            .agents
            .nodes
            .values()
            .filter(|n| {
                !active.is_empty()
                    && n.background
                    && n.workflow_id.is_none()
                    && n.parent.as_deref() == Some(active.as_str())
                    && is_node_running(n)
            })
            .map(|n| self.snapshot_agent(n))
            .collect();
        let running_workflows: Vec<(String, String, Duration, usize, usize)> = self
            .background_tasks
            .workflows
            .iter()
            .filter(|w| !active.is_empty() && w.owner == active && w.status.is_running())
            .map(|w| {
                let (total, done, _, _) = w.counts();
                (w.id.clone(), w.name.clone(), w.elapsed(), done, total)
            })
            .collect();
        let active_jobs: Vec<(String, String)> = if active.is_empty() {
            Vec::new()
        } else {
            self.automation
                .active_chat_jobs(&active)
                .into_iter()
                .map(|(id, name, _)| (id, name))
                .collect()
        };
        let active_workflow = self
            .inspector_workflow()
            .filter(|t| t.phase.active())
            .map(|t| {
                (
                    t.id.clone(),
                    t.objective.clone(),
                    t.phase.label().to_owned(),
                )
            });

        let (_, finished_count) = self.active_background_counts();
        let mut action = PanelAction::None;

        if running_shells.is_empty()
            && running_agents.is_empty()
            && running_workflows.is_empty()
            && active_jobs.is_empty()
            && active_workflow.is_none()
        {
            ui.label(theme::small(ui, "No active background tasks."));
        } else {
            let row_w = (ui.available_width() - 4.0).max(40.0);
            for (wf_id, wf_name, elapsed, done, total) in &running_workflows {
                ui.push_id(wf_id, |ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(row_w, 22.0),
                        Layout::right_to_left(Align::Center),
                        |ui| {
                            ui.set_max_width(row_w);
                            if compact_stop_dot(ui, "Stop dynamic workflow").clicked() {
                                action = PanelAction::StopDynamicWorkflow(wf_id.clone());
                            }
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!(
                                    "{done}/{total} · {}",
                                    format_task_duration(*elapsed)
                                ))
                                .size(10.5)
                                .color(colors.muted),
                            );
                            ui.add_space(6.0);
                            let left_w = ui.available_width().max(20.0);
                            ui.allocate_ui_with_layout(
                                Vec2::new(left_w, 22.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    ui.set_max_width(left_w);
                                    icons::inline(ui, Icon::Agents, colors.accent);
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(wf_name)
                                                    .size(11.5)
                                                    .color(colors.text),
                                            )
                                            .truncate()
                                            .frame(false),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text(wf_name)
                                        .clicked()
                                    {
                                        action = PanelAction::OpenDynamicWorkflow;
                                    }
                                },
                            );
                        },
                    );
                });
            }
            for shell in &running_shells {
                ui.push_id(&shell.id, |ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(row_w, 22.0),
                        Layout::right_to_left(Align::Center),
                        |ui| {
                            ui.set_max_width(row_w);
                            if compact_stop_dot(ui, "Stop background command").clicked() {
                                action = PanelAction::StopShell(shell.id.clone());
                            }
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format_task_duration(shell.elapsed))
                                    .size(10.5)
                                    .color(colors.muted),
                            );
                            ui.add_space(6.0);
                            let left_w = ui.available_width().max(20.0);
                            ui.allocate_ui_with_layout(
                                Vec2::new(left_w, 22.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    ui.set_max_width(left_w);
                                    icons::inline(ui, Icon::Terminal, colors.muted);
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(&shell.title)
                                                    .size(11.5)
                                                    .color(colors.text),
                                            )
                                            .truncate()
                                            .frame(false),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text(&shell.title)
                                        .clicked()
                                    {
                                        action = PanelAction::ToggleShell(shell.id.clone());
                                    }
                                },
                            );
                        },
                    );
                });
            }
            for agent in &running_agents {
                ui.push_id(&agent.id, |ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(row_w, 22.0),
                        Layout::right_to_left(Align::Center),
                        |ui| {
                            ui.set_max_width(row_w);
                            if compact_stop_dot(ui, "Stop background subagent").clicked() {
                                action = PanelAction::StopAgent(agent.id.clone());
                            }
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format_task_duration(agent.elapsed))
                                    .size(10.5)
                                    .color(colors.muted),
                            );
                            ui.add_space(6.0);
                            let left_w = ui.available_width().max(20.0);
                            ui.allocate_ui_with_layout(
                                Vec2::new(left_w, 22.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    ui.set_max_width(left_w);
                                    icons::inline(ui, Icon::Agents, colors.muted);
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(&agent.name)
                                                    .size(11.5)
                                                    .color(colors.text),
                                            )
                                            .truncate()
                                            .frame(false),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text(&agent.name)
                                        .clicked()
                                    {
                                        action = PanelAction::OpenAgent(agent.id.clone());
                                    }
                                },
                            );
                        },
                    );
                });
            }
            for (job_id, job_name) in &active_jobs {
                ui.push_id(job_id, |ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(row_w, 22.0),
                        Layout::right_to_left(Align::Center),
                        |ui| {
                            ui.set_max_width(row_w);
                            if compact_stop_dot(ui, "Pause job").clicked() {
                                action = PanelAction::PauseJob(job_id.clone());
                            }
                            ui.add_space(6.0);
                            let left_w = ui.available_width().max(20.0);
                            ui.allocate_ui_with_layout(
                                Vec2::new(left_w, 22.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    ui.set_max_width(left_w);
                                    icons::inline(ui, Icon::Clock, colors.muted);
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(job_name)
                                                    .size(11.5)
                                                    .color(colors.text),
                                            )
                                            .truncate()
                                            .frame(false),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text(job_name)
                                        .clicked()
                                    {
                                        action = PanelAction::OpenAutomation;
                                    }
                                },
                            );
                        },
                    );
                });
            }
            if let Some((wf_id, objective, phase)) = &active_workflow {
                let label = if objective.trim().is_empty() {
                    format!("Workflow · {phase}")
                } else {
                    objective.clone()
                };
                ui.horizontal(|ui| {
                    icons::inline(ui, Icon::Agents, colors.accent);
                    if ui
                        .add(
                            egui::Button::new(RichText::new(&label).size(11.5).color(colors.text))
                                .truncate()
                                .frame(false),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        action = PanelAction::OpenWorkflow(wf_id.clone());
                    }
                });
            }
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }

        if finished_count > 0 {
            ui.add_space(4.0);
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(format!("{finished_count} finished ›"))
                            .size(11.0)
                            .color(colors.muted),
                    )
                    .frame(false),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                self.open_background_tasks_panel(None, None);
            }
        }

        match action {
            PanelAction::OpenAgent(id) => self.open_background_tasks_panel(Some(id), None),
            PanelAction::ToggleShell(id) => self.open_background_tasks_panel(None, Some(id)),
            PanelAction::StopShell(id) => {
                self.background_tasks.stop_shell(&id);
            }
            PanelAction::StopAgent(id) => self.stop_agent_tree(&id),
            PanelAction::StopDynamicWorkflow(id) => self.stop_dynamic_workflow(&id),
            PanelAction::OpenDynamicWorkflow => self.open_background_tasks_panel(None, None),
            PanelAction::PauseJob(id) => {
                let _ = self.change_job(&id, "pause");
            }
            PanelAction::OpenAutomation => self.open_automation(),
            PanelAction::OpenWorkflow(id) => self.open_task_from_board(id, None),
            _ => {}
        }
    }

    pub(in crate::app) fn open_background_tasks_panel(
        &mut self,
        agent: Option<String>,
        shell: Option<String>,
    ) {
        self.kanban.open = false;
        self.automation.open = false;
        self.files.close();
        self.inspector_panel = false;
        if let Some(id) = agent {
            self.background_tasks.open_agent(id);
        } else if let Some(id) = shell {
            self.background_tasks.open_shell(id);
        } else {
            self.background_tasks.open_list();
        }
    }

    /// Dedicated right-side Background tasks pane (Claude Desktop parity):
    /// returns `true` when expanded to occupy the central area.
    pub(in crate::app) fn background_tasks_panel(&mut self, ctx: &egui::Context) -> bool {
        let colors = theme::palette(ctx);
        let expanded = self.background_tasks.expanded;
        let mut action = PanelAction::None;
        if expanded {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(colors.bg))
                .show(ctx, |ui| {
                    action = self.background_tasks_contents(ui);
                });
        } else {
            let available = ctx.available_rect();
            if available.width() < 700.0 {
                egui::TopBottomPanel::bottom("workspace-background-tasks-stack")
                    .default_height((available.height() * 0.45).clamp(160.0, 340.0))
                    .height_range(140.0..=(available.height() - 240.0).max(140.0))
                    .resizable(true)
                    .frame(egui::Frame::new().fill(colors.bg))
                    .show(ctx, |ui| {
                        ui.set_min_height(ui.available_height());
                        action = self.background_tasks_contents(ui);
                    });
            } else {
                egui::SidePanel::right("workspace-background-tasks-dock")
                    .default_width((available.width() * 0.44).clamp(320.0, 680.0))
                    .width_range(280.0..=(available.width() - 320.0).max(280.0))
                    .resizable(true)
                    .show_separator_line(true)
                    .frame(egui::Frame::new().fill(colors.bg))
                    .show(ctx, |ui| {
                        action = self.background_tasks_contents(ui);
                    });
            }
        }
        self.apply_panel_action(ctx, action);
        expanded
    }

    fn apply_panel_action(&mut self, ctx: &egui::Context, action: PanelAction) {
        match action {
            PanelAction::None => {}
            PanelAction::OpenAgent(id) => {
                self.background_tasks.selected_agent = Some(id);
            }
            PanelAction::BackToList => {
                self.background_tasks.selected_agent = None;
            }
            PanelAction::ToggleShell(id) => {
                if !self.background_tasks.expanded_shells.remove(&id) {
                    self.background_tasks.expanded_shells.insert(id);
                }
            }
            PanelAction::ToggleChildren(id) => {
                if !self.background_tasks.expanded_children.remove(&id) {
                    self.background_tasks.expanded_children.insert(id);
                }
            }
            PanelAction::ToggleBrief(id) => {
                if !self.background_tasks.expanded_brief.remove(&id) {
                    self.background_tasks.expanded_brief.insert(id);
                }
            }
            PanelAction::ToggleTools(id) => {
                if !self.background_tasks.expanded_tools.remove(&id) {
                    self.background_tasks.expanded_tools.insert(id);
                }
            }
            PanelAction::StopAgent(id) => self.stop_agent_tree(&id),
            PanelAction::StopShell(id) => {
                self.background_tasks.stop_shell(&id);
            }
            PanelAction::StopDynamicWorkflow(id) => self.stop_dynamic_workflow(&id),
            PanelAction::DismissWorkflowBudget(id) => {
                if let Some(wf) = self
                    .background_tasks
                    .workflows
                    .iter_mut()
                    .find(|w| w.id == id)
                {
                    wf.budget_dismissed = true;
                }
            }
            PanelAction::ToggleWorkflowPhase(wf_id, phase_idx) => {
                let key = (wf_id, phase_idx);
                if !self.background_tasks.expanded_workflow_phases.remove(&key) {
                    self.background_tasks.expanded_workflow_phases.insert(key);
                }
            }
            PanelAction::OpenWorkflow(id) => {
                self.background_tasks.open = false;
                self.open_task_from_board(id, None);
            }
            PanelAction::OpenDynamicWorkflow => {
                self.background_tasks.open_list();
            }
            PanelAction::OpenAutomation => {
                self.background_tasks.open = false;
                self.open_automation();
            }
            PanelAction::PauseJob(id) => {
                let _ = self.change_job(&id, "pause");
            }
            PanelAction::ClearFinished => self.clear_finished_background_tasks(),
            PanelAction::ToggleExpand => {
                self.background_tasks.expanded = !self.background_tasks.expanded;
            }
            PanelAction::Close => {
                self.background_tasks.open = false;
                self.background_tasks.expanded = false;
                self.background_tasks.selected_agent = None;
            }
            PanelAction::CopyText(text) => {
                ctx.copy_text(text);
                self.state.notice = Some("Copied background output.".into());
            }
        }
    }

    fn background_tasks_contents(&mut self, ui: &mut egui::Ui) -> PanelAction {
        let colors = theme::palette(ui.ctx());
        let mut action = PanelAction::None;
        let active = self.agents.active.clone();

        // Validate selected_agent still belongs to the active chat.
        if let Some(selected) = self.background_tasks.selected_agent.as_deref()
            && !self.is_background_agent_of(&active, selected)
        {
            self.background_tasks.selected_agent = None;
        }
        let (running_total, finished_total) = self.active_background_counts();
        if running_total > 0 {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }

        // Top pane header bar matching Pui/Claude quiet styling.
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    egui::Frame::new()
                        .fill(colors.surface)
                        .stroke(Stroke::new(1.0_f32, colors.border))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::symmetric(9, 4))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                icons::inline(ui, Icon::Agents, colors.text);
                                ui.label(
                                    RichText::new("Background tasks")
                                        .size(12.0)
                                        .color(colors.text),
                                );
                            });
                        });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icons::button(ui, Icon::Close, "Close Background tasks").clicked() {
                            action = PanelAction::Close;
                        }
                        let (exp_icon, exp_tip) = if self.background_tasks.expanded {
                            (Icon::Collapse, "Restore split view")
                        } else {
                            (Icon::Expand, "Expand Background tasks")
                        };
                        if icons::button(ui, exp_icon, exp_tip).clicked() {
                            action = PanelAction::ToggleExpand;
                        }
                        if finished_total > 0
                            && self.background_tasks.selected_agent.is_none()
                            && icons::button(ui, Icon::Trash, "Clear finished background tasks")
                                .clicked()
                        {
                            action = PanelAction::ClearFinished;
                        }
                    });
                });
            });
        ui.separator();

        if let Some(selected_id) = self.background_tasks.selected_agent.clone() {
            let detail_action = self.background_agent_detail(ui, &selected_id);
            if matches!(action, PanelAction::None) {
                action = detail_action;
            }
            return action;
        }

        // Gather top-level background tasks for `active`.
        let mut running_workflows = Vec::new();
        let mut finished_workflows = Vec::new();
        for wf in &self.background_tasks.workflows {
            if !active.is_empty() && wf.owner == active {
                if wf.status.is_running() {
                    running_workflows.push(wf.clone());
                } else {
                    finished_workflows.push(wf.clone());
                }
            }
        }
        running_workflows.sort_by_key(|w| std::cmp::Reverse(w.started));
        finished_workflows.sort_by_key(|w| std::cmp::Reverse(w.started));

        let mut running_agents = Vec::new();
        let mut finished_agents = Vec::new();
        for node in self.agents.nodes.values() {
            if !active.is_empty()
                && node.background
                && node.workflow_id.is_none()
                && node.parent.as_deref() == Some(active.as_str())
            {
                let snap = self.snapshot_agent(node);
                if snap.running {
                    running_agents.push(snap);
                } else {
                    finished_agents.push(snap);
                }
            }
        }
        running_agents.sort_by_key(|a| std::cmp::Reverse(a.created));
        finished_agents.sort_by_key(|a| std::cmp::Reverse(a.created));

        let mut running_shells = Vec::new();
        let mut finished_shells = Vec::new();
        for shell in &self.background_tasks.shells {
            if !active.is_empty() && shell.owner == active && shell.workflow_id.is_none() {
                let snap = Self::snapshot_shell(shell);
                if snap.status.is_running() {
                    running_shells.push(snap);
                } else {
                    finished_shells.push(snap);
                }
            }
        }
        running_shells.sort_by_key(|s| std::cmp::Reverse(s.started));
        finished_shells.sort_by_key(|s| std::cmp::Reverse(s.started));

        let active_jobs: Vec<(String, String, String)> = if active.is_empty() {
            Vec::new()
        } else {
            self.automation.active_chat_jobs(&active)
        };
        let active_workflow = self
            .inspector_workflow()
            .filter(|t| t.phase.active())
            .cloned();

        let has_any = !running_workflows.is_empty()
            || !finished_workflows.is_empty()
            || !running_agents.is_empty()
            || !finished_agents.is_empty()
            || !running_shells.is_empty()
            || !finished_shells.is_empty()
            || !active_jobs.is_empty()
            || active_workflow.is_some();

        egui::ScrollArea::vertical()
            .id_salt("bg-tasks-pane-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(14, 12))
                    .show(ui, |ui| {
                        ui.set_width((ui.available_width() - 4.0).max(80.0));
                        if !has_any {
                            ui.add_space(48.0);
                            ui.vertical_centered(|ui| {
                                icons::inline(ui, Icon::Agents, colors.dim);
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new("Background tasks appear here")
                                        .size(12.5)
                                        .color(colors.muted),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(
                                        "Background subagents, long-running shell commands, and monitors started by this chat show up here.",
                                    )
                                    .size(11.0)
                                    .color(colors.dim),
                                );
                            });
                            return;
                        }

                        let running_count = running_workflows.len()
                            + running_agents.len()
                            + running_shells.len()
                            + active_jobs.len()
                            + usize::from(active_workflow.is_some());
                        if running_count > 0 {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Running")
                                        .size(11.5)
                                        .color(colors.muted),
                                );
                                ui.label(
                                    RichText::new(running_count.to_string())
                                        .size(11.0)
                                        .color(colors.dim),
                                );
                            });
                            ui.add_space(6.0);
                            for wf in &running_workflows {
                                let wa = workflow_ui::render_dynamic_workflow_card(
                                    ui,
                                    wf,
                                    &self.background_tasks.expanded_workflow_phases,
                                );
                                if matches!(action, PanelAction::None) {
                                    action = map_workflow_action(wa);
                                }
                                ui.add_space(8.0);
                            }
                            for agent in &running_agents {
                                let a = self.render_agent_card(ui, agent, false);
                                if matches!(action, PanelAction::None) {
                                    action = a;
                                }
                                ui.add_space(8.0);
                            }
                            for shell in &running_shells {
                                let a = self.render_shell_card(ui, shell, false);
                                if matches!(action, PanelAction::None) {
                                    action = a;
                                }
                                ui.add_space(8.0);
                            }
                            for (job_id, name, sched) in &active_jobs {
                                egui::Frame::new()
                                    .fill(colors.surface)
                                    .stroke(Stroke::new(1.0_f32, colors.border))
                                    .corner_radius(10.0)
                                    .inner_margin(egui::Margin::symmetric(12, 9))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            icons::inline(ui, Icon::Clock, colors.muted);
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new(name)
                                                            .size(12.5)
                                                            .color(colors.text),
                                                    )
                                                    .truncate()
                                                    .frame(false),
                                                )
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .clicked()
                                            {
                                                action = PanelAction::OpenAutomation;
                                            }
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    if stop_square_button(ui, "Pause job").clicked() {
                                                        action =
                                                            PanelAction::PauseJob(job_id.clone());
                                                    }
                                                    ui.add_space(6.0);
                                                    ui.label(
                                                        RichText::new(sched)
                                                            .size(11.0)
                                                            .color(colors.dim),
                                                    );
                                                },
                                            );
                                        });
                                    });
                                ui.add_space(8.0);
                            }
                            if let Some(task) = &active_workflow {
                                let cells = workflow::verified_task_cells(task);
                                egui::Frame::new()
                                    .fill(colors.surface)
                                    .stroke(Stroke::new(1.0_f32, colors.border))
                                    .corner_radius(10.0)
                                    .inner_margin(egui::Margin::symmetric(12, 10))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            icons::inline(ui, Icon::Agents, colors.accent);
                                            let title = if task.objective.trim().is_empty() {
                                                "Verified workflow".to_owned()
                                            } else {
                                                task.objective.clone()
                                            };
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new(format!("{title}  ›"))
                                                            .size(12.5)
                                                            .color(colors.text),
                                                    )
                                                    .truncate()
                                                    .frame(false),
                                                )
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .clicked()
                                            {
                                                action = PanelAction::OpenWorkflow(task.id.clone());
                                            }
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(task.phase.label())
                                                            .size(11.0)
                                                            .color(colors.muted),
                                                    );
                                                },
                                            );
                                        });
                                        ui.add_space(6.0);
                                        crate::inline_visual::paint_workflow_cell_grid(
                                            ui,
                                            &cells,
                                            task.phase.active(),
                                            2,
                                        );
                                    });
                                ui.add_space(8.0);
                            }
                            ui.add_space(4.0);
                        }

                        let finished_count = finished_workflows.len()
                            + finished_agents.len()
                            + finished_shells.len();
                        if finished_count > 0 {
                            let header = ui
                                .horizontal(|ui| {
                                    let (chev, _) = ui.allocate_exact_size(
                                        Vec2::splat(12.0),
                                        Sense::hover(),
                                    );
                                    icons::chevron(
                                        ui,
                                        chev,
                                        self.background_tasks.finished_open,
                                        colors.dim,
                                    );
                                    ui.label(
                                        RichText::new("Finished")
                                            .size(11.5)
                                            .color(colors.muted),
                                    );
                                    ui.label(
                                        RichText::new(finished_count.to_string())
                                            .size(11.0)
                                            .color(colors.dim),
                                    );
                                })
                                .response
                                .interact(Sense::click())
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            if header.clicked() {
                                self.background_tasks.finished_open =
                                    !self.background_tasks.finished_open;
                            }
                            if self.background_tasks.finished_open {
                                ui.add_space(6.0);
                                // Merge workflows, agents & shells by start time descending.
                                enum FinItem {
                                    Workflow(workflow::DynamicWorkflow),
                                    Agent(AgentSnapshot),
                                    Shell(ShellSnapshot),
                                }
                                let mut items: Vec<(SystemTime, FinItem)> = Vec::new();
                                for w in finished_workflows {
                                    items.push((w.started, FinItem::Workflow(w)));
                                }
                                for a in finished_agents {
                                    items.push((a.created, FinItem::Agent(a)));
                                }
                                for s in finished_shells {
                                    items.push((s.started, FinItem::Shell(s)));
                                }
                                items.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
                                for (_, item) in &items {
                                    let a = match item {
                                        FinItem::Workflow(wf) => map_workflow_action(
                                            workflow_ui::render_dynamic_workflow_card(
                                                ui,
                                                wf,
                                                &self.background_tasks.expanded_workflow_phases,
                                            ),
                                        ),
                                        FinItem::Agent(agent) => {
                                            self.render_agent_card(ui, agent, false)
                                        }
                                        FinItem::Shell(shell) => {
                                            self.render_shell_card(ui, shell, false)
                                        }
                                    };
                                    if matches!(action, PanelAction::None) {
                                        action = a;
                                    }
                                    ui.add_space(8.0);
                                }
                            }
                        }
                    });
            });

        action
    }

    fn render_agent_card(
        &self,
        ui: &mut egui::Ui,
        agent: &AgentSnapshot,
        nested: bool,
    ) -> PanelAction {
        let colors = theme::palette(ui.ctx());
        let mut action = PanelAction::None;
        let fill = if nested { colors.bg } else { colors.surface };
        egui::Frame::new()
            .fill(fill)
            .stroke(Stroke::new(1.0_f32, colors.border))
            .corner_radius(if nested { 8.0 } else { 10.0 })
            .inner_margin(egui::Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.push_id(&agent.id, |ui| {
                    let row_w = ui.available_width();
                    ui.allocate_ui_with_layout(
                        Vec2::new(row_w, 24.0),
                        Layout::right_to_left(Align::Center),
                        |ui| {
                            ui.set_max_width(row_w);
                            if agent.running {
                                if stop_square_button(ui, "Stop background subagent").clicked() {
                                    action = PanelAction::StopAgent(agent.id.clone());
                                }
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(format_task_duration(agent.elapsed))
                                        .size(11.5)
                                        .color(colors.muted),
                                );
                            } else if agent.failed {
                                ui.label(RichText::new("Failed").size(11.5).color(colors.red));
                            } else if agent.stopped {
                                ui.label(RichText::new("Stopped").size(11.5).color(colors.dim));
                            } else {
                                ui.label(
                                    RichText::new(format_task_duration(agent.elapsed))
                                        .size(11.5)
                                        .color(colors.muted),
                                );
                            }
                            ui.add_space(8.0);
                            let left_w = ui.available_width().max(40.0);
                            ui.allocate_ui_with_layout(
                                Vec2::new(left_w, 24.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    ui.set_max_width(left_w);
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(format!("{}  ›", agent.name))
                                                    .size(12.5)
                                                    .color(colors.text),
                                            )
                                            .truncate()
                                            .frame(false),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text(&agent.name)
                                        .clicked()
                                    {
                                        action = PanelAction::OpenAgent(agent.id.clone());
                                    }
                                },
                            );
                        },
                    );

                    // Nested child-tasks summary row (`> 4 tasks, 1 failed` / `> 5 tasks, 1 running`).
                    if agent.child_total > 0 {
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(3.0);
                        let expanded = self.background_tasks.expanded_children.contains(&agent.id);
                        let toggle = ui
                            .horizontal(|ui| {
                                let (chev, _) =
                                    ui.allocate_exact_size(Vec2::splat(11.0), Sense::hover());
                                icons::chevron(ui, chev, expanded, colors.dim);
                                let total_label = if agent.child_total == 1 {
                                    "1 task".to_owned()
                                } else {
                                    format!("{} tasks", agent.child_total)
                                };
                                ui.label(RichText::new(total_label).size(11.0).color(colors.muted));
                                if agent.child_running > 0 {
                                    ui.label(
                                        RichText::new(format!(", {} running", agent.child_running))
                                            .size(11.0)
                                            .color(colors.accent),
                                    );
                                }
                                if agent.child_failed > 0 {
                                    ui.label(
                                        RichText::new(format!(", {} failed", agent.child_failed))
                                            .size(11.0)
                                            .color(colors.red),
                                    );
                                }
                            })
                            .response
                            .interact(Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if toggle.clicked() {
                            action = PanelAction::ToggleChildren(agent.id.clone());
                        }
                        if expanded {
                            ui.add_space(6.0);
                            let child_action = self.render_nested_children(ui, &agent.id);
                            if matches!(action, PanelAction::None) {
                                action = child_action;
                            }
                        }
                    }
                });
            });
        action
    }

    fn render_nested_children(&self, ui: &mut egui::Ui, parent_id: &str) -> PanelAction {
        let mut action = PanelAction::None;
        let mut child_agents: Vec<AgentSnapshot> = self
            .agents
            .nodes
            .values()
            .filter(|n| n.background && n.parent.as_deref() == Some(parent_id))
            .map(|n| self.snapshot_agent(n))
            .collect();
        child_agents.sort_by_key(|a| std::cmp::Reverse(a.created));
        let mut child_shells: Vec<ShellSnapshot> = self
            .background_tasks
            .shells
            .iter()
            .filter(|s| s.owner == parent_id)
            .map(Self::snapshot_shell)
            .collect();
        child_shells.sort_by_key(|s| std::cmp::Reverse(s.started));

        ui.indent(("bg-children", parent_id), |ui| {
            for agent in &child_agents {
                let a = self.render_agent_card(ui, agent, true);
                if matches!(action, PanelAction::None) {
                    action = a;
                }
                ui.add_space(6.0);
            }
            for shell in &child_shells {
                let a = self.render_shell_card(ui, shell, true);
                if matches!(action, PanelAction::None) {
                    action = a;
                }
                ui.add_space(6.0);
            }
        });
        action
    }

    fn render_shell_card(
        &self,
        ui: &mut egui::Ui,
        shell: &ShellSnapshot,
        nested: bool,
    ) -> PanelAction {
        let colors = theme::palette(ui.ctx());
        let mut action = PanelAction::None;
        let expanded = self.background_tasks.expanded_shells.contains(&shell.id);
        let fill = if nested { colors.bg } else { colors.surface };
        egui::Frame::new()
            .fill(fill)
            .stroke(Stroke::new(1.0_f32, colors.border))
            .corner_radius(if nested { 8.0 } else { 10.0 })
            .inner_margin(egui::Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.push_id(&shell.id, |ui| {
                    let row_w = ui.available_width();
                    ui.allocate_ui_with_layout(
                        Vec2::new(row_w, 22.0),
                        Layout::right_to_left(Align::Center),
                        |ui| {
                            ui.set_max_width(row_w);
                            match shell.status {
                                ShellStatus::Running => {
                                    if stop_square_button(ui, "Stop background command").clicked() {
                                        action = PanelAction::StopShell(shell.id.clone());
                                    }
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(format_task_duration(shell.elapsed))
                                            .size(11.5)
                                            .color(colors.muted),
                                    );
                                }
                                ShellStatus::Completed => {
                                    ui.label(
                                        RichText::new(format_task_duration(shell.elapsed))
                                            .size(11.5)
                                            .color(colors.muted),
                                    );
                                }
                                ShellStatus::Failed(_) => {
                                    ui.label(RichText::new("Failed").size(11.5).color(colors.red));
                                }
                                ShellStatus::Stopped => {
                                    ui.label(RichText::new("Stopped").size(11.5).color(colors.dim));
                                }
                            }
                            ui.add_space(8.0);
                            let left_w = ui.available_width().max(40.0);
                            ui.allocate_ui_with_layout(
                                Vec2::new(left_w, 22.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    ui.set_max_width(left_w);
                                    let (chev, _) =
                                        ui.allocate_exact_size(Vec2::splat(11.0), Sense::hover());
                                    icons::chevron(ui, chev, expanded, colors.dim);
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(&shell.title)
                                                    .size(12.0)
                                                    .color(colors.text),
                                            )
                                            .truncate()
                                            .frame(false),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text(&shell.command)
                                        .clicked()
                                    {
                                        action = PanelAction::ToggleShell(shell.id.clone());
                                    }
                                },
                            );
                        },
                    );
                    if expanded {
                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&shell.command)
                                        .monospace()
                                        .size(10.5)
                                        .color(colors.muted),
                                )
                                .wrap(),
                            );
                            if !shell.output.is_empty() {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if icons::button(ui, Icon::Copy, "Copy output").clicked() {
                                        action = PanelAction::CopyText(shell.output.clone());
                                    }
                                });
                            }
                        });
                        if let Some(error) = &shell.error {
                            ui.add_space(4.0);
                            ui.label(RichText::new(error).size(11.0).color(colors.red));
                        }
                        if shell.truncated {
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new("(Earlier output truncated)")
                                    .size(10.0)
                                    .color(colors.dim),
                            );
                        }
                        ui.add_space(4.0);
                        egui::Frame::new()
                            .fill(colors.bg)
                            .stroke(Stroke::new(1.0_f32, colors.border))
                            .corner_radius(6.0)
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                egui::ScrollArea::vertical()
                                    .id_salt(("bg-shell-out", &shell.id))
                                    .max_height(220.0)
                                    .stick_to_bottom(shell.status.is_running())
                                    .show(ui, |ui| {
                                        let body = if shell.output.is_empty() {
                                            if shell.status.is_running() {
                                                "Running… (no output yet)"
                                            } else {
                                                "(No output)"
                                            }
                                        } else {
                                            shell.output.as_str()
                                        };
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(body).monospace().size(10.5).color(
                                                    if shell.output.is_empty() {
                                                        colors.dim
                                                    } else {
                                                        colors.text
                                                    },
                                                ),
                                            )
                                            .selectable(true)
                                            .wrap(),
                                        );
                                    });
                            });
                    }
                });
            });
        action
    }

    /// Detail view for a single background subagent (matches Claude Desktop screenshot 3):
    /// `< Title`, collapsible prompt brief card, `Model ...`, expandable tool summary,
    /// latest status/assistant response, and nested child background tasks.
    fn background_agent_detail(&self, ui: &mut egui::Ui, id: &str) -> PanelAction {
        let colors = theme::palette(ui.ctx());
        let Some(node) = self.agents.nodes.get(id) else {
            return PanelAction::BackToList;
        };
        let agent = self.snapshot_agent(node);
        let mut action = PanelAction::None;

        egui::ScrollArea::vertical()
            .id_salt(("bg-agent-detail", id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_width((ui.available_width() - 4.0).max(80.0));

                        // Back navigation header (`<  Groups: left order column + groups popup`).
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("‹").size(16.0).color(colors.muted),
                                    )
                                    .frame(false),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text("Back to Background tasks")
                                .clicked()
                            {
                                action = PanelAction::BackToList;
                            }
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&agent.name).size(13.0).color(colors.text),
                                )
                                .truncate(),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if agent.running {
                                    if stop_square_button(ui, "Stop background subagent").clicked()
                                    {
                                        action = PanelAction::StopAgent(agent.id.clone());
                                    }
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(format_task_duration(agent.elapsed))
                                            .size(11.5)
                                            .color(colors.muted),
                                    );
                                } else if agent.failed {
                                    ui.label(RichText::new("Failed").size(11.5).color(colors.red));
                                } else if agent.stopped {
                                    ui.label(RichText::new("Stopped").size(11.5).color(colors.dim));
                                } else {
                                    ui.label(
                                        RichText::new(format_task_duration(agent.elapsed))
                                            .size(11.5)
                                            .color(colors.muted),
                                    );
                                }
                            });
                        });
                        ui.add_space(10.0);

                        // Prompt brief card with Show more / Show less.
                        let brief_expanded =
                            self.background_tasks.expanded_brief.contains(&agent.id);
                        let long_brief =
                            agent.task.chars().count() > 420 || agent.task.lines().count() > 7;
                        let brief_text: String = if brief_expanded || !long_brief {
                            agent.task.clone()
                        } else {
                            let cut: String = agent.task.chars().take(420).collect();
                            format!("{cut}…")
                        };
                        egui::Frame::new()
                            .fill(colors.surface)
                            .stroke(Stroke::new(1.0_f32, colors.border))
                            .corner_radius(10.0)
                            .inner_margin(egui::Margin::symmetric(14, 12))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&brief_text).size(12.0).color(colors.text),
                                    )
                                    .selectable(true)
                                    .wrap(),
                                );
                                if long_brief {
                                    ui.add_space(6.0);
                                    let toggle_label = if brief_expanded {
                                        "Show less"
                                    } else {
                                        "Show more"
                                    };
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(toggle_label)
                                                    .size(11.5)
                                                    .color(colors.muted),
                                            )
                                            .frame(false),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        action = PanelAction::ToggleBrief(agent.id.clone());
                                    }
                                }
                            });
                        ui.add_space(12.0);

                        // Model line.
                        if !agent.model_label.is_empty() {
                            ui.label(
                                RichText::new(format!("Model {}", agent.model_label))
                                    .size(11.5)
                                    .color(colors.dim),
                            );
                            ui.add_space(8.0);
                        }

                        // Expandable tool-call activity summary (`Ran 24 commands, read 11 files… >`).
                        if let Some(chat) = self.agents.parked.get(id) {
                            let summary = summarize_agent_tools(&chat.state);
                            let tools_expanded =
                                self.background_tasks.expanded_tools.contains(&agent.id);
                            let suffix = if summary.total > 0 {
                                if tools_expanded { " ⌄" } else { " ›" }
                            } else {
                                ""
                            };
                            let resp = ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(format!("{}{}", summary.line, suffix))
                                            .size(12.0)
                                            .color(colors.muted),
                                    )
                                    .wrap()
                                    .frame(false),
                                )
                                .on_hover_cursor(if summary.total > 0 {
                                    egui::CursorIcon::PointingHand
                                } else {
                                    egui::CursorIcon::Default
                                });
                            if summary.total > 0 && resp.clicked() {
                                action = PanelAction::ToggleTools(agent.id.clone());
                            }
                            if tools_expanded && summary.total > 0 {
                                ui.add_space(6.0);
                                egui::Frame::new()
                                    .fill(colors.surface)
                                    .stroke(Stroke::new(1.0_f32, colors.border))
                                    .corner_radius(8.0)
                                    .inner_margin(egui::Margin::symmetric(10, 8))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        for tool in
                                            chat.state.messages.iter().flat_map(|m| &m.tools)
                                        {
                                            let label = if !tool.command.is_empty() {
                                                tool.command
                                                    .lines()
                                                    .find(|l| !l.trim().is_empty())
                                                    .unwrap_or(&tool.command)
                                                    .trim()
                                                    .to_owned()
                                            } else if !tool.path.is_empty() {
                                                format!("{} {}", tool.name, tool.path)
                                            } else {
                                                tool.name.clone()
                                            };
                                            let text_color = if tool.failed {
                                                colors.red
                                            } else if !tool.finished {
                                                colors.accent
                                            } else {
                                                colors.muted
                                            };
                                            ui.horizontal(|ui| {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(&label)
                                                            .monospace()
                                                            .size(10.5)
                                                            .color(text_color),
                                                    )
                                                    .truncate(),
                                                )
                                                .on_hover_text(&label);
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        if let Some(ms) = tool.duration_ms {
                                                            ui.label(
                                                                RichText::new(
                                                                    format_task_duration(
                                                                        Duration::from_millis(ms),
                                                                    ),
                                                                )
                                                                .size(10.0)
                                                                .color(colors.dim),
                                                            );
                                                        } else if !tool.finished {
                                                            ui.label(
                                                                RichText::new("running")
                                                                    .size(10.0)
                                                                    .color(colors.accent),
                                                            );
                                                        }
                                                    },
                                                );
                                            });
                                        }
                                    });
                            }
                            ui.add_space(10.0);

                            // Latest assistant text / live status / error.
                            let latest_text = chat
                                .state
                                .messages
                                .iter()
                                .rev()
                                .find(|m| m.role == "assistant" && !m.text.trim().is_empty())
                                .map(|m| m.text.trim().to_owned());
                            if let Some(text) = latest_text {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(text).size(12.5).color(colors.text),
                                    )
                                    .selectable(true)
                                    .wrap(),
                                );
                            } else if let Some(err) = chat.state.error.as_deref() {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(err).size(12.0).color(colors.red),
                                    )
                                    .selectable(true)
                                    .wrap(),
                                );
                            } else if agent.running {
                                ui.label(
                                    RichText::new(&agent.status).size(12.0).color(colors.muted),
                                );
                            }
                        } else if let Some(result) = node.result.as_deref() {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(result).size(12.5).color(colors.text),
                                )
                                .selectable(true)
                                .wrap(),
                            );
                        }

                        if let Some(err) = node.failure.as_deref() {
                            ui.add_space(8.0);
                            ui.add(
                                egui::Label::new(RichText::new(err).size(12.0).color(colors.red))
                                    .selectable(true)
                                    .wrap(),
                            );
                        }

                        // Nested child background tasks launched by this subagent.
                        if agent.child_total > 0 {
                            ui.add_space(14.0);
                            ui.separator();
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(format!("Child tasks ({})", agent.child_total))
                                    .size(11.5)
                                    .color(colors.muted),
                            );
                            ui.add_space(6.0);
                            let child_action = self.render_nested_children(ui, &agent.id);
                            if matches!(action, PanelAction::None) {
                                action = child_action;
                            }
                        }
                    });
            });

        action
    }
}
