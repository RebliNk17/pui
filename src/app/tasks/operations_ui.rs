use super::*;

impl Pui {
    pub(in crate::app) fn task_operation_actions(&mut self, ui: &mut egui::Ui, task: &Task) {
        if let Some((id, status)) = &self.tasks.operations.status {
            if *id == task.id {
                ui.add(egui::Label::new(status).selectable(true).wrap());
            }
        }
        if self.tasks.operations.busy() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(theme::small(
                    ui,
                    "Workflow action in progress · no automatic retry",
                ));
            });
        }
        if let Some(process) = &self.tasks.setup {
            if process.record.binding.task == task.id {
                ui.add(
                    egui::Label::new(theme::small(
                        ui,
                        format!(
                            "Setup {:?} · {} recorded command(s)",
                            process.record.status,
                            process.record.results.len()
                        ),
                    ))
                    .wrap(),
                );
                if ui.button("Stop setup").clicked() {
                    if let Some(process) = &mut self.tasks.setup {
                        process.cancel();
                    }
                }
                self.task_setup_permission(ui, &task.id);
                return;
            }
        }
        if self.tasks.operations.quarantined.contains(&task.id) {
            ui.add(egui::Label::new("Workflow history is quarantined after an uncertain setup write. No command or stage will restart.").wrap());
            if ui
                .add_enabled(
                    self.tasks.operations.reconciling.is_none(),
                    egui::Button::new("Reconcile retained history"),
                )
                .clicked()
            {
                self.reconcile_setup_record(&task.id);
            }
        }
        let enabled = self.task_operation_guard(task).is_ok();
        ui.horizontal_wrapped(|ui| {
            if task.implementation_pending
                && ui
                    .add_enabled(enabled, egui::Button::new("Review initial implementation…"))
                    .clicked()
            {
                self.request_initial_resume(&task.id);
            }
            if ui
                .add_enabled(
                    enabled && task.latest().is_some(),
                    egui::Button::new("Review check rerun…"),
                )
                .clicked()
            {
                self.request_check_rerun(&task.id);
            }
            if ui
                .add_enabled(
                    enabled
                        && task.isolated
                        && matches!(task.phase, Phase::Blocked | Phase::Interrupted),
                    egui::Button::new("Review environment setup…"),
                )
                .clicked()
            {
                self.request_environment_setup(&task.id);
            }
            if ui
                .add_enabled(
                    enabled && task.isolated,
                    egui::Button::new("Inspect apply recovery"),
                )
                .clicked()
            {
                self.request_apply_recovery(task);
            }
        });
        let transactions =
            if self.tasks.operations.transactions_task.as_deref() == Some(task.id.as_str()) {
                self.tasks.operations.transactions.clone()
            } else {
                vec![]
            };
        for transaction in transactions {
            ui.add(
                egui::Label::new(format!(
                    "{} · {:?}\n{}\n{}",
                    transaction.transaction_id,
                    transaction.status,
                    transaction.detail,
                    transaction.retained_at.display()
                ))
                .wrap(),
            );
            if matches!(
                transaction.status,
                domain::apply::TransactionStatus::RecoveryRequired
                    | domain::apply::TransactionStatus::Interrupted
            ) && ui
                .add_enabled(enabled, egui::Button::new("Review rollback…"))
                .clicked()
            {
                self.tasks.operations.review_scope = Some(self.operation_scope());
                self.tasks.operations.decision = Some(Decision::RollBack {
                    expected: task.clone(),
                    transaction: transaction.transaction_id,
                });
            }
        }
    }
    fn task_setup_permission(&mut self, ui: &mut egui::Ui, task_id: &str) {
        if !self.tasks.open || self.tasks.selected.as_deref() != Some(task_id) {
            return;
        }
        let Some(process) = &self.tasks.setup else {
            return;
        };
        let Some(dialog) = process.state.dialogs.front().cloned() else {
            return;
        };
        let id = protocol::string(&dialog, "id").to_owned();
        let Some(permission) = super::super::super::prompts::approval(&dialog) else {
            ui.add(egui::Label::new("Unexpected setup question; no command may run. Stop setup and inspect retained evidence.").wrap());
            return;
        };
        let enabled = self.setup_scope_valid()
            && self
                .tasks
                .setup
                .as_ref()
                .is_some_and(|process| process.record.binding.task == task_id);
        let mut answer = None;
        prompt_ui::inline(ui.ctx()).show(ui, |ui| {
            prompt_ui::style(ui);
            prompt_ui::heading(
                ui,
                &permission.title,
                "Retained workflow environment setup · exact-command permission",
            );
            ui.add(egui::Label::new(&permission.explanation).wrap());
            approval_view::arguments(ui, &permission.args);
            prompt_ui::footer(ui, |ui| {
                if prompt_ui::button(ui, "Allow once", prompt_ui::Button::Primary, enabled)
                    .clicked()
                {
                    answer = Some(super::super::super::prompts::permission_response(
                        &permission,
                        false,
                    ));
                }
                if prompt_ui::button(ui, "Deny", prompt_ui::Button::Secondary, true).clicked() {
                    answer = Some(json!({"cancelled":true}));
                }
            });
        });
        if let Some(answer) = answer {
            if !self.setup_scope_valid() {
                if let Some(process) = &mut self.tasks.setup {
                    process.cancel();
                }
                return;
            }
            let value = if answer.get("cancelled") == Some(&json!(true)) {
                None
            } else {
                answer
                    .get("value")
                    .cloned()
                    .or_else(|| answer.get("confirmed").cloned())
            };
            if let Some(process) = &mut self.tasks.setup {
                if let Err(error) = process.answer_dialog(&id, value) {
                    self.state.notice = Some(error);
                }
            }
        }
    }
}
