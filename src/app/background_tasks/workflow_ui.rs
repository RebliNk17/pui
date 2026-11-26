//! Claude-style workflow card inside the Background tasks pane:
//! 6×6px agent micro-grid, collapsible Phase rows, surfaced error lines,
//! Agent | Tokens | Time table, budget notice, and click-to-inspect drill-in.
use super::*;
use crate::inline_visual::{
    WorkflowCell, WorkflowCellState, format_token_count, paint_workflow_cell_grid,
};

pub(super) enum WorkflowUiAction {
    None,
    StopWorkflow(String),
    DismissBudget(String),
    TogglePhase(String, usize),
    OpenAgent(String),
    OpenShell(String),
}

fn stop_square(ui: &mut egui::Ui, tooltip: &str) -> egui::Response {
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

fn draw_state_glyph(ui: &mut egui::Ui, state: WorkflowCellState) {
    let colors = theme::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    match state {
        WorkflowCellState::Done => icons::draw(ui, rect, Icon::Check, colors.green),
        WorkflowCellState::Running => {
            ui.painter()
                .circle_filled(rect.center(), 3.2, colors.accent);
        }
        WorkflowCellState::Error => {
            ui.painter().circle_filled(rect.center(), 3.2, colors.red);
        }
        WorkflowCellState::Stopped => {
            ui.painter()
                .circle_stroke(rect.center(), 3.0, Stroke::new(1.2_f32, colors.dim));
        }
        WorkflowCellState::Pending => {
            ui.painter()
                .circle_stroke(rect.center(), 3.0, Stroke::new(1.0_f32, colors.border));
        }
    }
}

pub(super) fn render_dynamic_workflow_card(
    ui: &mut egui::Ui,
    wf: &workflow::DynamicWorkflow,
    expanded_phases: &BTreeSet<(String, usize)>,
) -> WorkflowUiAction {
    let colors = theme::palette(ui.ctx());
    let mut action = WorkflowUiAction::None;
    let (total, done, running, failed) = wf.counts();
    let total_tokens = wf.total_tokens();

    egui::Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1.0_f32, colors.border))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.push_id(&wf.id, |ui| {
                // Header row: icon + title + elapsed/status + Stop button.
                let row_w = ui.available_width();
                ui.allocate_ui_with_layout(
                    Vec2::new(row_w, 24.0),
                    Layout::right_to_left(Align::Center),
                    |ui| {
                        ui.set_max_width(row_w);
                        if wf.status.is_running() {
                            if stop_square(ui, "Stop dynamic workflow").clicked() {
                                action = WorkflowUiAction::StopWorkflow(wf.id.clone());
                            }
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(format_task_duration(wf.elapsed()))
                                    .size(11.5)
                                    .color(colors.muted),
                            );
                        } else if wf.status == workflow::WorkflowStatus::Failed {
                            ui.label(RichText::new("Failed").size(11.5).color(colors.red));
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(format_task_duration(wf.elapsed()))
                                    .size(11.0)
                                    .color(colors.dim),
                            );
                        } else if wf.status == workflow::WorkflowStatus::Stopped {
                            ui.label(RichText::new("Stopped").size(11.5).color(colors.dim));
                        } else {
                            ui.label(
                                RichText::new(format_task_duration(wf.elapsed()))
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
                                icons::inline(
                                    ui,
                                    Icon::Agents,
                                    if wf.status.is_running() {
                                        colors.accent
                                    } else {
                                        colors.muted
                                    },
                                );
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&wf.name).size(12.5).color(colors.text),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&wf.objective);
                            },
                        );
                    },
                );

                // Aggregate stats line (`6 agents · 2 running · 3 done · 1 failed · 42.1k tok`).
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    let noun = if total == 1 {
                        "1 agent".to_owned()
                    } else {
                        format!("{total} agents")
                    };
                    ui.label(RichText::new(noun).size(11.0).color(colors.muted));
                    if running > 0 {
                        ui.label(
                            RichText::new(format!("· {running} running"))
                                .size(11.0)
                                .color(colors.accent),
                        );
                    }
                    if done > 0 {
                        ui.label(
                            RichText::new(format!("· {done} done"))
                                .size(11.0)
                                .color(colors.muted),
                        );
                    }
                    if failed > 0 {
                        ui.label(
                            RichText::new(format!("· {failed} failed"))
                                .size(11.0)
                                .color(colors.red),
                        );
                    }
                    if total_tokens > 0 {
                        ui.label(
                            RichText::new(format!("· {} tok", format_token_count(total_tokens)))
                                .size(11.0)
                                .color(colors.dim),
                        );
                    }
                });

                // Overall 6×6px status micro-grid.
                let all_cells = wf.cells();
                if !all_cells.is_empty() {
                    ui.add_space(6.0);
                    paint_workflow_cell_grid(ui, &all_cells, wf.status.is_running(), 6);
                }

                // Dismissible usage/agent budget warning (Claude Desktop `$qe` parity).
                if wf.show_budget_warning() {
                    ui.add_space(8.0);
                    egui::Frame::new()
                        .fill(colors.amber.gamma_multiply(0.10))
                        .stroke(Stroke::new(1.0_f32, colors.amber.gamma_multiply(0.35)))
                        .corner_radius(7.0)
                        .inner_margin(egui::Margin::symmetric(9, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let msg = if total_tokens > 0 {
                                    format!(
                                        "{total} agents scheduled ({} tokens used). Dynamic workflows can use your usage limit quickly — stop anytime.",
                                        format_token_count(total_tokens)
                                    )
                                } else {
                                    format!(
                                        "{total} agents scheduled. Dynamic workflows can use your usage limit quickly — stop anytime."
                                    )
                                };
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(msg).size(10.5).color(colors.text),
                                    )
                                    .wrap(),
                                );
                                ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                                    if icons::button(ui, Icon::Close, "Dismiss usage notice")
                                        .clicked()
                                    {
                                        action = WorkflowUiAction::DismissBudget(wf.id.clone());
                                    }
                                });
                            });
                        });
                }

                // Collapsible Phase rows.
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(2.0);
                let show_tokens_col = ui.available_width() >= 310.0;
                ui.spacing_mut().item_spacing.y = 2.0;
                for phase in &wf.phases {
                    let open = expanded_phases.contains(&(wf.id.clone(), phase.index));
                    let (p_total, p_done, p_running, p_failed) = phase.counts();
                    let header_w = ui.available_width();
                    let header_resp = ui
                        .allocate_ui_with_layout(
                            Vec2::new(header_w, 22.0),
                            Layout::right_to_left(Align::Center),
                            |ui| {
                                ui.set_max_width(header_w);
                                ui.label(
                                    RichText::new(format!("{p_done}/{p_total}"))
                                        .size(11.0)
                                        .color(colors.muted),
                                );
                                if p_failed > 0 {
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(format!("{p_failed} failed ·"))
                                            .size(11.0)
                                            .color(colors.red),
                                    );
                                }
                                ui.add_space(6.0);
                                let left_w = ui.available_width().max(40.0);
                                ui.allocate_ui_with_layout(
                                    Vec2::new(left_w, 22.0),
                                    Layout::left_to_right(Align::Center),
                                    |ui| {
                                        ui.set_max_width(left_w);
                                        let (chev, _) = ui.allocate_exact_size(
                                            Vec2::splat(11.0),
                                            Sense::hover(),
                                        );
                                        icons::chevron(ui, chev, open, colors.dim);
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&phase.title)
                                                    .size(11.5)
                                                    .color(if p_running > 0 {
                                                        colors.text
                                                    } else {
                                                        colors.muted
                                                    }),
                                            )
                                            .truncate(),
                                        );
                                    },
                                );
                            },
                        )
                        .response
                        .interact(Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if header_resp.clicked() {
                        action = WorkflowUiAction::TogglePhase(wf.id.clone(), phase.index);
                    }

                    // Show phase micro-grid when active or open (matching Claude Desktop `aLe`).
                    if (p_running > 0 || open) && !phase.agents.is_empty() {
                        let phase_cells: Vec<WorkflowCell> = phase
                            .agents
                            .iter()
                            .map(|a| WorkflowCell {
                                label: a.label.clone(),
                                state: a.state,
                                detail: a
                                    .error
                                    .clone()
                                    .unwrap_or_else(|| format!("{:?}", a.state)),
                            })
                            .collect();
                        ui.indent(("wf-phase-grid", &wf.id, phase.index), |ui| {
                            paint_workflow_cell_grid(ui, &phase_cells, p_running > 0, 4);
                        });
                    }

                    // Surfaced failed agents directly under a collapsed phase (`ZIe`).
                    if !open && p_failed > 0 {
                        let failed_agents: Vec<&workflow::WorkflowAgentSpec> = phase
                            .agents
                            .iter()
                            .filter(|a| a.state == WorkflowCellState::Error)
                            .collect();
                        ui.indent(("wf-phase-err", &wf.id, phase.index), |ui| {
                            for fa in failed_agents.iter().take(3) {
                                let err = fa.error.as_deref().unwrap_or("Failed");
                                let line = format!("{}: {err}", fa.label);
                                let resp = ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(&line).size(10.5).color(colors.red),
                                        )
                                        .truncate()
                                        .frame(false),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text(err);
                                if resp.clicked()
                                    && let Some(aid) = &fa.agent_id
                                {
                                    action = WorkflowUiAction::OpenAgent(aid.clone());
                                }
                            }
                            if failed_agents.len() > 3 {
                                ui.label(
                                    RichText::new(format!(
                                        "and {} more failed",
                                        failed_agents.len() - 3
                                    ))
                                    .size(10.0)
                                    .color(colors.dim),
                                );
                            }
                        });
                    }

                    // Expanded Phase Agent table: `[glyph] | Agent | Tokens | Time` (`WIe`).
                    if open {
                        ui.add_space(2.0);
                        ui.indent(("wf-phase-table", &wf.id, phase.index), |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.spacing_mut().interact_size.y = 18.0;
                            let table_w = ui.available_width();
                            let time_col_w = 64.0_f32;
                            let tok_col_w = 46.0_f32;
                            // Column headers
                            ui.allocate_ui_with_layout(
                                Vec2::new(table_w, 16.0),
                                Layout::right_to_left(Align::Center),
                                |ui| {
                                    ui.set_max_width(table_w);
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(time_col_w, 16.0),
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new("Time")
                                                    .size(10.0)
                                                    .color(colors.dim),
                                            );
                                        },
                                    );
                                    if show_tokens_col {
                                        ui.add_space(6.0);
                                        ui.allocate_ui_with_layout(
                                            Vec2::new(tok_col_w, 16.0),
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new("Tokens")
                                                        .size(10.0)
                                                        .color(colors.dim),
                                                );
                                            },
                                        );
                                    }
                                    ui.add_space(6.0);
                                    let left_w = ui.available_width().max(40.0);
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(left_w, 16.0),
                                        Layout::left_to_right(Align::Center),
                                        |ui| {
                                            ui.add_space(18.0);
                                            ui.label(
                                                RichText::new("Agent")
                                                    .size(10.0)
                                                    .color(colors.dim),
                                            );
                                        },
                                    );
                                },
                            );
                            for agent in &phase.agents {
                                ui.allocate_ui_with_layout(
                                    Vec2::new(table_w, 19.0),
                                    Layout::right_to_left(Align::Center),
                                    |ui| {
                                        ui.set_max_width(table_w);
                                        ui.allocate_ui_with_layout(
                                            Vec2::new(time_col_w, 19.0),
                                            Layout::right_to_left(Align::Center),
                                            |ui| match agent.state {
                                                WorkflowCellState::Error => {
                                                    ui.label(
                                                        RichText::new("Error")
                                                            .size(10.5)
                                                            .color(colors.red),
                                                    )
                                                    .on_hover_text(
                                                        agent.error.as_deref().unwrap_or("Failed"),
                                                    );
                                                }
                                                WorkflowCellState::Stopped => {
                                                    ui.label(
                                                        RichText::new("Stopped")
                                                            .size(10.5)
                                                            .color(colors.dim),
                                                    );
                                                }
                                                _ => {
                                                    let dur = agent
                                                        .duration_ms
                                                        .map(|ms| {
                                                            format_task_duration(
                                                                Duration::from_millis(ms),
                                                            )
                                                        })
                                                        .unwrap_or_default();
                                                    ui.label(
                                                        RichText::new(dur)
                                                            .size(10.5)
                                                            .color(colors.muted),
                                                    );
                                                }
                                            },
                                        );
                                        if show_tokens_col {
                                            ui.add_space(6.0);
                                            ui.allocate_ui_with_layout(
                                                Vec2::new(tok_col_w, 19.0),
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    let tok = if agent.tokens > 0 {
                                                        format_token_count(agent.tokens)
                                                    } else {
                                                        String::new()
                                                    };
                                                    ui.label(
                                                        RichText::new(tok)
                                                            .size(10.5)
                                                            .color(colors.dim),
                                                    );
                                                },
                                            );
                                        }
                                        ui.add_space(6.0);
                                        let left_w = ui.available_width().max(40.0);
                                        ui.allocate_ui_with_layout(
                                            Vec2::new(left_w, 19.0),
                                            Layout::left_to_right(Align::Center),
                                            |ui| {
                                                ui.set_max_width(left_w);
                                                draw_state_glyph(ui, agent.state);
                                                let clickable = agent.agent_id.is_some();
                                                let resp = ui
                                                    .add(
                                                        egui::Label::new(
                                                            RichText::new(&agent.label)
                                                                .size(11.0)
                                                                .color(if clickable {
                                                                    colors.text
                                                                } else {
                                                                    colors.muted
                                                                }),
                                                        )
                                                        .truncate()
                                                        .sense(if clickable {
                                                            Sense::click()
                                                        } else {
                                                            Sense::hover()
                                                        }),
                                                    )
                                                    .on_hover_cursor(if clickable {
                                                        egui::CursorIcon::PointingHand
                                                    } else {
                                                        egui::CursorIcon::Default
                                                    });
                                                if clickable
                                                    && resp.clicked()
                                                    && let Some(aid) = &agent.agent_id
                                                {
                                                    action = WorkflowUiAction::OpenAgent(
                                                        aid.clone(),
                                                    );
                                                }
                                            },
                                        );
                                    },
                                );
                            }
                        });
                        ui.add_space(2.0);
                    }
                }

                // Optional verification Checks phase.
                if !wf.checks.is_empty() {
                    let check_phase_idx = wf.phases.len();
                    let open = expanded_phases.contains(&(wf.id.clone(), check_phase_idx));
                    let c_done = wf
                        .checks
                        .iter()
                        .filter(|c| c.state == WorkflowCellState::Done)
                        .count();
                    let c_failed = wf
                        .checks
                        .iter()
                        .filter(|c| c.state == WorkflowCellState::Error)
                        .count();
                    let header_w = ui.available_width();
                    let resp = ui
                        .allocate_ui_with_layout(
                            Vec2::new(header_w, 22.0),
                            Layout::right_to_left(Align::Center),
                            |ui| {
                                ui.set_max_width(header_w);
                                ui.label(
                                    RichText::new(format!("{c_done}/{}", wf.checks.len()))
                                        .size(11.0)
                                        .color(colors.muted),
                                );
                                if c_failed > 0 {
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(format!("{c_failed} failed ·"))
                                            .size(11.0)
                                            .color(colors.red),
                                    );
                                }
                                ui.add_space(6.0);
                                let left_w = ui.available_width().max(40.0);
                                ui.allocate_ui_with_layout(
                                    Vec2::new(left_w, 24.0),
                                    Layout::left_to_right(Align::Center),
                                    |ui| {
                                        let (chev, _) = ui.allocate_exact_size(
                                            Vec2::splat(11.0),
                                            Sense::hover(),
                                        );
                                        icons::chevron(ui, chev, open, colors.dim);
                                        ui.label(
                                            RichText::new("Verification checks")
                                                .size(11.5)
                                                .color(colors.muted),
                                        );
                                    },
                                );
                            },
                        )
                        .response
                        .interact(Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if resp.clicked() {
                        action = WorkflowUiAction::TogglePhase(wf.id.clone(), check_phase_idx);
                    }
                    if open {
                        ui.indent(("wf-checks", &wf.id), |ui| {
                            for check in &wf.checks {
                                ui.horizontal(|ui| {
                                    draw_state_glyph(ui, check.state);
                                    let resp = ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(&check.id)
                                                    .size(11.0)
                                                    .color(colors.text),
                                            )
                                            .truncate()
                                            .frame(false),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text(&check.command);
                                    if resp.clicked()
                                        && let Some(sid) = &check.shell_id
                                    {
                                        action = WorkflowUiAction::OpenShell(sid.clone());
                                    }
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if check.state == WorkflowCellState::Error {
                                            ui.label(
                                                RichText::new("Failed")
                                                    .size(10.5)
                                                    .color(colors.red),
                                            );
                                        } else if let Some(ms) = check.duration_ms {
                                            ui.label(
                                                RichText::new(format_task_duration(
                                                    Duration::from_millis(ms),
                                                ))
                                                .size(10.5)
                                                .color(colors.muted),
                                            );
                                        }
                                    });
                                });
                            }
                        });
                    }
                }
            });
        });
    action
}
