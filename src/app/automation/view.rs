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
            permission: None,
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
                next_run: cron::Cron::parse("0 9 * * 1-5").unwrap().next(now()).unwrap(),
                last_run: Some(now() - 86400),
                evidence:None,
                previous_checks:Vec::new(),
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
                evidence:None,
                previous_checks:Vec::new(),
                deliveries: 0,
                pending: None,
                dispatching: false,
                error: None,
            },
            Job {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Weekly dependency review".into(),
                creator: creator.clone(),
                trigger: Trigger::Cron {
                    schedule: "30 10 * * 1".into(),
                },
                message: "Inspect dependency advisories and propose updates. Do not modify files."
                    .into(),
                enabled: false,
                completed: false,
                next_run: cron::Cron::parse("30 10 * * 1").unwrap().next(now()).unwrap(),
                last_run: None,
                evidence:None,
                previous_checks:Vec::new(),
                deliveries: 0,
                pending: None,
                dispatching: false,
                error: None,
            },
            Job {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Harbor PR 71 validation completion".into(),
                creator: agents::AgentRecord {
                    name: "Harbor release checks".into(),
                    id: "agent-3".into(),
                    ..creator
                },
                trigger: Trigger::Monitor {
                    command: "gh pr checks 71 --repo example/harbor --required".into(),
                    interval_seconds: 60,
                    timeout_seconds: 20,
                },
                message: "Recheck the exact PR head, all CI/security results and merge rules. Report blockers; do not bypass rules or deploy manually.".into(),
                enabled: false,
                completed: true,
                next_run: now(),
                last_run: Some(now() - 1800),
                evidence:None,
                previous_checks:Vec::new(),
                deliveries: 1,
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
        let deletable = self.can_delete_scheduled_job();
        let mut action = None;
        let mut creator = None;
        let mut reschedule = None;
        let mut edit_content = None;
        let mut create = false;
        let mut monitor = false;
        let mut test = None;
        let counts = job_counts(&self.automation.jobs);
        let filter_id = egui::Id::new("automation-filter");
        let mut filter = ctx
            .data(|data| data.get_temp::<JobFilter>(filter_id))
            .unwrap_or_default();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(colors.bg).inner_margin(24))
            .show(ctx, |ui| {
                // Navigation lives in the shared Chat / Scheduled tasks breadcrumb header.
                ui.heading("Scheduled tasks");
                if ui.add_enabled(writable,egui::Button::new("New recurring prompt…")).clicked(){create=true;}
                if ui.add_enabled(writable,egui::Button::new("New shell monitor…")).clicked(){monitor=true;}
                ui.label(theme::small(ui, format!("{} active · {} paused · {} completed", counts[0], counts[1], counts[2])));
                if self.demo {
                    ui.label(theme::small(ui, "Design preview · fixture jobs only · no commands or provider calls"));
                }
                if self.automation.lock.is_none() && self.automation.home.is_some() && !self.automation.failed {
                    ui.add(egui::Label::new(RichText::new("Another Pui window owns scheduling. You can delete jobs here; other changes must be made in that window. This window takes over when it closes.").color(colors.amber)).wrap());
                }
                if self.automation.deleting.is_some() {
                    ui.label(theme::small(ui, "Deleting job in the scheduler’s window…"));
                }
                if let Some(error) = &self.automation.error {
                    ui.add(egui::Label::new(RichText::new(error).color(colors.red)).wrap());
                }
                ui.add_space(16.0);
                ui.horizontal_wrapped(|ui| {
                    for (value, label, count) in [
                        (JobFilter::All, "All tasks", self.automation.jobs.len()),
                        (JobFilter::Active, "Active", counts[0]),
                        (JobFilter::History, "History", counts[2]),
                    ] {
                        if theme::choice(ui, filter == value, format!("{label}  {count}")).clicked() {
                            filter = value;
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().id_salt("automation-list").auto_shrink([false, false]).show(ui, |ui| {
                    if self.automation.jobs.is_empty() {
                        ui.add_space(24.0);
                        ui.label("No scheduled tasks yet.");
                        ui.label(theme::small(ui, "Ask an agent to create a monitor or recurring prompt."));
                    } else if !self.automation.jobs.iter().any(|job| filter.includes(job)) {
                        ui.add_space(24.0);
                        ui.label(theme::small(ui, "No tasks in this view."));
                    }
                    for job in self.automation.jobs.iter().filter(|job| filter.includes(job)) {
                        ui.push_id(&job.id, |ui| {
                            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), ui.id().with("details"), false);
                            let response = job_row(ui, job, self.automation.running.contains_key(&job.id), state.is_open());
                            if response.clicked() {
                                state.toggle(ui);
                            }
                            // Errors and delivery blockers stay visible even when details are closed.
                            if let Some(error) = &job.error {
                                ui.add(egui::Label::new(RichText::new(error).size(12.0).color(colors.amber)).wrap());
                            }
                            state.show_body_unindented(ui, |ui| {
                                egui::Frame::new().fill(colors.surface).stroke(Stroke::new(1.0, colors.border)).corner_radius(8).inner_margin(16).show(ui, |ui| {
                                    ui.set_max_width(960.0_f32.min(ui.available_width()));
                                    ui.set_width(ui.available_width());
                                    ui.horizontal_wrapped(|ui| {
                                        let label = if job.enabled { "Pause" } else if job.completed { "Run again" } else { "Resume" };
                                        if ui.add_enabled(writable, egui::Button::new(label)).clicked() {
                                            action = Some((job.id.clone(), if job.enabled { "pause" } else { "resume" }));
                                        }
                                        if ui.add_enabled(writable&&!job.enabled&&!job.dispatching&&job.pending.is_none(),egui::Button::new(if matches!(job.trigger,Trigger::Cron{..}){"Edit schedule…"}else{"Edit timing…"})).clicked(){reschedule=Some(job.id.clone());}
                                        if ui.add_enabled(writable&&!job.enabled&&!job.dispatching&&job.pending.is_none(),egui::Button::new("Edit prompt…")).clicked(){edit_content=Some(job.id.clone());}
                                        if matches!(job.trigger,Trigger::Monitor{..})&&ui.add_enabled(writable&&!job.enabled&&job.pending.is_none()&&!job.dispatching&&self.automation.precheck.is_none(),egui::Button::new("Test once…")).clicked(){test=Some(job.id.clone());}
                                        ui.menu_button("•••", |ui| {
                                            theme::menu_controls(ui);
                                            if ui.add_enabled(deletable, egui::Button::new("Delete…")).clicked() {
                                                self.automation.delete = Some(job.id.clone());
                                                ui.close();
                                            }
                                        });
                                    });
                                    ui.add_space(8.0);
                                    ui.label(theme::small(ui, "Prompt for the creator chat"));
                                    ui.add(egui::Label::new(&job.message).wrap().selectable(true));
                                    ui.add_space(12.0);
                                    job.show_check_history(ui);
                                    ui.label(theme::small(ui, "Created by"));
                                    if ui.add_enabled(!self.demo, egui::Button::new(RichText::new(&job.creator.name).size(12.0).color(colors.accent)).frame(false).truncate())
                                        .on_hover_text(format!("{} ({})\n{}", job.creator.name, job.creator.id, job.creator.path.display())).clicked() {
                                        creator = Some(job.creator.clone());
                                    }
                                    ui.label(theme::small(ui, format!("Last run: {} · {} delivered", job.last_run.map(timestamp).unwrap_or_else(|| "Never".into()), job.deliveries)));
                                    if job.enabled && !job.dispatching && job.pending.is_none() {
                                        ui.label(theme::small(ui, format!("Next: {}", timestamp(job.next_run))));
                                    }
                                    if job.completed {
                                        ui.add(egui::Label::new(theme::small(ui, "Monitor completed; follow-up work is not verified here. Review the creator chat before running again.")).wrap());
                                    }
                                    ui.add_space(8.0);
                                    egui::CollapsingHeader::new(theme::small(ui, "Execution details")).id_salt("execution").show(ui, |ui| {
                                        match &job.trigger {
                                            Trigger::Cron { schedule } => {
                                                ui.add(egui::Label::new(RichText::new(format!("Cron · {schedule} · UTC")).monospace()).wrap().selectable(true));
                                            }
                                            Trigger::Monitor { command, interval_seconds, timeout_seconds } => {
                                                ui.add(egui::Label::new(theme::small(ui, format!("Monitor · every {interval_seconds}s · timeout {timeout_seconds}s · one-shot"))).wrap());
                                                ui.add(egui::Label::new(RichText::new(command).monospace()).wrap().selectable(true));
                                            }
                                        }
                                        ui.add(egui::Label::new(theme::small(ui, format!("Workspace: {}", job.creator.cwd.display()))).wrap().selectable(true));
                                        ui.add(egui::Label::new(theme::small(ui, format!("Creator: {} · {}", job.creator.id, job.creator.path.display()))).wrap().selectable(true));
                                    });
                                });
                                ui.add_space(12.0);
                            });
                            ui.separator();
                        });
                    }
                    ui.add_space(18.0);
                    ui.add(egui::Label::new(theme::small(ui, "Runs only while Pui is open · Active jobs reconnect on restart")).wrap());
                    egui::CollapsingHeader::new(theme::small(ui, "How scheduling works")).id_salt("automation-help").show(ui, |ui| {
                        for text in [
                            "Ask an agent to create a monitor or cron job. Cron schedules use UTC; missed occurrences are skipped.",
                            "Checks and scheduling use no AI or context. Delivered events/prompts start an agent response and use tokens.",
                            "Monitors trigger once on exit 0; exit 1 keeps waiting. Other failures are shown on the task.",
                            "Commands run with local OS permissions, not in a sandbox. Use side-effect-free checks.",
                        ] {
                            ui.add(egui::Label::new(theme::small(ui, text)).wrap());
                        }
                    });
                });
            });
        ctx.data_mut(|data| data.insert_temp(filter_id, filter));
        if let Some((id, operation)) = action {
            if let Err(error) = self.change_job(&id, operation) {
                self.automation.error = Some(error);
            }
        }
        if let Some(id) = reschedule
            && let Err(error) = self.begin_reschedule(&id)
        {
            self.automation.error = Some(error);
        }
        if let Some(id) = edit_content
            && let Err(error) = self.begin_job_content(&id)
        {
            self.automation.error = Some(error);
        }
        if create && let Err(error) = self.begin_cron_form() {
            self.automation.error = Some(error);
        }
        if monitor && let Err(error) = self.begin_monitor_form() {
            self.automation.error = Some(error);
        }
        if let Some(id) = test
            && let Err(error) = self.request_monitor_test(&id)
        {
            self.automation.error = Some(error);
        }
        self.monitor_test_dialog(ctx);
        self.cron_form(ctx);
        self.reschedule_dialog(ctx);
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
fn job_counts(jobs: &[Job]) -> [usize; 3] {
    let mut counts = [0; 3];
    for job in jobs {
        counts[if job.completed {
            2
        } else if job.enabled {
            0
        } else {
            1
        }] += 1;
    }
    counts
}
fn job_status(job: &Job, running: bool, colors: &theme::Palette) -> (&'static str, egui::Color32) {
    if job.dispatching {
        ("Delivering", colors.green)
    } else if job.error.is_some() {
        ("Needs review", colors.amber)
    } else if job.completed {
        ("Completed", colors.dim)
    } else if !job.enabled {
        ("Paused", colors.amber)
    } else if running {
        ("Checking", colors.green)
    } else if job.pending.is_some() {
        ("Waiting to deliver", colors.amber)
    } else {
        ("Active", colors.green)
    }
}
fn next_activity(job: &Job, at: i64) -> String {
    if job.dispatching {
        "Sending to creator chat".into()
    } else if job.completed {
        format!("{} delivered", job.deliveries)
    } else if !job.enabled {
        "Not scheduled".into()
    } else if job.pending.is_some() {
        "Waiting for creator chat".into()
    } else {
        let prefix = if matches!(job.trigger, Trigger::Monitor { .. }) {
            "Check"
        } else {
            "Prompt"
        };
        let seconds = job.next_run.saturating_sub(at);
        if seconds <= 0 {
            format!("{prefix} due")
        } else if seconds < 60 {
            format!("{prefix} in {seconds}s")
        } else if seconds < 3600 {
            format!("{prefix} in {}m", (seconds + 59) / 60)
        } else {
            format!(
                "{prefix}: {}",
                chrono::DateTime::from_timestamp(job.next_run, 0)
                    .map(|t| t.format("%b %d, %H:%M UTC").to_string())
                    .unwrap_or_else(|| "Unavailable".into())
            )
        }
    }
}
fn cadence(trigger: &Trigger) -> String {
    match trigger {
        Trigger::Monitor {
            interval_seconds: 60,
            ..
        } => "Monitor · Every minute".into(),
        Trigger::Monitor {
            interval_seconds, ..
        } => format!("Monitor · Every {interval_seconds}s"),
        Trigger::Cron { schedule } => {
            let fields: Vec<_> = schedule.split_whitespace().collect();
            if let [minute, hour, "*", "*", day] = fields.as_slice()
                && let (Ok(minute), Ok(hour)) = (minute.parse::<u8>(), hour.parse::<u8>())
                && minute < 60
                && hour < 24
                && let Some(day) = match *day {
                    "*" => Some("Daily"),
                    "1-5" => Some("Weekdays"),
                    "1" => Some("Mondays"),
                    "2" => Some("Tuesdays"),
                    "3" => Some("Wednesdays"),
                    "4" => Some("Thursdays"),
                    "5" => Some("Fridays"),
                    "6" => Some("Saturdays"),
                    "0" | "7" => Some("Sundays"),
                    _ => None,
                }
            {
                format!("Cron · {day} at {hour:02}:{minute:02} UTC")
            } else {
                format!("Cron · {schedule} · UTC")
            }
        }
    }
}
fn row_text(ui: &egui::Ui, rect: egui::Rect, text: &str, size: f32, color: egui::Color32) {
    let mut layout = egui::text::LayoutJob::simple(
        text.into(),
        egui::FontId::proportional(size),
        color,
        rect.width().max(0.0),
    );
    layout.wrap.max_rows = 1;
    layout.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(layout);
    ui.painter().galley(rect.min, galley, color);
}
fn job_row(ui: &mut egui::Ui, job: &Job, running: bool, open: bool) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let wide = ui.available_width() >= 650.0;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), if wide { 72.0 } else { 88.0 }),
        Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!(
                "{} · {} details",
                job.name,
                if open { "Hide" } else { "Show" }
            ),
        )
    });
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 6, colors.surface);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            6,
            Stroke::new(1.0, colors.accent),
            egui::StrokeKind::Inside,
        );
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 14.0, rect.top() + 32.0),
        Vec2::splat(26.0),
    );
    ui.painter().rect_stroke(
        icon_rect,
        7,
        Stroke::new(1.0, colors.border),
        egui::StrokeKind::Inside,
    );
    icons::draw(
        ui,
        icon_rect,
        if matches!(job.trigger, Trigger::Monitor { .. }) {
            Icon::Terminal
        } else {
            Icon::Clock
        },
        colors.muted,
    );
    let left = rect.left() + 40.0;
    let status_left = rect.right() - if wide { 310.0 } else { 126.0 };
    let line = |x, y, width| {
        egui::Rect::from_min_size(egui::pos2(x, rect.top() + y), Vec2::new(width, 18.0))
    };
    row_text(
        ui,
        line(left, 17.0, status_left - left - 12.0),
        &job.name,
        13.0,
        colors.text,
    );
    let cadence = cadence(&job.trigger);
    row_text(
        ui,
        line(
            left,
            38.0,
            if wide {
                status_left - left - 12.0
            } else {
                rect.right() - left - 28.0
            },
        ),
        &format!("{cadence} · {}", job.creator.name),
        11.0,
        colors.muted,
    );
    let (status, color) = job_status(job, running, colors);
    theme::status_dot(ui, egui::pos2(status_left, rect.top() + 25.0), color, 2.5);
    row_text(
        ui,
        line(status_left + 9.0, 18.0, if wide { 125.0 } else { 96.0 }),
        status,
        11.0,
        color,
    );
    let next = next_activity(job, now());
    row_text(
        ui,
        if wide {
            line(rect.right() - 166.0, 20.0, 140.0)
        } else {
            line(left, 59.0, rect.width() - 70.0)
        },
        &next,
        11.0,
        colors.muted,
    );
    let center = egui::pos2(rect.right() - 10.0, rect.top() + 32.0);
    let points = if open {
        vec![
            center + egui::vec2(-4.0, -2.0),
            center + egui::vec2(0.0, 2.0),
            center + egui::vec2(4.0, -2.0),
        ]
    } else {
        vec![
            center + egui::vec2(-2.0, -4.0),
            center + egui::vec2(2.0, 0.0),
            center + egui::vec2(-2.0, 4.0),
        ]
    };
    ui.painter()
        .add(egui::Shape::line(points, Stroke::new(1.2, colors.dim)));
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!(
            "{}\n{cadence}\nCreated by {} ({})\n{next}\nClick to {} details",
            job.name,
            job.creator.name,
            job.creator.id,
            if open { "hide" } else { "show" }
        ))
}
#[cfg(test)]
mod tests;

fn timestamp(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| "Unavailable".into())
}
