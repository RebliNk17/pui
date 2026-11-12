use super::*;

impl Pui {
    pub(in crate::app) fn task_panel(&mut self, ctx: &egui::Context) {
        let colors = theme::palette(ctx);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(colors.bg)).show(ctx, |ui| {
            egui::TopBottomPanel::top("task-conversation-header")
                .show_separator_line(false)
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(24, 10)))
                .show_inside(ui, |ui| self.task_header(ui));
            let Some(task) = self.tasks.current().cloned() else { return; };
            let linked = self.task_in_chat(&task);
            if linked {
                egui::TopBottomPanel::bottom("composer-v2")
                    .show_separator_line(false)
                    .frame(egui::Frame::new().fill(colors.bg).inner_margin(egui::Margin { left: 24, right: 24, top: 8, bottom: 3 }))
                    .show_inside(ui, |ui| {
                        let width = (ui.available_width() - 24.0).min(760.0);
                        let padding = ((ui.available_width() - width) / 2.0).max(0.0);
                        ui.horizontal(|ui| {
                            ui.add_space(padding);
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                ui.label(theme::small(ui, "Chat with Pi · same conversation, separate tool permissions"));
                                self.composer(ui, ctx);
                            });
                        });
                    });
            }
            // The work item has one scroll and one identity across all tabs.
            // Worker logs render inside it; they never replace the chat composer.
            egui::ScrollArea::vertical().id_salt(("work-item", &task.id))
                .auto_shrink([false, false]).show(ui, |ui| {
                    let width = ui.available_width().min(760.0);
                    let pad = ((ui.available_width() - width) / 2.0).max(0.0);
                    ui.horizontal(|ui| {
                        ui.add_space(pad);
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            self.task_content(ui);
                        });
                    });
                });
        });
    }

    pub(super) fn task_routing_details(&self, ui: &mut egui::Ui, task: &Task) {
        if task.dynamic_routing || task.runs.iter().any(|r| r.selection.is_some()) {
            crate::section::Section::new("Model choices & routing reasons").id_salt((&task.id, "routing-details")).summary("Per-stage models, reported cost and selection rationale").icon(Icon::Sliders).show(ui,|ui| {
                if task.runs.is_empty(){ui.label("Model selection happens before each stage, using the available catalog and workflow evidence.");}
                for run in &task.runs {
                    if let Some(selection)=&run.selection {
                        let label=run.routing_for.map(|r|format!("Router → {}",r.label())).unwrap_or_else(||run.role.label().into());
                        ui.add(egui::Label::new(format!("{} · attempt {} · {} / {} · {} · ${:.4}\n{}",label,run.attempt+1,selection.provider,selection.model,thinking_label(&selection.thinking),run.cost+run.permission_cost,selection.reason)).wrap().selectable(true));
                    }
                }
                if let Some(limit)=task.spend_limit{ui.label(format!("Reported stop threshold: ${limit:.2} (not a hard cap)"));}
            });
        }
    }
    pub(super) fn task_dialog(&mut self, ui: &mut egui::Ui) {
        // Approval belongs to the displayed workflow, never to an unrelated
        // history page that happens to be open while another workflow runs.
        if !self.tasks.open
            || self.tasks.selected.is_none()
            || self.tasks.selected != self.tasks.live
        {
            return;
        }
        let Some(task) = self.tasks.current() else {
            return;
        };
        let Some((run, dialog)) = self
            .tasks
            .processes
            .iter()
            .filter(|p| !p.ended && task.runs.iter().any(|r| r.id == p.run))
            .find_map(|p| p.state.dialogs.front().map(|d| (p.run.clone(), d.clone())))
        else {
            return;
        };
        let objective = task.objective.clone();
        let workspace = task
            .workspace
            .as_ref()
            .unwrap_or(&task.source)
            .display()
            .to_string();
        let id = protocol::string(&dialog, "id");
        if self.tasks.current_dialog != id {
            self.tasks.current_dialog = id.into();
            self.tasks.dialog_text = protocol::string(&dialog, "prefill").into();
        }
        if let Some(request) = questions::Request::parse(&dialog) {
            if let Some(answer) = self.tasks.questions.show(
                ui,
                &dialog,
                &request,
                false,
                prompt_ui::keyboard_available(ui),
            ) {
                self.task_answer(&run, id, answer);
            }
            return;
        }
        let mut answer = None;
        prompt_ui::inline(ui.ctx()).show(ui, |ui| {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    format!("Workflow: {objective}\nWorkspace: {workspace}"),
                ))
                .wrap(),
            );
            prompt_ui::style(ui);
            ui.label(
                RichText::new(format!(
                    "{} · action required",
                    self.tasks
                        .processes
                        .iter()
                        .find(|p| p.run == run)
                        .unwrap()
                        .role
                        .label()
                ))
                .strong(),
            );
            let permission = super::super::prompts::approval(&dialog);
            egui::ScrollArea::vertical()
                .id_salt(("task-permission", id))
                .max_height(210.0)
                .show(ui, |ui| {
                    if let Some(p) = &permission {
                        prompt_ui::heading(ui, &p.title, "");
                        ui.add(egui::Label::new(&p.explanation).wrap());
                        approval_view::arguments(ui, &p.args);
                    } else {
                        prompt_ui::heading(ui, protocol::string(&dialog, "title"), "");
                        ui.add(egui::Label::new(protocol::string(&dialog, "message")).wrap());
                    }
                });
            ui.horizontal_wrapped(|ui| {
                if let Some(p) = &permission {
                    if prompt_ui::button(ui, "Allow once", prompt_ui::Button::Primary, true)
                        .clicked()
                    {
                        answer = Some(super::super::prompts::permission_response(p, false));
                    }
                    if p.always
                        && prompt_ui::button(
                            ui,
                            "Always allow exact action",
                            prompt_ui::Button::Secondary,
                            true,
                        )
                        .clicked()
                    {
                        answer = Some(super::super::prompts::permission_response(p, true));
                    }
                } else {
                    match protocol::string(&dialog, "method") {
                        "confirm" => {
                            if prompt_ui::button(ui, "Confirm", prompt_ui::Button::Primary, true)
                                .clicked()
                            {
                                answer = Some(json!({"confirmed":true}));
                            }
                        }
                        "select" => {
                            if let Some(options) = dialog["options"].as_array() {
                                for o in options.iter().filter_map(Value::as_str) {
                                    if prompt_ui::option(ui, o, "", false).clicked() {
                                        answer = Some(json!({"value":o}));
                                    }
                                }
                            }
                        }
                        "input" | "editor" => {
                            ui.add(
                                egui::TextEdit::multiline(&mut self.tasks.dialog_text)
                                    .desired_rows(2),
                            );
                            if prompt_ui::button(
                                ui,
                                "Send answer",
                                prompt_ui::Button::Primary,
                                true,
                            )
                            .clicked()
                            {
                                answer = Some(json!({"value":self.tasks.dialog_text}));
                            }
                        }
                        _ => {
                            ui.label("Unsupported extension prompt; deny to stop safely.");
                        }
                    }
                }
                if prompt_ui::button(ui, "Deny / cancel", prompt_ui::Button::Secondary, true)
                    .clicked()
                {
                    answer = Some(json!({"cancelled":true}));
                }
            });
        });
        if let Some(answer) = answer {
            self.task_answer(&run, id, answer);
        }
    }
    pub(in crate::app) fn demo_task(&mut self) {
        let mut t = Task::new(
            self.prefs.cwd.clone(),
            "example-provider".into(),
            "example-model".into(),
            "high".into(),
        );
        t.objective = "Preserve the unsent draft when switching chats".into();
        t.parent_session = self.state.session_file.as_ref().map(PathBuf::from);
        t.phase = Phase::Verifying;
        t.workspace = Some(PathBuf::from("/demo/isolated-worktree"));
        t.isolated = true;
        t.proposed = Contract {
            criteria: vec![domain::Criterion {
                id: "C1".into(),
                description: "Switch away and back: exact draft is retained".into(),
                command: "cargo test task_draft_survives_chat_switch".into(),
            }],
        };
        t.approved = Some(t.proposed.clone());
        let mut a = Attempt::new(0);
        a.fingerprint = Some("demo-source-fingerprint-not-real".into());
        a.checks.push(Check {
            criterion: "C1".into(),
            command: t.proposed.criteria[0].command.clone(),
            exit_code: Some(0),
            output: "DEMO ONLY: example regression test passed. No command was executed.".into(),
            truncated: false,
            cancelled: false,
            duration_ms: 81,
        });
        t.attempts.push(a);
        for role in [Role::Proposal, Role::Builder, Role::Checks, Role::Verifier] {
            t.runs.push(Run {
                id: format!("demo-{role:?}"),
                role,
                attempt: 0,
                status: if role == Role::Verifier {
                    "Working"
                } else {
                    "Finished"
                }
                .into(),
                session: None,
                summary: if role == Role::Proposal {
                    serde_json::to_string(&t.proposed).unwrap()
                } else {
                    "DEMO: example agent transcript, not a provider run.".into()
                },
                cost: if role == Role::Builder { 0.42 } else { 0.03 },
                permission_cost: 0.0,
                selection: None,
                routing_for: None,
            });
        }
        let runs = t.runs.clone();
        for run in runs {
            self.tasks.histories.insert(
                run.id.clone(),
                State {
                    model: t.model.clone(),
                    provider: t.provider.clone(),
                    thinking: t.thinking.clone(),
                    messages: vec![Message {
                        role: "assistant".into(),
                        text: if run.role == Role::Proposal {
                            run.summary.clone()
                        } else {
                            format!("DEMO ONLY · {}\n\n{}", run.role.label(), run.summary)
                        },
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            );
        }
        let parent = self.ensure_agent();
        self.tasks.parents.insert(t.id.clone(), parent);
        self.state.messages = vec![
            Message { role: "user".into(), text: "Keep my unsent draft when I switch to another chat and back.".into(), ..Default::default() },
            Message { role: "assistant".into(), text: "The draft-preservation change is implemented. The regression check passed; a fresh verifier is reviewing the result.".into(), ..Default::default() },
        ];
        self.tasks.selected = Some(t.id.clone());
        self.tasks.records = vec![t];
        self.tasks.open = true;
        self.prefs.cards.hidden.remove(&Card::Workflow);
        self.prefs.cards.collapsed.remove(&Card::Workflow);
        self.prefs
            .cards
            .order
            .retain(|card| *card != Card::Workflow);
        self.prefs.cards.order.insert(0, Card::Workflow);
        for card in [Card::Environment, Card::Context, Card::Outputs, Card::Pulls] {
            self.prefs.cards.collapsed.insert(card);
        }
    }
}
