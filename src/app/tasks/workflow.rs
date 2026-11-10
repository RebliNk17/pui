//! Bounded inspector workflow. Other sessions are secondary, never a flat chat dump.
use super::*;

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod tests;

impl Pui {
    pub(in crate::app) fn task_workflow(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        let Some(task) = self.tasks.current().cloned() else {
            ui.label(theme::caption(ui, "VERIFIED TASKS"));
            ui.add(
                egui::Label::new(
                    RichText::new("Build with a clear finish line")
                        .size(14.0)
                        .strong(),
                )
                .wrap(),
            );
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    "Agree on checks, implement, then independently verify the result.",
                ))
                .wrap(),
            );
            if ui.button("New task").clicked() {
                self.new_task();
            }
            self.workflow_agents(ui);
            return;
        };
        ui.label(theme::caption(
            ui,
            if self.demo {
                "DEMO · VERIFIED TASK"
            } else {
                "VERIFIED TASK"
            },
        ));
        let objective = if task.objective.is_empty() {
            "Untitled task"
        } else {
            &task.objective
        };
        if line(ui, objective, 13.0, colors.text, true).clicked() {
            self.tasks.open = true;
            self.tasks.selected_run = None;
        }
        line(
            ui,
            task.phase.label(),
            11.0,
            phase_color(task.phase, colors),
            false,
        );
        let model = task
            .runs
            .iter()
            .rev()
            .find_map(|r| r.selection.as_ref())
            .map(|s| format!("{} · {}", s.model, thinking_label(&s.thinking)))
            .unwrap_or_else(|| format!("{} · {}", task.model, thinking_label(&task.thinking)));
        line(ui, &model, 10.0, colors.muted, false);
        if let Some(route) = &task.pending_route {
            line(
                ui,
                &format!("Approval needed: {}", route.selection.model),
                10.0,
                colors.amber,
                false,
            );
        }
        line(
            ui,
            &format!("${:.3} reported · up to 2 repairs", task.cost()),
            10.0,
            colors.dim,
            false,
        );
        ui.add_space(8.0);
        let proposal = task.runs.iter().rev().find(|r| r.role == Role::Proposal);
        let status = if task.approved.is_some() {
            "Approved"
        } else if task.phase == Phase::AwaitingApproval {
            "Needs your approval"
        } else {
            proposal.map(|r| r.status.as_str()).unwrap_or("Not started")
        };
        if step(
            ui,
            1,
            "Acceptance checks",
            status,
            phase_color(task.phase, colors),
            self.tasks.open && self.tasks.selected_run.as_ref() == proposal.map(|r| &r.id),
        )
        .clicked()
        {
            self.tasks.open = true;
            self.tasks.selected_run = proposal.map(|r| r.id.clone());
        }
        // The current attempt is the useful overview; previous attempts remain in Results & evidence.
        let attempt = task.latest();
        if let Some(attempt) = attempt {
            line(
                ui,
                &format!(
                    "Attempt {} of 3{}",
                    attempt.number + 1,
                    if attempt.invalidated {
                        " · evidence stale"
                    } else {
                        ""
                    }
                ),
                10.0,
                colors.muted,
                false,
            );
        }
        for (index, role) in [Role::Builder, Role::Checks, Role::Verifier]
            .into_iter()
            .enumerate()
        {
            let run = attempt.and_then(|a| {
                task.runs
                    .iter()
                    .rev()
                    .find(|r| r.role == role && r.attempt == a.number)
            });
            let status = run
                .map(|r| {
                    if self
                        .tasks
                        .processes
                        .iter()
                        .any(|p| p.run == r.id && !p.ended && !p.state.dialogs.is_empty())
                    {
                        "Waiting for input"
                    } else {
                        &r.status
                    }
                })
                .unwrap_or("Not started");
            let selected = self.tasks.open
                && run.is_some_and(|r| self.tasks.selected_run.as_ref() == Some(&r.id));
            if step(
                ui,
                index + 2,
                role.label(),
                status,
                status_color(status, colors),
                selected,
            )
            .clicked()
            {
                self.tasks.open = true;
                self.tasks.selected_run = if role == Role::Checks {
                    None
                } else {
                    run.map(|r| r.id.clone())
                };
                self.tasks.transcript = Default::default();
            }
        }
        if step(
            ui,
            5,
            "Result & evidence",
            task.phase.label(),
            phase_color(task.phase, colors),
            false,
        )
        .clicked()
        {
            self.tasks.open = true;
            self.tasks.selected_run = None;
        }
        ui.add_space(8.0);
        if ui.button("Open workflow").clicked() {
            self.tasks.open = true;
        }
        self.task_routing_details(ui, &task);
        self.workflow_agents(ui);
    }

    fn workflow_agents(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        let active = self.agents.active.clone();
        let root = self
            .agents
            .nodes
            .get(&active)
            .and_then(|n| n.parent.clone())
            .unwrap_or_else(|| active.clone());
        let mut nodes: Vec<_> = self
            .agents
            .nodes
            .values()
            .map(|n| {
                (
                    n.id.clone(),
                    n.parent.clone(),
                    n.name.clone(),
                    n.status.clone(),
                    n.color(colors),
                )
            })
            .collect();
        // Current root first, direct workers next, unrelated sessions only on request.
        nodes.sort_by_key(|(id, parent, ..)| {
            (
                if id == &root {
                    0
                } else if parent.as_ref() == Some(&root) {
                    1
                } else {
                    2
                },
                id.clone(),
            )
        });
        let (current, other): (Vec<_>, Vec<_>) = nodes
            .into_iter()
            .partition(|(id, parent, ..)| id == &root || parent.as_ref() == Some(&root));
        if !current.is_empty() {
            ui.add_space(12.0);
            ui.separator();
            ui.label(theme::caption(ui, "CURRENT SESSION"));
            for (id, parent, name, status, color) in current {
                if agent_row(ui, &name, &status, color, id == active, parent.is_some()).clicked() {
                    self.view_agent(&id);
                }
            }
        }
        if !other.is_empty() {
            ui.add_space(8.0);
            let label = format!("Other sessions · {}", other.len());
            egui::CollapsingHeader::new(label)
                .id_salt("workflow-other-sessions")
                .default_open(false)
                .show(ui, |ui| {
                    for (id, parent, name, status, color) in other {
                        if agent_row(ui, &name, &status, color, id == active, parent.is_some())
                            .clicked()
                        {
                            self.view_agent(&id);
                        }
                    }
                });
        }
    }
}

fn phase_color(phase: Phase, colors: &theme::Palette) -> Color32 {
    match phase {
        Phase::Ready | Phase::Accepted => colors.green,
        Phase::Failed => colors.red,
        Phase::AwaitingApproval | Phase::Blocked | Phase::Stale => colors.amber,
        p if p.active() => colors.accent,
        _ => colors.muted,
    }
}
fn status_color(status: &str, colors: &theme::Palette) -> Color32 {
    match status {
        "Working" | "Connecting" => colors.accent,
        "Waiting for input" | "Blocked" | "Interrupted" => colors.amber,
        "Finished" => colors.green,
        "Failed" => colors.red,
        _ => colors.dim,
    }
}
fn paint_line(ui: &egui::Ui, rect: egui::Rect, text: &str, size: f32, color: Color32) {
    let mut job =
        egui::text::LayoutJob::simple_singleline(text.into(), FontId::proportional(size), color);
    job.wrap.max_width = rect.width().max(0.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        rect.left_center() - egui::vec2(0.0, galley.size().y / 2.0),
        galley,
        color,
    );
}
fn line(
    ui: &mut egui::Ui,
    text: &str,
    size: f32,
    color: Color32,
    clickable: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), size + 9.0),
        if clickable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    paint_line(ui, rect, text, size, color);
    response.on_hover_text(text)
}
fn step(
    ui: &mut egui::Ui,
    number: usize,
    label: &str,
    status: &str,
    color: Color32,
    selected: bool,
) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 46.0), Sense::click());
    ui.painter().rect_filled(
        rect,
        7,
        if selected {
            colors.accent_bg
        } else if response.hovered() {
            colors.hover
        } else {
            colors.surface
        },
    );
    let center = rect.left_center() + egui::vec2(16.0, 0.0);
    ui.painter()
        .circle_filled(center, 10.0, color.gamma_multiply(0.12));
    ui.painter().text(
        center,
        egui::Align2::CENTER_CENTER,
        number.to_string(),
        FontId::proportional(10.0),
        color,
    );
    let text_rect = egui::Rect::from_min_max(
        rect.min + egui::vec2(34.0, 5.0),
        rect.max - egui::vec2(9.0, 5.0),
    );
    paint_line(
        ui,
        egui::Rect::from_min_max(
            text_rect.min,
            egui::pos2(text_rect.right(), text_rect.center().y),
        ),
        label,
        11.5,
        colors.text,
    );
    paint_line(
        ui,
        egui::Rect::from_min_max(
            egui::pos2(text_rect.left(), text_rect.center().y),
            text_rect.max,
        ),
        status,
        10.0,
        color,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{label}: {status}"))
    });
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("{label}\n{status}"))
}
fn agent_row(
    ui: &mut egui::Ui,
    name: &str,
    status: &str,
    color: Color32,
    selected: bool,
    indented: bool,
) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let width = ui.available_width();
    let indent = if indented { 12.0 } else { 0.0 };
    let (slot, response) = ui.allocate_exact_size(Vec2::new(width, 42.0), Sense::click());
    let rect = egui::Rect::from_min_max(slot.min + egui::vec2(indent, 0.0), slot.max);
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            7,
            if selected {
                colors.accent_bg
            } else {
                colors.hover
            },
        );
    }
    ui.painter()
        .circle_filled(rect.left_center() + egui::vec2(8.0, 0.0), 2.5, color);
    let text_rect = egui::Rect::from_min_max(
        rect.min + egui::vec2(19.0, 4.0),
        rect.max - egui::vec2(6.0, 4.0),
    );
    paint_line(
        ui,
        egui::Rect::from_min_max(
            text_rect.min,
            egui::pos2(text_rect.right(), text_rect.center().y),
        ),
        name,
        11.5,
        colors.text,
    );
    paint_line(
        ui,
        egui::Rect::from_min_max(
            egui::pos2(text_rect.left(), text_rect.center().y),
            text_rect.max,
        ),
        status,
        10.0,
        colors.muted,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{name}: {status}"))
    });
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("{name}\n{status}"))
}
