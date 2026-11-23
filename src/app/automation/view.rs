//! Scheduled tasks page: header, notices, filters, job list and footer help.
//! Rendering only records intents in `Actions`; they are applied after the
//! panel closure so no job state changes while it is being drawn.
use super::*;
mod demo;
mod row;
use row::*;

/// User intents collected during one render pass.
#[derive(Default)]
struct Actions {
    change: Option<(String, &'static str)>,
    creator: Option<agents::AgentRecord>,
    reschedule: Option<String>,
    edit_content: Option<String>,
    new_prompt: bool,
    new_monitor: bool,
    test: Option<String>,
    compare: Option<String>,
    delete: Option<String>,
    reload: bool,
    dismiss: bool,
}

/// Inputs shared by every job entry in one pass.
#[derive(Clone, Copy)]
struct Access {
    writable: bool,
    deletable: bool,
    owner: bool,
}

const HELP: [&str; 4] = [
    "Ask an agent to create a monitor or cron job. Cron schedules use UTC; missed occurrences are skipped.",
    "Checks and scheduling use no AI or context. Delivered events/prompts start an agent response and use tokens.",
    "Monitors trigger once on exit 0; exit 1 keeps waiting. Other failures are shown on the task.",
    "Commands run with local OS permissions, not in a sandbox. Use side-effect-free checks.",
];

impl Pui {
    pub(in crate::app) fn open_automation(&mut self) {
        self.open_agents_board();
        self.kanban.open = false;
        self.tasks.open = false;
        self.automation.open = true;
    }

    pub(in crate::app) fn automation_panel(&mut self, ctx: &egui::Context) {
        let colors = theme::palette(ctx);
        let owner = self.automation.lock.is_some();
        let access = Access {
            writable: self.automation.writable() && !self.demo,
            deletable: self.can_delete_scheduled_job(),
            owner,
        };
        self.drop_stale_comparison(owner);
        let filter_id = egui::Id::new("automation-filter");
        let mut filter = ctx
            .data(|data| data.get_temp::<JobFilter>(filter_id))
            .unwrap_or_default();
        let mut actions = Actions::default();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(colors.bg).inner_margin(24))
            .show(ctx, |ui| {
                // Navigation lives in the shared Chat / Scheduled tasks breadcrumb header.
                self.automation_header(ui, access.writable, &mut actions);
                self.automation_notices(ui, &mut actions);
                ui.add_space(16.0);
                if self.automation.jobs.is_empty() {
                    egui::ScrollArea::vertical()
                        .id_salt("automation-list")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            self.automation_empty(ui);
                            help(ui, true);
                        });
                    return;
                }
                filter_bar(ui, &mut filter, &self.automation.jobs);
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("automation-list")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut shown = false;
                        for job in self.automation.jobs.iter().filter(|j| filter.includes(j)) {
                            shown = true;
                            self.job_entry(ui, job, access, &mut actions);
                        }
                        if !shown {
                            ui.add_space(24.0);
                            ui.label(theme::small(ui, "No tasks in this view."));
                        }
                        help(ui, false);
                    });
            });
        ctx.data_mut(|data| data.insert_temp(filter_id, filter));
        self.apply_automation_actions(ctx, actions, owner);
        self.monitor_test_dialog(ctx);
        self.cron_form(ctx);
        self.reschedule_dialog(ctx);
        self.delete_job_dialog(ctx, access.deletable);
    }

    /// A comparison snapshot is only meaningful while its job inputs are unchanged.
    fn drop_stale_comparison(&mut self, owner: bool) {
        let home = self.automation.home.as_deref();
        if self
            .automation
            .comparison
            .as_ref()
            .is_some_and(|comparison| {
                !self
                    .automation
                    .jobs
                    .iter()
                    .any(|job| comparison.applies(job, home, owner))
            })
        {
            self.automation.comparison = None;
        }
    }

    fn automation_header(&self, ui: &mut egui::Ui, writable: bool, actions: &mut Actions) {
        let colors = theme::palette(ui.ctx());
        let wide = ui.available_width() >= 560.0;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Scheduled tasks").size(25.0).strong());
            if wide {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Right-to-left: the primary action sits at the far edge.
                    create_button(ui, Create::Prompt, writable, actions);
                    create_button(ui, Create::Monitor, writable, actions);
                });
            }
        });
        let counts = job_counts(&self.automation.jobs);
        let summary = if self.automation.jobs.is_empty() {
            "Recurring prompts and shell monitors that wake your agents.".to_string()
        } else {
            format!(
                "{} active · {} paused · {} completed",
                counts[0], counts[1], counts[2]
            )
        };
        ui.label(RichText::new(summary).size(12.0).color(colors.muted));
        if self.demo {
            ui.label(
                RichText::new("Design preview · fixture jobs only · no commands or provider calls")
                    .size(11.0)
                    .color(colors.amber),
            );
        }
        if !wide {
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                create_button(ui, Create::Prompt, writable, actions);
                create_button(ui, Create::Monitor, writable, actions);
            });
        }
    }

    fn automation_notices(&self, ui: &mut egui::Ui, actions: &mut Actions) {
        let colors = theme::palette(ui.ctx());
        let automation = &self.automation;
        if automation.lock.is_none() && automation.home.is_some() && !automation.failed {
            ui.add_space(12.0);
            banner(
                ui,
                colors.amber,
                "Another Pui window owns scheduling",
                "You can delete jobs here; other changes must be made in that window. This window takes over when it closes.",
                |_| {},
            );
        }
        if automation.deleting.is_some() {
            ui.add_space(8.0);
            ui.label(theme::small(ui, "Deleting job in the scheduler’s window…"));
        }
        let Some(error) = &automation.error else {
            return;
        };
        ui.add_space(12.0);
        if automation.load_failed {
            let store = automation
                .home
                .as_ref()
                .map(|home| home.join("jobs.json").display().to_string())
                .unwrap_or_else(|| "the automation store".into());
            banner(
                ui,
                colors.red,
                "Couldn’t load scheduled tasks",
                &format!(
                    "{error} Nothing runs or is modified until the store loads. Fix or move {store}, then reload."
                ),
                |ui| {
                    if prompt_ui::button(ui, "Reload", prompt_ui::Button::Secondary, true).clicked()
                    {
                        actions.reload = true;
                    }
                },
            );
        } else if automation.failed {
            banner(
                ui,
                colors.red,
                "Scheduling stopped",
                &format!("{error} Restart Pui after fixing storage."),
                |_| {},
            );
        } else {
            banner(ui, colors.amber, error, "", |ui| {
                if prompt_ui::button(ui, "Dismiss", prompt_ui::Button::Secondary, true).clicked() {
                    actions.dismiss = true;
                }
            });
        }
    }

    fn automation_empty(&self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        let (title, body) = if self.automation.load_failed {
            (
                "Scheduled tasks unavailable",
                "Saved jobs stay on disk and do not run until the store loads.",
            )
        } else {
            (
                "No scheduled tasks yet.",
                "Ask an agent to create a monitor or recurring prompt, or create one above.",
            )
        };
        egui::Frame::new()
            .fill(colors.surface)
            .stroke(Stroke::new(1.0_f32, colors.border))
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(24, 28))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
                    ui.painter().rect_stroke(
                        rect,
                        9,
                        Stroke::new(1.0_f32, colors.border),
                        egui::StrokeKind::Inside,
                    );
                    icons::draw(ui, rect, Icon::Clock, colors.muted);
                    ui.add_space(10.0);
                    ui.label(RichText::new(title).size(14.0).strong());
                    ui.add(egui::Label::new(theme::small(ui, body)).wrap());
                });
            });
        ui.add_space(8.0);
    }

    fn job_entry(&self, ui: &mut egui::Ui, job: &Job, access: Access, actions: &mut Actions) {
        let colors = theme::palette(ui.ctx());
        ui.push_id(&job.id, |ui| {
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.id().with("details"),
                false,
            );
            let running = self.automation.running.contains_key(&job.id);
            if job_row(ui, job, running, state.is_open()).clicked() {
                state.toggle(ui);
            }
            // Errors and delivery blockers stay visible even when details are closed.
            if let Some(error) = &job.error {
                ui.add(
                    egui::Label::new(RichText::new(error).size(12.0).color(colors.amber)).wrap(),
                );
            }
            state.show_body_unindented(ui, |ui| {
                egui::Frame::new()
                    .fill(colors.surface)
                    .stroke(Stroke::new(1.0_f32, colors.border))
                    .corner_radius(8)
                    .inner_margin(16)
                    .show(ui, |ui| {
                        ui.set_max_width(960.0_f32.min(ui.available_width()));
                        ui.set_width(ui.available_width());
                        self.job_details(ui, job, access, actions);
                    });
                ui.add_space(12.0);
            });
            ui.separator();
        });
    }

    fn job_details(&self, ui: &mut egui::Ui, job: &Job, access: Access, actions: &mut Actions) {
        let colors = theme::palette(ui.ctx());
        let writable = access.writable;
        // Editing is only safe while the job is paused with nothing in flight.
        let editable = writable && !job.enabled && !job.dispatching && job.pending.is_none();
        ui.horizontal_wrapped(|ui| {
            let label = if job.enabled {
                "Pause"
            } else if job.completed {
                "Run again"
            } else {
                "Resume"
            };
            if ui.add_enabled(writable, egui::Button::new(label)).clicked() {
                actions.change =
                    Some((job.id.clone(), if job.enabled { "pause" } else { "resume" }));
            }
            let monitor = matches!(job.trigger, Trigger::Monitor { .. });
            let timing = if monitor {
                "Edit timing…"
            } else {
                "Edit schedule…"
            };
            if ui
                .add_enabled(editable, egui::Button::new(timing))
                .clicked()
            {
                actions.reschedule = Some(job.id.clone());
            }
            if ui
                .add_enabled(editable, egui::Button::new("Edit prompt…"))
                .clicked()
            {
                actions.edit_content = Some(job.id.clone());
            }
            if monitor
                && ui
                    .add_enabled(
                        editable && self.automation.precheck.is_none(),
                        egui::Button::new("Test once…"),
                    )
                    .clicked()
            {
                actions.test = Some(job.id.clone());
            }
            ui.menu_button("•••", |ui| {
                theme::menu_controls(ui);
                if ui
                    .add_enabled(access.deletable, egui::Button::new("Delete…"))
                    .clicked()
                {
                    actions.delete = Some(job.id.clone());
                    ui.close();
                }
            });
        });
        ui.add_space(8.0);
        ui.label(theme::small(ui, "Prompt for the creator chat"));
        ui.add(egui::Label::new(&job.message).wrap().selectable(true));
        ui.add_space(12.0);
        if job.show_check_history(
            ui,
            self.automation.home.as_deref(),
            access.owner,
            self.automation.comparison.as_ref(),
        ) {
            actions.compare = Some(job.id.clone());
        }
        ui.label(theme::small(ui, "Created by"));
        if ui
            .add_enabled(
                !self.demo,
                egui::Button::new(
                    RichText::new(&job.creator.name)
                        .size(12.0)
                        .color(colors.accent),
                )
                .frame(false)
                .truncate(),
            )
            .on_hover_text(format!(
                "{} ({})\n{}",
                job.creator.name,
                job.creator.id,
                job.creator.path.display()
            ))
            .clicked()
        {
            actions.creator = Some(job.creator.clone());
        }
        ui.label(theme::small(
            ui,
            format!(
                "Last run: {} · {} delivered",
                job.last_run
                    .map(timestamp)
                    .unwrap_or_else(|| "Never".into()),
                job.deliveries
            ),
        ));
        if job.enabled && !job.dispatching && job.pending.is_none() {
            ui.label(theme::small(
                ui,
                format!("Next: {}", timestamp(job.next_run)),
            ));
        }
        if job.completed {
            ui.add(egui::Label::new(theme::small(ui, "Monitor completed; follow-up work is not verified here. Review the creator chat before running again.")).wrap());
        }
        ui.add_space(8.0);
        egui::CollapsingHeader::new(theme::small(ui, "Execution details"))
            .id_salt("execution")
            .show(ui, |ui| execution_details(ui, job));
    }

    fn apply_automation_actions(&mut self, ctx: &egui::Context, actions: Actions, owner: bool) {
        // Explicit user activation only, outside render closures; no idle collection.
        // Keep one bounded comparison snapshot across all jobs/checks.
        if let Some(id) = actions.compare
            && !egui::Popup::is_any_open(ctx)
            && !prompt_ui::modal_owns_current_pass(ctx)
            && ctx.memory(|memory| memory.top_modal_layer().is_none())
            && let Some(job) = self.automation.jobs.iter().find(|job| job.id == id)
        {
            self.automation.comparison =
                evidence::Comparison::collect(job, self.automation.home.as_deref(), owner);
            ctx.request_repaint();
        }
        if actions.dismiss {
            self.automation.error = None;
        }
        if actions.reload {
            // The next scheduler poll restores owners and resumes normally.
            self.automation.reload();
        }
        if let Some(id) = actions.delete {
            self.automation.delete = Some(id);
        }
        let results = [
            actions.change.map(|(id, op)| self.change_job(&id, op)),
            actions.reschedule.map(|id| self.begin_reschedule(&id)),
            actions.edit_content.map(|id| self.begin_job_content(&id)),
            actions.new_prompt.then(|| self.begin_cron_form()),
            actions.new_monitor.then(|| self.begin_monitor_form()),
            actions.test.map(|id| self.request_monitor_test(&id)),
        ];
        if let Some(error) = results.into_iter().flatten().find_map(Result::err) {
            self.automation.error = Some(error);
        }
        if let Some(record) = actions.creator {
            self.open_job_creator(record);
        }
    }

    fn open_job_creator(&mut self, record: agents::AgentRecord) {
        if self.agents.nodes.contains_key(&record.id) {
            self.view_agent(&record.id);
            return;
        }
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

    fn delete_job_dialog(&mut self, ctx: &egui::Context, deletable: bool) {
        let Some(id) = self.automation.delete.clone() else {
            return;
        };
        let choice = confirmations::Dialog::new(
            "delete-automation",
            "Delete this job?",
            "Stop future checks/prompts and remove this scheduled job.",
            "Cancel",
            "Delete job",
        )
        .enabled(deletable)
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
                if let Err(error) = self.request_scheduled_job_delete(&id) {
                    self.automation.error = Some(error);
                }
                self.automation.delete = None;
            }
            None => {}
        }
    }
}

#[derive(Clone, Copy)]
enum Create {
    Prompt,
    Monitor,
}

fn create_button(ui: &mut egui::Ui, kind: Create, writable: bool, actions: &mut Actions) {
    let (label, style, tip) = match kind {
        Create::Prompt => (
            "New recurring prompt…",
            prompt_ui::Button::Primary,
            "Send a prompt to a chat on a UTC cron schedule",
        ),
        Create::Monitor => (
            "New shell monitor…",
            prompt_ui::Button::Secondary,
            "Run a shell check until it exits 0, then notify a chat",
        ),
    };
    if prompt_ui::button(ui, label, style, writable)
        .on_hover_text(tip)
        .clicked()
    {
        match kind {
            Create::Prompt => actions.new_prompt = true,
            Create::Monitor => actions.new_monitor = true,
        }
    }
}

/// Tinted notice with a title, optional body and trailing actions.
fn banner(
    ui: &mut egui::Ui,
    color: egui::Color32,
    title: &str,
    body: &str,
    actions: impl FnOnce(&mut egui::Ui),
) {
    let colors = theme::palette(ui.ctx());
    egui::Frame::new()
        .fill(color.gamma_multiply(0.08))
        .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.45)))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(title).size(13.0).color(color)).wrap());
            if !body.is_empty() {
                ui.add(
                    egui::Label::new(RichText::new(body).size(12.0).color(colors.muted))
                        .wrap()
                        .selectable(true),
                );
            }
            ui.horizontal_wrapped(actions);
        });
}

fn filter_bar(ui: &mut egui::Ui, filter: &mut JobFilter, jobs: &[Job]) {
    let counts = job_counts(jobs);
    ui.horizontal_wrapped(|ui| {
        for (value, label, count) in [
            (JobFilter::All, "All tasks", jobs.len()),
            (JobFilter::Active, "Active", counts[0]),
            (JobFilter::History, "History", counts[2]),
        ] {
            if theme::choice(ui, *filter == value, format!("{label}  {count}")).clicked() {
                *filter = value;
            }
        }
    });
}

fn execution_details(ui: &mut egui::Ui, job: &Job) {
    match &job.trigger {
        Trigger::Cron { schedule } => {
            ui.add(
                egui::Label::new(RichText::new(format!("Cron · {schedule} · UTC")).monospace())
                    .wrap()
                    .selectable(true),
            );
        }
        Trigger::Monitor {
            command,
            interval_seconds,
            timeout_seconds,
        } => {
            ui.add(
                egui::Label::new(theme::small(
                    ui,
                    format!(
                        "Monitor · every {interval_seconds}s · timeout {timeout_seconds}s · one-shot"
                    ),
                ))
                .wrap(),
            );
            ui.add(
                egui::Label::new(RichText::new(command).monospace())
                    .wrap()
                    .selectable(true),
            );
        }
    }
    ui.add(
        egui::Label::new(theme::small(
            ui,
            format!("Workspace: {}", job.creator.cwd.display()),
        ))
        .wrap()
        .selectable(true),
    );
    ui.add(
        egui::Label::new(theme::small(
            ui,
            format!(
                "Creator: {} · {}",
                job.creator.id,
                job.creator.path.display()
            ),
        ))
        .wrap()
        .selectable(true),
    );
}

fn help(ui: &mut egui::Ui, open: bool) {
    ui.add_space(18.0);
    ui.add(
        egui::Label::new(theme::small(
            ui,
            "Runs only while Pui is open · Active jobs reconnect on restart",
        ))
        .wrap(),
    );
    egui::CollapsingHeader::new(theme::small(ui, "How scheduling works"))
        .id_salt("automation-help")
        .default_open(open)
        .show(ui, |ui| {
            for text in HELP {
                ui.add(egui::Label::new(theme::small(ui, text)).wrap());
            }
        });
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum JobFilter {
    #[default]
    All,
    Active,
    History,
}
impl JobFilter {
    fn includes(self, job: &Job) -> bool {
        match self {
            Self::All => true,
            Self::Active => job.enabled && !job.completed,
            Self::History => job.completed,
        }
    }
}

#[cfg(test)]
mod tests;
