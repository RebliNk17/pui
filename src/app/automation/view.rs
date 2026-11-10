use super::*;
impl Pui {
    pub(in crate::app) fn demo_automation(&mut self) {
        let creator = agents::AgentRecord {
            id: "agent-1".into(),
            parent: None,
            name: "Release readiness".into(),
            cwd: PathBuf::from("/workspace/payments"),
            path: PathBuf::from("/example/release-readiness.jsonl"),
            access: "workspace".into(),
            worktree: None,
        };
        self.automation.jobs = vec![
            Job {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Morning release review".into(),
                creator: creator.clone(),
                trigger: Trigger::Cron {
                    schedule: "0 9 * * 1-5".into(),
                },
                message: "Review open pull requests and summarize release blockers.".into(),
                enabled: true,
                completed: false,
                next_run: now() + 3600,
                last_run: Some(now() - 86400),
                deliveries: 3,
                pending: None,
                dispatching: false,
                error: None,
            },
            Job {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Wait for CI to pass".into(),
                creator: agents::AgentRecord {
                    name: "Fix checkout tests".into(),
                    id: "agent-2".into(),
                    ..creator.clone()
                },
                trigger: Trigger::Monitor {
                    command:
                        "gh run view 12345 --json conclusion --jq '.conclusion' | grep -qx success"
                            .into(),
                    interval_seconds: 60,
                    timeout_seconds: 30,
                },
                message: "CI is green. Review the results before proceeding.".into(),
                enabled: true,
                completed: false,
                next_run: now() + 40,
                last_run: Some(now() - 20),
                deliveries: 0,
                pending: None,
                dispatching: false,
                error: None,
            },
            Job {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Weekly dependency review".into(),
                creator,
                trigger: Trigger::Cron {
                    schedule: "30 10 * * 1".into(),
                },
                message: "Inspect dependency advisories and propose updates. Do not modify files."
                    .into(),
                enabled: false,
                completed: false,
                next_run: now() + 86400,
                last_run: None,
                deliveries: 0,
                pending: None,
                dispatching: false,
                error: None,
            },
        ];
    }
    pub(in crate::app) fn open_automation(&mut self) {
        self.open_agents_board();
        self.kanban.open = false;
        self.tasks.open = false;
        self.automation.open = true;
    }
    pub(in crate::app) fn automation_panel(&mut self, ctx: &egui::Context) {
        let colors = theme::palette(ctx);
        let writable = self.automation.writable() && !self.demo;
        let mut action = None;
        let mut creator = None;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(colors.bg).inner_margin(24)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Scheduled tasks");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Back to chat").clicked() { self.automation.open = false; }
                });
            });
            ui.add_space(8.0);
            if self.demo { ui.label(theme::small(ui, "Design preview · fixture jobs only · no commands or provider calls")); }
            ui.label(theme::small(ui, "Runs while Pui is open · Active jobs reconnect their creator on restart · Cron schedules use UTC"));
            ui.label(theme::small(ui, "Checks and scheduling use no AI or context. Delivered events/prompts start an agent response and use tokens."));
            ui.label(theme::small(ui, "Ask an agent to create a monitor or cron job. Monitors trigger once on exit 0; exit 1 keeps waiting. Missed cron occurrences are skipped."));
            if self.automation.lock.is_none() && self.automation.home.is_some() && !self.automation.failed {
                ui.colored_label(colors.amber, "Read-only · Another Pui window owns the scheduler. Manage jobs in that window; this window takes over when it closes.");
            }
            if let Some(error) = &self.automation.error { ui.colored_label(colors.red, error); }
            ui.add_space(16.0);
            if self.automation.jobs.is_empty() {
                ui.label("No cron jobs or monitors yet.");
            }
            egui::ScrollArea::vertical().id_salt("automation-list").show(ui, |ui| {
                for job in &self.automation.jobs {
                    ui.push_id(&job.id, |ui| {
                        egui::Frame::new().fill(colors.surface).stroke(Stroke::new(1.0, colors.border)).corner_radius(8).inner_margin(16).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(&job.name).strong());
                                let status = if job.dispatching { "Delivering" } else if job.completed { "Completed" } else if !job.enabled { "Paused" } else if self.automation.running.contains_key(&job.id) { "Checking" } else if job.pending.is_some() { "Event waiting for delivery" } else { "Active" };
                                ui.colored_label(if job.enabled { colors.green } else { colors.dim }, status);
                                if ui.add_enabled(writable, egui::Button::new(if job.enabled { "Pause" } else { "Resume" })).clicked() {
                                    action = Some((job.id.clone(), if job.enabled { "pause" } else { "resume" }));
                                }
                                if ui.add_enabled(writable, egui::Button::new("Delete…")).clicked() { self.automation.delete = Some(job.id.clone()); }
                            });
                            ui.horizontal_wrapped(|ui| {
                                ui.label("Created by");
                                if ui.add_enabled(!self.demo, egui::Link::new(format!("{} ({})", job.creator.name, job.creator.id))).on_hover_text(job.creator.path.display().to_string()).clicked() { creator = Some(job.creator.clone()); }
                            });
                            match &job.trigger {
                                Trigger::Cron { schedule } => { ui.label(RichText::new(format!("Cron · {schedule} · UTC")).monospace()); }
                                Trigger::Monitor { command, interval_seconds, timeout_seconds } => {
                                    ui.label(format!("Monitor · every {interval_seconds}s · timeout {timeout_seconds}s · one-shot"));
                                    ui.add(egui::Label::new(RichText::new(command).monospace()).wrap().selectable(true));
                                }
                            }
                            ui.add(egui::Label::new(&job.message).wrap().selectable(true));
                            if job.enabled && !job.dispatching { ui.label(theme::small(ui, format!("Next: {}", timestamp(job.next_run)))); }
                            ui.label(theme::small(ui, format!("Last run: {} · {} delivered", job.last_run.map(timestamp).unwrap_or_else(|| "Never".into()), job.deliveries)));
                            ui.label(theme::small(ui, format!("Workspace: {}", job.creator.cwd.display())));
                            if let Some(error) = &job.error { ui.colored_label(colors.amber, error); }
                        });
                    });
                    ui.add_space(10.0);
                }
            });
        });
        if let Some((id, operation)) = action {
            if let Err(error) = self.change_job(&id, operation) {
                self.automation.error = Some(error);
            }
        }
        if let Some(record) = creator {
            if self.agents.nodes.contains_key(&record.id) {
                self.view_agent(&record.id);
            } else {
                self.automation.open = false;
                self.open_chat(Session {
                    path: record.path,
                    project: record.cwd.clone(),
                    cwd: record.cwd,
                    title: record.name,
                    created: None,
                    modified: std::time::SystemTime::now(),
                });
            }
        }
        if let Some(id) = self.automation.delete.clone() {
            let choice = confirmations::Dialog::new(
                "delete-automation",
                "Delete this job?",
                "Stop future checks/prompts and remove this scheduled job.",
                "Cancel",
                "Delete job",
            )
            .enabled(writable)
            .show(ctx, |ui| {
                if let Some(job) = self.automation.jobs.iter().find(|job| job.id == id) {
                    ui.add(egui::Label::new(RichText::new(&job.name).strong()).wrap());
                }
                confirmations::note(
                    ui,
                    "The creator chat and any already delivered messages are kept.",
                );
            });
            match choice {
                Some(confirmations::Choice::Cancel) => self.automation.delete = None,
                Some(confirmations::Choice::Confirm) => {
                    if let Err(error) = self.change_job(&id, "delete") {
                        self.automation.error = Some(error);
                    }
                    self.automation.delete = None;
                }
                None => {}
            }
        }
    }
}
fn timestamp(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| "Unavailable".into())
}
