//! Read-only workflow progress lives in the conversation, not a second workspace.
use super::*;

#[cfg(test)]
#[path = "chat_tests.rs"]
mod tests;

impl Pui {
    pub(in crate::app) fn task_in_chat(&self, task: &Task) -> bool {
        if let Some(parent) = &task.parent_session {
            return self.state.session_file.as_deref().is_some_and(|path| {
                sync::identity(std::path::Path::new(path)) == sync::identity(parent)
            });
        }
        self.tasks.parents.get(&task.id) == Some(&self.agents.active)
            && !self.agents.active.is_empty()
    }

    /// Viewing a task explicitly keeps its context; ordinary chats only see
    /// tasks owned by that exact session, never the last globally selected task.
    pub(in crate::app) fn task_for_chat(&self) -> Option<&Task> {
        self.tasks
            .current()
            .filter(|t| self.task_in_chat(t))
            .or_else(|| {
                self.tasks
                    .records
                    .iter()
                    .filter(|t| self.task_in_chat(t))
                    .max_by_key(|t| t.updated)
            })
    }

    pub(in crate::app) fn workflow_task(&self) -> Option<&Task> {
        if self.tasks.open {
            self.tasks.current()
        } else {
            self.task_for_chat()
        }
    }

    pub(in crate::app) fn active_workflow(&self) -> Option<&Task> {
        if self.tasks.open {
            self.tasks.current().filter(|t| t.phase.active())
        } else {
            self.tasks
                .records
                .iter()
                .filter(|t| t.phase.active() && self.task_in_chat(t))
                .max_by_key(|t| t.updated)
        }
    }

    fn task_same_session(&self, left: &Task, right: &Task) -> bool {
        if left.id == right.id {
            return true;
        }
        match (&left.parent_session, &right.parent_session) {
            (Some(left), Some(right)) => sync::identity(left) == sync::identity(right),
            (None, None) => self
                .tasks
                .parents
                .get(&left.id)
                .is_some_and(|parent| self.tasks.parents.get(&right.id) == Some(parent)),
            _ => false,
        }
    }

    pub(in crate::app) fn toggle_chat_tasks(&mut self) {
        if !self.tasks.open && self.task_for_chat().is_none() {
            return;
        }
        self.kanban.open = false;
        self.automation.open = false;
        if self.tasks.open {
            self.task_parent_chat();
        } else if let Some(id) = self.task_for_chat().map(|t| t.id.clone()) {
            self.open_task_from_board(id, None);
        }
    }

    pub(in crate::app) fn task_chat_cards(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        let tasks: Vec<_> = self
            .tasks
            .records
            .iter()
            .filter(|t| self.task_in_chat(t))
            .cloned()
            .collect();
        for task in tasks {
            egui::Frame::new()
                .fill(colors.surface)
                .corner_radius(12)
                .inner_margin(14)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(theme::caption(ui, "WORKFLOW"));
                    ui.add(
                        egui::Label::new(
                            RichText::new(if task.objective.is_empty() {
                                "Untitled workflow"
                            } else {
                                &task.objective
                            })
                            .strong(),
                        )
                        .wrap(),
                    );
                    ui.horizontal_wrapped(|ui| {
                        ui.label(theme::small(ui, task.phase.label()));
                        if ui.button("Open workflow").clicked() {
                            self.open_task_from_board(task.id.clone(), None);
                        }
                    });
                });
            ui.add_space(12.0);
        }
    }

    pub(in crate::app) fn task_header(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        ui.horizontal_wrapped(|ui| {
            if ui.button("← Conversation").clicked() {
                self.task_parent_chat();
            }
            self.task_picker(ui);
            if self.tasks.current().is_some_and(|task| task.phase.active())
                && ui
                    .add_enabled(
                        !self.demo && self.tasks.live == self.tasks.selected,
                        egui::Button::new("Stop workflow"),
                    )
                    .clicked()
            {
                self.tasks.stop();
            }
            ui.menu_button("More", |ui| {
                theme::menu_controls(ui);
                if let Some(id) = self.tasks.selected.clone() {
                    if ui
                        .button(RichText::new("Delete workflow history…").color(colors.red))
                        .clicked()
                    {
                        self.request_delete(deletion::Target::Task(id));
                        ui.close();
                    }
                }
            });
        });
    }

    fn task_picker(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width().clamp(80.0, 320.0);
        let context = self.workflow_task();
        let mut selected = context.map(|t| t.id.clone());
        let title = context
            .map(|t| {
                if t.objective.is_empty() {
                    "Untitled workflow"
                } else {
                    &t.objective
                }
            })
            .unwrap_or("Choose workflow");
        let response = ui
            .allocate_ui_with_layout(
                Vec2::new(width, ui.spacing().interact_size.y),
                Layout::top_down(Align::LEFT),
                |ui| {
                    ui.set_width(width);
                    egui::ComboBox::from_id_salt("task-history")
                        .selected_text(title)
                        .width(width)
                        .truncate()
                        .height(300.0)
                        .show_ui(ui, |ui| {
                            // ComboBox defaults to Extend inside its popup. Bound BOTH the
                            // menu UI and each selectable label, including unbroken titles.
                            ui.set_width((width - 16.0).max(48.0));
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                            for task in self.tasks.records.iter().filter(|t| {
                                context.is_some_and(|context| self.task_same_session(t, context))
                            }) {
                                let label = format!(
                                    "{} · {}",
                                    task.phase.label(),
                                    if task.objective.is_empty() {
                                        "Untitled"
                                    } else {
                                        &task.objective
                                    }
                                );
                                let row = ui.add_sized(
                                    [ui.available_width(), 28.0],
                                    egui::Button::selectable(
                                        selected.as_ref() == Some(&task.id),
                                        &label,
                                    )
                                    .truncate(),
                                );
                                let galley = egui::WidgetText::from(label.as_str()).into_galley(
                                    ui,
                                    Some(egui::TextWrapMode::Truncate),
                                    (row.rect.width() - 2.0 * ui.spacing().button_padding.x)
                                        .max(0.0),
                                    egui::TextStyle::Button,
                                );
                                let row = if galley.elided {
                                    row.on_hover_text(&label)
                                } else {
                                    row
                                };
                                if row.clicked() {
                                    selected = Some(task.id.clone());
                                }
                            }
                            #[cfg(test)]
                            ui.ctx().data_mut(|d| {
                                d.insert_temp(
                                    egui::Id::new("task-picker-popup-rect"),
                                    ui.min_rect(),
                                )
                            });
                        })
                        .response
                },
            )
            .inner;
        let text_width = response.rect.width()
            - 2.0 * ui.spacing().button_padding.x
            - ui.spacing().icon_spacing
            - ui.spacing().icon_width;
        let galley = egui::WidgetText::from(title).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            text_width.max(0.0),
            egui::TextStyle::Button,
        );
        let response = if galley.elided {
            response.on_hover_text(title)
        } else {
            response
        };
        #[cfg(test)]
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("task-picker-rect"), response.rect));
        let _ = response;
        if selected != context.map(|t| t.id.clone()) {
            if let Some(id) = selected {
                self.open_task_from_board(id, None);
            }
        }
    }

    pub(in crate::app) fn task_parent_chat(&mut self) {
        let Some(task) = self.tasks.current().cloned() else {
            self.tasks.open = false;
            return;
        };
        self.tasks.selected_run = None;
        self.tasks.open = false;
        if self.task_in_chat(&task) {
            return;
        }
        if let Some(parent) = self.tasks.parents.get(&task.id).cloned() {
            if self.agents.active == parent || self.agents.parked.contains_key(&parent) {
                self.view_agent(&parent);
                return;
            }
        }
        if let Some(path) = task.parent_session {
            self.open_chat(Session {
                path,
                cwd: task.source.clone(),
                project: task.source,
                title: "Workflow conversation".into(),
                created: None,
                modified: std::time::SystemTime::UNIX_EPOCH,
            });
        }
    }

    pub(in crate::app) fn task_content(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        let Some(task) = self.tasks.current().cloned() else {
            ui.label("Ask the agent in chat to start a workflow.");
            return;
        };
        if let Some(error) = &self.tasks.error {
            ui.add(egui::Label::new(RichText::new(error).color(colors.red)).wrap());
        }
        if self.demo {
            ui.label(
                RichText::new("DEMO · no execution")
                    .size(10.0)
                    .color(colors.amber),
            );
        }
        ui.label(theme::caption(ui, "WORKFLOW"));
        ui.add(egui::Label::new(RichText::new(&task.objective).size(18.0).strong()).wrap());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(task.phase.label()).color(colors.accent));
            ui.label(theme::small(ui, format!("${:.3} reported", task.cost())));
        });
        if !task.note.is_empty() {
            ui.add(egui::Label::new(theme::small(ui, &task.note)).wrap());
        }
        if matches!(task.phase, Phase::Draft | Phase::AwaitingApproval) {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    "This legacy workflow is read-only. Ask the agent in chat to start a new workflow.",
                ))
                .wrap(),
            );
        }
        self.task_dialog(ui);
        if let Some(pending) = &task.pending_route {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    format!(
                        "Exceptional routing is blocked: {} / {}. Ask the agent for a new workflow using a supported model.\n{}",
                        pending.selection.provider, pending.selection.model, pending.selection.reason
                    ),
                ))
                .wrap(),
            );
        }
        ui.add_space(16.0);
        self.task_results(ui, &task);
        ui.add_space(12.0);
        self.task_routing_details(ui, &task);
    }

    fn task_results(&mut self, ui: &mut egui::Ui, task: &Task) {
        if let Some(path) = &task.workspace {
            crate::section::Section::new("Workspace")
                .id_salt((&task.id, "workspace"))
                .summary(path.display().to_string())
                .icon(Icon::Folder)
                .show(ui, |ui| {
                    ui.label(theme::small(
                        ui,
                        if task.isolated {
                            "Isolated worktree"
                        } else {
                            "Working in place · non-Git"
                        },
                    ));
                    ui.add(
                        egui::Label::new(path.display().to_string())
                            .selectable(true)
                            .wrap(),
                    );
                    if ui.button("Copy path").clicked() {
                        ui.ctx().copy_text(path.display().to_string());
                    }
                });
        }
        if !task.constraints.is_empty() {
            crate::section::Section::new("Constraints & out of scope")
                .id_salt((&task.id, "constraints"))
                .icon(Icon::Shield)
                .show(ui, |ui| {
                    ui.add(egui::Label::new(&task.constraints).selectable(true).wrap());
                });
        }
        let contract = task.approved.as_ref().unwrap_or(&task.proposed);
        if !contract.criteria.is_empty() {
            crate::section::Section::new("Acceptance checks")
                .id_salt((&task.id, "approved-checks"))
                .summary(format!("{} checks · read-only", contract.criteria.len()))
                .icon(Icon::Board)
                .default_open(matches!(task.phase, Phase::Draft | Phase::AwaitingApproval))
                .show(ui, |ui| {
                    for criterion in &contract.criteria {
                        ui.add(
                            egui::Label::new(format!(
                                "{} · {}",
                                criterion.id, criterion.description
                            ))
                            .wrap(),
                        );
                        ui.add(
                            egui::Label::new(
                                RichText::new(&criterion.command).monospace().size(11.0),
                            )
                            .selectable(true)
                            .wrap(),
                        );
                    }
                });
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
            if !attempt.note.is_empty() {
                ui.add(egui::Label::new(&attempt.note).wrap());
            }
            if let Some(fingerprint) = &attempt.fingerprint {
                crate::section::Section::new("Source fingerprint").id_salt((&task.id, attempt.number, "fingerprint")).summary("Evidence tied to this exact source candidate").icon(Icon::Shield).show(ui, |ui| {
                    ui.add(egui::Label::new(theme::small(ui, format!("SHA-256: {fingerprint}"))).selectable(true).wrap());
                    ui.add(egui::Label::new(theme::small(ui, "Git tracked + nonignored files; non-Git excludes generated directories. Not an external-process lock.")).wrap());
                });
            }
            for (index, change) in attempt.changes.iter().enumerate() {
                crate::section::Section::new(&change.path).summary(format!("Recorded edits · +{} −{}", change.added, change.removed)).icon(Icon::Edit).id_salt((&task.id, attempt.number, "patch", index)).show(ui, |ui| {
                    ui.add(egui::Label::new(theme::small(ui, "Recorded replacements, not a net Git diff. Shell side effects are not represented here.")).wrap());
                    ui.add(egui::Label::new(RichText::new(&change.diff).monospace()).selectable(true).wrap());
                });
            }
            for check in &attempt.checks {
                let colors = theme::palette(ui.ctx());
                let description = task
                    .approved
                    .as_ref()
                    .and_then(|c| c.criteria.iter().find(|c| c.id == check.criterion))
                    .map(|c| c.description.as_str())
                    .unwrap_or("Acceptance check");
                crate::section::Section::new(format!("{} · {}", check.criterion, description))
                    .summary(format!(
                        "Exit {} · {} ms · command & output",
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
                    .id_salt((&task.id, attempt.number, &check.criterion))
                    .default_open(!check.passed())
                    .show(ui, |ui| {
                        approval_view::arguments(
                            ui,
                            &json!({"command":check.command,"output":check.output}),
                        );
                    });
            }
            if let Some(review) = &attempt.review {
                ui.label(RichText::new("Independent review").strong());
                ui.add(egui::Label::new(&review.summary).wrap().selectable(true));
                for finding in &review.criteria {
                    ui.add(
                        egui::Label::new(format!(
                            "{} · {:?}: {}",
                            finding.id, finding.status, finding.evidence
                        ))
                        .wrap()
                        .selectable(true),
                    );
                }
            }
        }
    }
}
