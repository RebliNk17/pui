//! Bounded workflow progress only; conversation navigation belongs in the sidebar.
use super::*;

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod tests;

impl Pui {
    pub(in crate::app) fn task_workflow(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        let Some(task) = self.inspector_workflow().cloned() else {
            return;
        };
        ui.label(theme::caption(
            ui,
            if self.demo {
                "DEMO · WORKFLOW"
            } else {
                "WORKFLOW"
            },
        ));
        let objective = if task.objective.is_empty() {
            "Untitled workflow"
        } else {
            &task.objective
        };
        if line(ui, objective, 13.0, colors.text, true).clicked() {
            self.open_task_from_board(task.id.clone(), None);
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
                &format!("Exceptional routing blocked: {}", route.selection.model),
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
            "Established"
        } else if matches!(task.phase, Phase::Draft | Phase::AwaitingApproval) {
            "Legacy · new agent request required"
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
            self.open_task_from_board(task.id.clone(), proposal.map(|r| r.id.clone()));
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
                let run = if role == Role::Checks {
                    None
                } else {
                    run.map(|r| r.id.clone())
                };
                if role == Role::Checks {
                    self.open_work_item_tab(task.id.clone(), work_item::Tab::Evidence);
                } else {
                    self.open_task_from_board(task.id.clone(), run);
                }
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
            self.open_work_item_tab(task.id.clone(), work_item::Tab::Evidence);
        }
        self.task_routing_details(ui, &task);
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
fn paint_line(ui: &egui::Ui, rect: egui::Rect, text: &str, size: f32, color: Color32) -> bool {
    let mut job =
        egui::text::LayoutJob::simple_singleline(text.into(), FontId::proportional(size), color);
    job.wrap.max_width = rect.width().max(0.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(job);
    let elided = galley.elided;
    ui.painter().galley(
        rect.left_center() - egui::vec2(0.0, galley.size().y / 2.0),
        galley,
        color,
    );
    elided
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
    if paint_line(ui, rect, text, size, color) {
        response.on_hover_text(text)
    } else {
        response
    }
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
    let label_elided = paint_line(
        ui,
        egui::Rect::from_min_max(
            text_rect.min,
            egui::pos2(text_rect.right(), text_rect.center().y),
        ),
        label,
        11.5,
        colors.text,
    );
    let status_elided = paint_line(
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
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if label_elided || status_elided {
        response.on_hover_text(format!("{label}\n{status}"))
    } else {
        response
    }
}
