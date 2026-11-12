//! One session-owned work item. Navigation is presentation, never authorization.
use super::*;
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::app) enum Tab {
    #[default]
    Overview,
    Evidence,
    Changes,
    Activity,
}
impl Tab {
    const ALL: [Self; 4] = [
        Self::Overview,
        Self::Evidence,
        Self::Changes,
        Self::Activity,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Evidence => "Evidence",
            Self::Changes => "Changes",
            Self::Activity => "Activity",
        }
    }
}
#[derive(Default)]
pub(in crate::app) struct View {
    pub tab: Tab,
    permission_open: bool,
    activity_run: String,
    activity_width: f32,
    activity_heights: BTreeMap<usize, (u64, f32)>,
    activity_rendered: usize,
    preview: changes_preview::Preview,
}

#[cfg(test)]
#[path = "work_item_tests.rs"]
mod tests;

impl Pui {
    fn work_item_waiting(&self, task: &Task) -> bool {
        self.tasks.live.as_ref() == Some(&task.id)
            && self.tasks.processes.iter().any(|p| {
                !p.ended && task.runs.iter().any(|r| r.id == p.run) && !p.state.dialogs.is_empty()
            })
    }

    pub(in crate::app) fn task_content(&mut self, ui: &mut egui::Ui) {
        let Some(task) = self.tasks.current().cloned() else {
            return;
        };
        let colors = theme::palette(ui.ctx());
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.spacing_mut().interact_size.y = 24.0;
        let mut view = self.tasks.views.remove(&task.id).unwrap_or_default();
        if self.demo {
            ui.label(
                RichText::new("DEMO · no execution")
                    .size(10.0)
                    .color(colors.amber),
            );
        }
        if let Some(error) = &self.tasks.error {
            ui.add(egui::Label::new(RichText::new(error).color(colors.red)).wrap());
        }
        egui::Frame::new().fill(colors.surface).stroke(Stroke::new(1.0, colors.border))
            .corner_radius(14).inner_margin(18).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(theme::caption(ui, "WORK ITEM"));
                ui.add(egui::Label::new(RichText::new(&task.objective).size(20.0).strong()).wrap());
                ui.horizontal_wrapped(|ui| {
                    ui.label(theme::small(ui, if self.task_in_chat(&task) { "Attached to this conversation" } else { "Viewing retained workflow · use Conversation to return to its owner" }));
                    ui.label(theme::small(ui, if task.workspace.is_none() { "Workspace preparation pending" } else if task.isolated { "Isolated worktree" } else { "Working in place · non-Git" }));
                    ui.label(theme::small(ui, format!("${:.3} reported", task.cost().max(0.0))));
                });
                ui.add_space(12.0);
                self.work_item_status(ui, &task, &mut view);
                ui.add_space(14.0);
                let previous_tab = view.tab;
                tabs(ui, &task.id, &mut view.tab);
                if view.tab != previous_tab && view.tab != Tab::Activity { self.tasks.selected_run = None; }
                ui.add_space(12.0);
                if self.work_item_waiting(&task) && view.permission_open { self.task_dialog(ui); ui.add_space(12.0); }
                match view.tab {
                    Tab::Overview => self.work_item_overview(ui, &task, &mut view),
                    Tab::Evidence => {
                        self.work_item_revalidation(ui, &task);
                        evidence(ui, &task);
                    },
                    Tab::Changes => {
                        self.task_changes_preview(ui, &task, &mut view.preview);
                        changes(ui, &task);
                    },
                    Tab::Activity => self.work_item_activity(ui, &task, &mut view),
                }
                if matches!(task.phase, Phase::Blocked | Phase::Interrupted | Phase::Failed | Phase::Stale | Phase::Cancelled) {
                    self.work_item_recovery(ui, &task);
                }
                ui.add_space(14.0);
                ui.separator();
                ui.add(egui::Label::new(theme::small(ui, format!("{} · tool permissions stay separate", task.latest().map(|a| format!("Attempt {} of at most 3", a.number + 1)).unwrap_or_else(|| "No implementation attempt yet".into())))).wrap());
                crate::section::Section::new("Models & technical details").id_salt((&task.id, "work-item-details"))
                    .summary("Workspace, source identity and model choices").icon(Icon::Sliders).show(ui, |ui| {
                        ui.add(egui::Label::new(task.workspace.as_ref().unwrap_or(&task.source).display().to_string()).selectable(true).wrap());
                        if ui.button("Copy workspace path").clicked() { ui.ctx().copy_text(task.workspace.as_ref().unwrap_or(&task.source).display().to_string()); }
                        if let Some(fingerprint) = task.latest().and_then(|a| a.fingerprint.as_ref()) {
                            ui.add(egui::Label::new(theme::small(ui, format!("Source SHA-256: {fingerprint}"))).selectable(true).wrap());
                            ui.add(egui::Label::new(theme::small(ui, "Evidence covers the captured source, not an external-process lock. Source edits require revalidation.")).wrap());
                        }
                        self.task_routing_details(ui, &task);
                    });
            });
        if self.task_in_chat(&task) {
            if let Some(message) = self
                .state
                .messages
                .iter()
                .rev()
                .find(|m| !m.text.is_empty())
            {
                ui.add_space(12.0);
                ui.label(theme::caption(ui, "LATEST IN THIS CONVERSATION"));
                ui.add(
                    egui::Label::new(format!(
                        "{}{}",
                        message.text.chars().take(1200).collect::<String>(),
                        if message.text.chars().count() > 1200 {
                            "… · full message in Conversation"
                        } else {
                            ""
                        }
                    ))
                    .wrap(),
                );
            }
        }
        self.tasks.views.insert(task.id, view);
    }

    fn work_item_status(&mut self, ui: &mut egui::Ui, task: &Task, view: &mut View) {
        let colors = theme::palette(ui.ctx());
        let waiting = self.work_item_waiting(task);
        let (title, explanation) = status(task.phase, waiting);
        let explanation = if matches!(task.phase, Phase::Ready | Phase::Accepted) && !task.isolated
        {
            "Checks and review passed for the captured candidate. Non-Git work runs in place, so edits may already be present; nothing was committed or deployed."
        } else {
            explanation
        };
        let warning = waiting
            || matches!(
                task.phase,
                Phase::Blocked | Phase::Interrupted | Phase::Failed | Phase::Stale
            );
        egui::Frame::new().fill(if warning { colors.amber.gamma_multiply(0.10) } else { colors.accent_bg })
            .corner_radius(10).inner_margin(14).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(RichText::new(title).size(14.0).strong()).wrap());
                ui.add(egui::Label::new(theme::small(ui, explanation)).wrap());
                if !task.note.is_empty() { ui.add(egui::Label::new(theme::small(ui, &task.note)).wrap()); }
                if let Some(route) = &task.pending_route {
                    ui.add(egui::Label::new(theme::small(ui, format!("Exceptional routing is blocked: {} / {}. {}", route.selection.provider, route.selection.model, route.selection.reason))).wrap());
                }
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if waiting {
                        if ui.button(if view.permission_open { "Hide permission details" } else { "Review permission" }).clicked() { view.permission_open = !view.permission_open; }
                    } else {
                        let action = match task.phase {
                            Phase::Ready | Phase::Accepted => Some(("Review changes", Tab::Changes)),
                            Phase::Blocked | Phase::Failed | Phase::Stale => Some(("Review evidence", Tab::Evidence)),
                            Phase::Interrupted | Phase::Cancelled => Some(("Inspect retained work", Tab::Activity)),
                            Phase::Draft | Phase::AwaitingApproval => Some(("Review proposed checks", Tab::Evidence)),
                            _ => None,
                        };
                        if let Some((label, tab)) = action {
                            if ui.button(label).clicked() { view.tab = tab; self.tasks.selected_run = None; }
                        }
                    }
                    if matches!(task.phase, Phase::Blocked | Phase::Interrupted | Phase::Failed | Phase::Stale) && ui.button("Copy recovery context").clicked() {
                        ui.ctx().copy_text(format!("Workflow: {}\nState: {}\nWorkspace: {}\nBlocker: {}\nInspect existing evidence before proposing scoped recovery. Do not replay uncertain commands, install dependencies, or apply changes without explicit permission.", task.objective, task.phase.label(), task.workspace.as_ref().unwrap_or(&task.source).display(), task.note));
                    }
                });
            });
    }

    fn work_item_revalidation(&mut self, ui: &mut egui::Ui, task: &Task) {
        let blocked = self.task_revalidate_blocked(&task.id);
        ui.add(egui::Label::new(theme::small(ui, "Source revalidation compares the retained files to the captured fingerprint only. It does not rerun commands, resume work or turn stale checks into a pass.")).wrap());
        let response = ui.add_enabled(
            blocked.is_none(),
            egui::Button::new("Revalidate retained source"),
        );
        if let Some(reason) = blocked {
            response.on_hover_text(reason);
        } else if response.clicked() {
            if let Err(error) = self.task_revalidate_retained(&task.id) {
                self.state.notice = Some(error);
            }
        }
        ui.add_space(10.0);
    }

    fn work_item_recovery(&mut self, ui: &mut egui::Ui, task: &Task) {
        let plan = self.task_recovery_plan(&task.id);
        crate::section::Section::new("Recovery options")
            .id_salt((&task.id, "recovery-options"))
            .summary(plan.summary)
            .icon(Icon::Shield)
            .show(ui, |ui| {
                ui.add(egui::Label::new(plan.detail).wrap());
                self.work_item_revalidation(ui, task);
            });
    }

    fn work_item_overview(&mut self, ui: &mut egui::Ui, task: &Task, view: &mut View) {
        ui.label(RichText::new("The finish line").strong());
        let contract = task.approved.as_ref().unwrap_or(&task.proposed);
        if contract.criteria.is_empty() {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    "Acceptance checks have not been established yet.",
                ))
                .wrap(),
            );
        }
        for criterion in &contract.criteria {
            ui.add(
                egui::Label::new(format!("{} · {}", criterion.id, criterion.description)).wrap(),
            );
        }
        ui.add_space(12.0);
        ui.label(RichText::new("Progress").strong());
        for role in [Role::Proposal, Role::Builder, Role::Checks, Role::Verifier] {
            let run = task.runs.iter().rev().find(|r| {
                r.role == role
                    && r.routing_for.is_none()
                    && (role == Role::Proposal
                        || task.latest().is_some_and(|a| a.number == r.attempt))
            });
            let summary = run.map(|r| r.status.as_str()).unwrap_or("Not started");
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(role.label()).size(12.0));
                ui.label(theme::small(ui, summary));
                if let Some(run) = run {
                    if ui
                        .small_button(if role == Role::Checks {
                            "View results"
                        } else {
                            "Details"
                        })
                        .clicked()
                    {
                        view.tab = if role == Role::Checks {
                            Tab::Evidence
                        } else {
                            Tab::Activity
                        };
                        self.tasks.selected_run = if role == Role::Checks {
                            None
                        } else {
                            Some(run.id.clone())
                        };
                    }
                }
            });
            ui.separator();
        }
        if !task.constraints.is_empty() {
            crate::section::Section::new("Constraints & out of scope")
                .id_salt((&task.id, "work-item-constraints"))
                .show(ui, |ui| {
                    ui.add(egui::Label::new(&task.constraints).selectable(true).wrap());
                });
        }
    }

    fn work_item_activity(&mut self, ui: &mut egui::Ui, task: &Task, view: &mut View) {
        ui.label(RichText::new("Stage activity").strong());
        ui.add(
            egui::Label::new(theme::small(
                ui,
                "Logs belong to this work item. Viewing them does not start or resume a stage.",
            ))
            .wrap(),
        );
        for run in &task.runs {
            let label = format!(
                "{} · {} · attempt {}",
                run.routing_for
                    .map(|r| format!("Routing → {}", r.label()))
                    .unwrap_or_else(|| run.role.label().into()),
                run.status,
                run.attempt + 1
            );
            if ui
                .add(
                    egui::Button::selectable(
                        self.tasks.selected_run.as_ref() == Some(&run.id),
                        label,
                    )
                    .wrap(),
                )
                .clicked()
            {
                self.tasks.selected_run = Some(run.id.clone());
            }
        }
        let Some(run) = self
            .tasks
            .selected_run
            .as_ref()
            .and_then(|id| task.runs.iter().find(|r| &r.id == id))
            .cloned()
        else {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    if task.runs.is_empty() {
                        "No stage activity was recorded."
                    } else {
                        "Select a stage to inspect its retained activity here."
                    },
                ))
                .wrap(),
            );
            return;
        };
        ui.add_space(10.0);
        let available = self.tasks.search_state(&run.id).is_some();
        if !available && self.tasks.preview.is_none() {
            if let Some(path) = run.session.clone() {
                let (tx, rx) = mpsc::channel();
                let ctx = ui.ctx().clone();
                self.tasks.preview = Some((run.id.clone(), rx));
                std::thread::spawn(move || {
                    let _ = tx.send(crate::history::load(&path));
                    ctx.request_repaint();
                });
            }
        }
        let target = self
            .tasks
            .activity_target
            .take()
            .filter(|(id, _)| *id == run.id);
        if view.activity_run != run.id || (view.activity_width - ui.available_width()).abs() > 0.5 {
            view.activity_heights.clear();
            view.activity_run = run.id.clone();
            view.activity_width = ui.available_width();
        }
        view.activity_rendered = 0;
        if let Some(state) = self.tasks.search_state(&run.id) {
            for (index, message) in state.messages.iter().enumerate() {
                let signature = activity_signature(message);
                let cached = view
                    .activity_heights
                    .get(&index)
                    .filter(|(sig, _)| *sig == signature)
                    .map(|(_, height)| *height);
                let y = ui.cursor().top();
                let is_target = target
                    .as_ref()
                    .is_some_and(|(_, message)| *message == index);
                if !is_target
                    && cached.is_some_and(|height| {
                        y + height < ui.clip_rect().top() - 100.0
                            || y > ui.clip_rect().bottom() + 100.0
                    })
                {
                    ui.allocate_space(Vec2::new(
                        ui.available_width(),
                        (cached.unwrap() - ui.spacing().item_spacing.y).max(0.0),
                    ));
                    continue;
                }
                view.activity_rendered += 1;
                let response = ui
                    .push_id((&run.id, index), |ui| {
                        if !message.text.is_empty() {
                            ui.label(theme::caption(ui, &message.role));
                            let structured = matches!(run.role, Role::Proposal | Role::Verifier)
                                && run.routing_for.is_none()
                                && crate::task_output::show(ui, &message.text);
                            if !structured {
                                ui.add(
                                    egui::Label::new(
                                        message.text.chars().take(48000).collect::<String>(),
                                    )
                                    .selectable(true)
                                    .wrap(),
                                );
                            }
                        }
                        for tool in &message.tools {
                            crate::tool_view::row(ui, tool);
                        }
                    })
                    .response;
                view.activity_heights
                    .insert(index, (signature, ui.cursor().top() - y));
                if is_target {
                    response.scroll_to_me(Some(Align::Center));
                }
            }
        } else if !matches!(run.role, Role::Proposal | Role::Verifier)
            || run.routing_for.is_some()
            || !crate::task_output::show(ui, &run.summary)
        {
            ui.add(
                egui::Label::new(run.summary.chars().take(48000).collect::<String>())
                    .selectable(true)
                    .wrap(),
            );
        }
        if let Some(path) = &run.session {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    format!("Full Pi session retained: {}", path.display()),
                ))
                .selectable(true)
                .wrap(),
            );
        }
    }
}

fn activity_signature(message: &Message) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    message.revision.hash(&mut hash);
    message.text.len().hash(&mut hash);
    message.tools.len().hash(&mut hash);
    for tool in &message.tools {
        tool.id.hash(&mut hash);
        tool.finished.hash(&mut hash);
        tool.failed.hash(&mut hash);
        tool.output.len().hash(&mut hash);
        tool.duration_ms.hash(&mut hash);
        for child in &tool.children {
            child.finished.hash(&mut hash);
            child.failed.hash(&mut hash);
            child.output.len().hash(&mut hash);
            child.args.len().hash(&mut hash);
        }
    }
    hash.finish()
}

fn status(phase: Phase, waiting: bool) -> (&'static str, &'static str) {
    if waiting {
        return (
            "Your permission is needed",
            "Review the exact action for this work item. Nothing is approved merely by opening these details.",
        );
    }
    match phase {
        Phase::Draft | Phase::AwaitingApproval => (
            "Not started · retained proposal",
            "This legacy workflow is read-only. Ask the agent in chat to start a new workflow.",
        ),
        Phase::Proposing => (
            "Establishing acceptance checks",
            "The agent is defining executable evidence for the agreed outcome.",
        ),
        Phase::Preparing => (
            "Preparing the workspace",
            "The execution workspace is being prepared. Implementation has not started.",
        ),
        Phase::Building => (
            "Implementing the change",
            "The builder is working against the established checks. Verification follows automatically.",
        ),
        Phase::Capturing => (
            "Capturing the candidate",
            "Binding evidence to the source before executing checks.",
        ),
        Phase::Checking => (
            "Checking the implementation",
            "The approved checks are running. Independent review follows when execution finishes.",
        ),
        Phase::Verifying => (
            "Independent review in progress",
            "A fresh reviewer is checking the requirement against the candidate and execution evidence.",
        ),
        Phase::Ready | Phase::Accepted => (
            "Completed · review the retained result",
            "Checks and review passed for the captured candidate. This did not commit, merge or apply changes to your checkout.",
        ),
        Phase::Blocked => (
            "Blocked · needs a decision",
            "No further implementation or repairs will run. Review the blocker before asking for scoped recovery; nothing will retry automatically.",
        ),
        Phase::Failed => (
            "Verification failed · work retained",
            "The repair limit was reached. Inspect the evidence and changes before deciding what to do next.",
        ),
        Phase::Interrupted => (
            "Interrupted · nothing was replayed",
            "The workspace and progress are retained. An uncertain command must not be resumed automatically.",
        ),
        Phase::Cancelled => (
            "Stopped · work retained",
            "No further stages will run. Completed file edits and external effects were not undone.",
        ),
        Phase::Stale => (
            "Evidence is stale",
            "The source changed after verification. The earlier passing result does not verify this candidate.",
        ),
    }
}

fn tabs(ui: &mut egui::Ui, task: &str, selected: &mut Tab) {
    let colors = theme::palette(ui.ctx());
    let width = ui.available_width().min(450.0);
    egui::Frame::new()
        .fill(colors.bg)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(10)
        .inner_margin(4)
        .show(ui, |ui| {
            ui.set_width((width - 10.0).max(1.0));
            ui.spacing_mut().item_spacing.x = 3.0;
            let button_width = ((ui.available_width() - 9.0) / 4.0).max(1.0);
            ui.horizontal(|ui| {
                for (index, tab) in Tab::ALL.into_iter().enumerate() {
                    let id = ui.make_persistent_id((task, "work-item-tab", index));
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(button_width, 36.0), Sense::hover());
                    let response = ui.interact(rect, id, Sense::click());
                    if response.clicked() {
                        *selected = tab;
                        response.request_focus();
                    }
                    if response.has_focus() {
                        ui.memory_mut(|m| {
                            m.set_focus_lock_filter(
                                id,
                                egui::EventFilter {
                                    horizontal_arrows: true,
                                    ..Default::default()
                                },
                            )
                        });
                        let target = ui.input_mut(|i| {
                            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                                Some((index + 1) % 4)
                            } else if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                                Some((index + 3) % 4)
                            } else if i.consume_key(egui::Modifiers::NONE, egui::Key::Home) {
                                Some(0)
                            } else if i.consume_key(egui::Modifiers::NONE, egui::Key::End) {
                                Some(3)
                            } else {
                                None
                            }
                        });
                        if let Some(target) = target {
                            *selected = Tab::ALL[target];
                            let target = ui.make_persistent_id((task, "work-item-tab", target));
                            ui.memory_mut(|m| {
                                m.request_focus(target);
                                m.set_focus_lock_filter(
                                    target,
                                    egui::EventFilter {
                                        horizontal_arrows: true,
                                        ..Default::default()
                                    },
                                );
                            });
                        }
                    }
                    let active = *selected == tab;
                    ui.painter().rect_filled(
                        rect,
                        7,
                        if active || response.hovered() {
                            colors.hover
                        } else {
                            colors.bg
                        },
                    );
                    if active {
                        ui.painter().rect_stroke(
                            rect.shrink(0.5),
                            7,
                            Stroke::new(1.0, colors.border),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if response.has_focus() {
                        ui.painter().rect_stroke(
                            rect.shrink(1.0),
                            7,
                            Stroke::new(1.5, colors.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let mut job = egui::text::LayoutJob::simple_singleline(
                        tab.label().into(),
                        FontId::proportional(if button_width < 80.0 { 11.0 } else { 12.0 }),
                        if active { colors.text } else { colors.muted },
                    );
                    job.wrap.max_width = (rect.width() - 8.0).max(1.0);
                    job.wrap.max_rows = 1;
                    let galley = ui.painter().layout_job(job);
                    ui.painter()
                        .galley(rect.center() - galley.size() / 2.0, galley, colors.text);
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::SelectableLabel,
                            ui.is_enabled(),
                            active,
                            tab.label(),
                        )
                    });
                    #[cfg(test)]
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(egui::Id::new(("work-item-tab-rect", tab.label())), rect)
                    });
                }
            });
        });
}

fn evidence(ui: &mut egui::Ui, task: &Task) {
    ui.label(RichText::new("Acceptance checks").strong());
    let contract = task.approved.as_ref().unwrap_or(&task.proposed);
    if contract.criteria.is_empty() {
        ui.label(theme::small(ui, "No acceptance checks recorded."));
    }
    for criterion in &contract.criteria {
        ui.add(egui::Label::new(format!("{} · {}", criterion.id, criterion.description)).wrap());
        ui.add(
            egui::Label::new(RichText::new(&criterion.command).monospace().size(11.0))
                .selectable(true)
                .wrap(),
        );
    }
    if task.attempts.is_empty() {
        ui.add(egui::Label::new(theme::small(ui, "No executed check or independent review yet. Proposed commands are not execution results.")).wrap());
    }
    for attempt in task.attempts.iter().rev() {
        ui.add_space(12.0);
        ui.label(
            RichText::new(format!(
                "Attempt {}{}",
                attempt.number + 1,
                if attempt.invalidated {
                    " · stale / superseded"
                } else {
                    ""
                }
            ))
            .strong(),
        );
        if attempt.checks.is_empty() {
            ui.label(theme::small(ui, "No completed check result."));
        }
        for check in &attempt.checks {
            let colors = theme::palette(ui.ctx());
            crate::section::Section::new(format!(
                "{} · {}",
                check.criterion,
                if check.passed() {
                    "Passed"
                } else {
                    "Failed / incomplete"
                }
            ))
            .id_salt((&task.id, attempt.number, &check.criterion, "work-evidence"))
            .summary(format!(
                "Exit {} · {} ms",
                check
                    .exit_code
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "unknown".into()),
                check.duration_ms
            ))
            .icon(Icon::Command)
            .status(
                if check.passed() {
                    "Passed"
                } else {
                    "Incomplete"
                },
                if check.passed() {
                    colors.green
                } else {
                    colors.amber
                },
            )
            .default_open(!check.passed())
            .show(ui, |ui| {
                approval_view::arguments(
                    ui,
                    &json!({"command":check.command,"output":check.output}),
                );
                if check.truncated {
                    ui.label(theme::small(
                        ui,
                        "Output was truncated; full output is not shown here.",
                    ));
                }
                if check.cancelled {
                    ui.label(theme::small(
                        ui,
                        "Command cancelled · not a passing result.",
                    ));
                }
            });
        }
        if let Some(review) = &attempt.review {
            ui.label(RichText::new("Independent review").strong());
            ui.add(egui::Label::new(&review.summary).selectable(true).wrap());
            for finding in &review.criteria {
                ui.add(
                    egui::Label::new(format!(
                        "{} · {:?}: {}",
                        finding.id, finding.status, finding.evidence
                    ))
                    .selectable(true)
                    .wrap(),
                );
            }
        } else {
            ui.label(theme::small(ui, "No independent review finding yet."));
        }
    }
}

fn changes(ui: &mut egui::Ui, task: &Task) {
    ui.label(RichText::new("Recorded candidate changes").strong());
    ui.add(egui::Label::new(theme::small(ui, if task.isolated { "Retained in the isolated workspace. These edits have not been applied to the original checkout." } else { "Non-Git execution works in place: edits may already be present. No commit or deployment was performed." })).wrap());
    if task.attempts.iter().all(|a| a.changes.is_empty()) {
        ui.add(egui::Label::new(theme::small(ui, "No file-tool edits were recorded. This is not a claim that the workspace is unchanged.")).wrap());
    }
    for attempt in task.attempts.iter().rev() {
        for (index, change) in attempt.changes.iter().enumerate() {
            crate::section::Section::new(&change.path).id_salt((&task.id, attempt.number, index, "work-changes"))
                .summary(format!("Attempt {} · +{} −{}{}", attempt.number + 1, change.added, change.removed, if attempt.invalidated { " · superseded" } else { "" }))
                .icon(Icon::Edit).show(ui, |ui| {
                    ui.add(egui::Label::new(theme::small(ui, "Recorded replacements, not a net Git diff. Shell side effects are not represented.")).wrap());
                    ui.add(egui::Label::new(RichText::new(&change.diff).monospace()).selectable(true).wrap());
                });
        }
    }
}
