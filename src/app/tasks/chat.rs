//! Task controls live in the conversation, not a second workspace.
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
        self.kanban.open = false;
        if self.tasks.open {
            self.task_parent_chat();
        } else if let Some(id) = self.task_for_chat().map(|t| t.id.clone()) {
            self.open_task_from_board(id, None);
        } else {
            self.new_task();
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
                    ui.label(theme::caption(ui, "TASK"));
                    ui.add(
                        egui::Label::new(
                            RichText::new(if task.objective.is_empty() {
                                "New task"
                            } else {
                                &task.objective
                            })
                            .strong(),
                        )
                        .wrap(),
                    );
                    ui.horizontal_wrapped(|ui| {
                        ui.label(theme::small(ui, task.phase.label()));
                        if ui
                            .button(if task.phase == Phase::AwaitingApproval {
                                "Review checks"
                            } else {
                                "Open task"
                            })
                            .clicked()
                        {
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
            if ui.small_button("New task").clicked() {
                self.new_task();
            }
            ui.menu_button("More", |ui| {
                if let Some(id) = self.tasks.selected.clone() {
                    if ui
                        .button(RichText::new("Delete task…").color(colors.red))
                        .clicked()
                    {
                        self.request_delete(deletion::Target::Task(id));
                        ui.close();
                    }
                }
            });
        });
        if self.tasks.running() && self.tasks.live != self.tasks.selected {
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::small(ui, "Another task is active."));
                if ui.button("Open active task").clicked() {
                    if let Some(id) = self.tasks.live.clone() {
                        self.open_task_from_board(id, None);
                    }
                }
            });
        }
    }

    fn task_picker(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width().clamp(80.0, 320.0);
        let context = self.workflow_task();
        let mut selected = context.map(|t| t.id.clone());
        let title = context
            .map(|t| {
                if t.objective.is_empty() {
                    "New task"
                } else {
                    &t.objective
                }
            })
            .unwrap_or("Choose task");
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
                                if row.on_hover_text(&label).clicked() {
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
            .inner
            .on_hover_text(title);
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
                title: "Task conversation".into(),
                created: None,
                modified: std::time::SystemTime::UNIX_EPOCH,
            });
        }
    }

    pub(in crate::app) fn task_content(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        let Some(task) = self.tasks.current().cloned() else {
            ui.label("Start a new task to agree on a finish line.");
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
        if task.phase == Phase::Draft {
            self.task_draft(ui, &task);
            return;
        }
        ui.label(theme::caption(ui, "TASK"));
        ui.add(egui::Label::new(RichText::new(&task.objective).size(18.0).strong()).wrap());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(task.phase.label()).color(colors.accent));
            ui.label(theme::small(ui, format!("${:.3} reported", task.cost())));
            if task.phase.active()
                && ui
                    .add_enabled(!self.demo, egui::Button::new("Stop task"))
                    .clicked()
            {
                self.tasks.stop();
            }
        });
        if !task.note.is_empty() {
            ui.add(egui::Label::new(theme::small(ui, &task.note)).wrap());
        }
        if task.phase.active() && self.tasks.live.as_ref() != Some(&task.id) && !self.demo {
            return;
        }
        self.task_dialog(ui);
        if let Some(pending) = &task.pending_route {
            prompt_ui::inline(ui.ctx()).show(ui, |ui| {
                prompt_ui::style(ui);
                prompt_ui::heading(ui, "Exceptional model requested", &format!("{} / {}\n{}", pending.selection.provider, pending.selection.model, pending.selection.reason));
                ui.horizontal_wrapped(|ui| {
                    if prompt_ui::button(ui, "Approve exceptional model · uses provider", prompt_ui::Button::Primary, !self.demo).clicked() { self.task_approve_exceptional(); }
                    if ui.button("Decline").clicked() {
                        if let Some(t) = self.tasks.current_mut() { t.pending_route = None; t.note = "Exceptional routing declined. No model call was made; start a new task to revise scope or policy.".into(); }
                        self.tasks.save();
                    }
                });
            });
        }
        ui.add_space(16.0);
        if task.phase == Phase::AwaitingApproval {
            self.task_contract(ui, &task);
        } else {
            self.task_results(ui, &task);
        }
        ui.add_space(12.0);
        self.task_routing_details(ui, &task);
    }

    fn task_draft(&mut self, ui: &mut egui::Ui, task: &Task) {
        ui.label(theme::caption(ui, "NEW TASK"));
        ui.label(RichText::new("What should change?").size(22.0).strong());
        ui.add(
            egui::Label::new(theme::small(
                ui,
                "Define the outcome. Review the checks before any implementation starts.",
            ))
            .wrap(),
        );
        ui.add_space(12.0);
        let t = self.tasks.current_mut().unwrap();
        ui.add(
            egui::TextEdit::multiline(&mut t.objective)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .char_limit(16000)
                .hint_text("Describe the behavior you want…"),
        );
        ui.add_space(8.0);
        crate::section::Section::new("Constraints & out of scope")
            .summary("Compatibility, boundaries and protected behavior")
            .icon(Icon::Shield)
            .id_salt((&task.id, "constraints"))
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut t.constraints)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .char_limit(12000)
                        .hint_text("Compatibility, protected behavior, design constraints…"),
                );
            });
        crate::section::Section::new("Workspace & model settings").summary("Execution location, routing and spending").icon(Icon::Sliders).id_salt((&task.id, "settings")).show(ui, |ui| {
            ui.add(egui::Label::new(task.source.display().to_string()).selectable(true).wrap());
            ui.add(egui::Label::new(theme::small(ui, "Git: isolated worktree. Non-Git: work in place. Uncommitted changes are never silently copied or omitted.")).wrap());
            if ui.button("Use current chat workspace and model").clicked() {
                let source = self.prefs.cwd.clone(); let provider = self.state.provider.clone(); let model = self.state.model.clone(); let thinking = self.state.thinking.clone();
                if let Some(t) = self.tasks.current_mut() { t.source = source; t.provider = provider; t.model = model; t.thinking = thinking; }
                self.tasks.save();
            }
            let t = self.tasks.current_mut().unwrap();
            ui.checkbox(&mut t.dynamic_routing, "Choose a model for each stage");
            let mut limited = t.spend_limit.is_some();
            if ui.checkbox(&mut limited, "Stop at a reported spend threshold").changed() { t.spend_limit = limited.then_some(10.0); }
            if let Some(limit) = &mut t.spend_limit { ui.add(egui::DragValue::new(limit).range(0.1..=1000.0).prefix("$ ").speed(0.5)); }
            ui.add(egui::Label::new(theme::small(ui, if t.dynamic_routing { "Haiku assesses each stage. Easy work uses Haiku; harder work uses Opus 5.5 or GPT-6.1 Sol. Fable/Astra always needs separate approval." } else { "Pinned to the chat model. No automatic substitution." })).wrap());
            ui.add(egui::Label::new(theme::small(ui, format!("{} / {} · {}", t.provider, t.model, thinking_label(&t.thinking)))).wrap());
        });
        ui.add_space(12.0);
        if prompt_ui::button(
            ui,
            "Generate acceptance checks",
            prompt_ui::Button::Primary,
            !self.demo && self.tasks.loaded.is_none() && self.tasks.live.is_none(),
        )
        .clicked()
        {
            self.task_propose();
        }
        ui.add(egui::Label::new(theme::small(ui, "Uses provider credits for routing and proposal only. Implementation waits for your approval. Reported spending thresholds are not a hard cap.")).wrap());
        if self.tasks.current().is_some_and(|t| {
            t.objective != task.objective
                || t.constraints != task.constraints
                || t.dynamic_routing != task.dynamic_routing
                || t.spend_limit != task.spend_limit
                || t.source != task.source
        }) {
            self.tasks.save();
        }
    }

    fn task_contract(&mut self, ui: &mut egui::Ui, task: &Task) {
        let colors = theme::palette(ui.ctx());
        ui.label(RichText::new("Here's the finish line").size(16.0).strong());
        ui.add(
            egui::Label::new(theme::small(
                ui,
                "Review the requirements and exact commands. Edit any check before approving.",
            ))
            .wrap(),
        );
        let t = self.tasks.current_mut().unwrap();
        for criterion in &mut t.proposed.criteria {
            ui.add_space(10.0);
            egui::Frame::new()
                .fill(colors.surface)
                .stroke(Stroke::new(1.0, colors.border))
                .corner_radius(10)
                .inner_margin(14)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(theme::caption(ui, &criterion.id));
                    ui.add(
                        egui::Label::new(&criterion.description)
                            .selectable(true)
                            .wrap(),
                    );
                    if criterion.command.trim().is_empty() {
                        ui.label(
                            RichText::new("Review only · no executable check")
                                .size(11.0)
                                .color(colors.amber),
                        );
                    } else {
                        ui.add_space(6.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(&criterion.command)
                                    .monospace()
                                    .size(11.0)
                                    .color(colors.muted),
                            )
                            .selectable(true)
                            .wrap(),
                        );
                    }
                    crate::section::Section::new("Edit check")
                        .icon(Icon::Edit)
                        .id_salt((&task.id, &criterion.id, "edit"))
                        .show(ui, |ui| {
                            ui.label(theme::small(ui, "Requirement"));
                            ui.add(
                                egui::TextEdit::multiline(&mut criterion.description)
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY)
                                    .char_limit(4000),
                            );
                            ui.label(theme::small(ui, "Exact command · blank means review only"));
                            ui.add(
                                egui::TextEdit::multiline(&mut criterion.command)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY)
                                    .char_limit(4000),
                            );
                        });
                });
        }
        if task.dirty_source {
            ui.add_space(12.0);
            ui.label(RichText::new("Source has uncommitted changes").color(colors.amber));
            ui.checkbox(
                &mut self.tasks.dirty_confirmed,
                "Use committed HEAD only; leave uncommitted changes out of this task",
            );
            ui.add(egui::Label::new(theme::small(ui, "Otherwise cancel and prepare the source yourself. Pui will not commit or copy it for you.")).wrap());
        }
        ui.add_space(16.0);
        ui.add(egui::Label::new(theme::small(ui, "Approval starts paid implementation, checks and fresh verification, with up to two repair rounds. Commands still use Pi's permission gate and can execute project code; this is not an OS sandbox. Fable/Astra need separate approval. Each process has a 30-minute deadline. No automatic commits, merges or deployments.")).wrap());
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if prompt_ui::button(
                ui,
                "Approve checks & start",
                prompt_ui::Button::Primary,
                !self.demo,
            )
            .clicked()
            {
                self.task_start();
            }
            if ui.button("Cancel task").clicked() {
                if let Some(t) = self.tasks.current_mut() {
                    t.phase = Phase::Cancelled;
                    t.note = "Cancelled before implementation.".into();
                }
                self.tasks.save();
            }
        });
        if self.tasks.current().is_some_and(|t| {
            serde_json::to_value(&t.proposed).ok() != serde_json::to_value(&task.proposed).ok()
        }) {
            self.tasks.save();
        }
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
        if matches!(task.phase, Phase::Ready | Phase::Accepted) {
            ui.horizontal_wrapped(|ui| {
                if task.phase == Phase::Ready
                    && ui
                        .add_enabled(
                            self.tasks.live.is_none() && !self.demo,
                            egui::Button::new("Accept this result"),
                        )
                        .clicked()
                {
                    self.task_fingerprint(FingerprintStep::Accept);
                }
                if matches!(task.phase, Phase::Ready | Phase::Accepted)
                    && ui
                        .add_enabled(
                            self.tasks.live.is_none() && !self.demo,
                            egui::Button::new("Revalidate source"),
                        )
                        .clicked()
                {
                    self.task_fingerprint(FingerprintStep::Revalidate);
                }
            });
        }
        if let Some(contract) = &task.approved {
            crate::section::Section::new("Approved acceptance checks")
                .id_salt((&task.id, "approved-checks"))
                .summary(format!(
                    "{} checks · agreed before implementation",
                    contract.criteria.len()
                ))
                .icon(Icon::Board)
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
