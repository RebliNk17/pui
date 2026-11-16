//! Native recurring prompts start paused, bound to one retained conversation owner.
use super::*;
struct Owner {
    record: agents::AgentRecord,
    active: String,
}
pub(super) struct Form {
    owner: Owner,
    name: String,
    message: String,
    schedule: String,
    base: i64,
    preview: Option<Result<Vec<i64>, String>>,
    monitor: Option<Monitor>,
}
struct Monitor {
    command: String,
    interval: String,
    timeout: String,
    acknowledged: bool,
}
fn validate_monitor(monitor: &Monitor) -> Result<(u64, u64), String> {
    if monitor.command.trim().is_empty()
        || monitor.command.len() > 16000
        || monitor.command.contains('\0')
        || !monitor.acknowledged
    {
        return Err("Supply a bounded command and acknowledge local shell execution before creating this paused monitor.".into());
    }
    reschedule::monitor_timing(&monitor.interval, &monitor.timeout)
}
impl Pui {
    fn cron_owner(&self) -> Result<Owner, String> {
        if self.demo
            || self.is_mirror()
            || self.agents.exiting
            || !self.automation.writable()
            || !self.ultracode_ready
            || self.active_permission() == settings::PermissionMode::Plan
            || self.loading_chat
            || self.deletion.in_progress()
        {
            return Err("Open a saved writable conversation outside Plan mode in the scheduler-owning window.".into());
        }
        let active = self.agents.active.clone();
        let record = self
            .agent_records()
            .into_iter()
            .find(|r| r.id == active && r.access != "read-only")
            .ok_or("Save the current conversation before creating a recurring prompt.")?;
        Ok(Owner { record, active })
    }
    pub(super) fn begin_cron_form(&mut self) -> Result<(), String> {
        let owner = self.cron_owner()?;
        if self.automation.jobs.len() >= 128 {
            return Err("128 retained jobs reached.".into());
        }
        self.automation.create = Some(Form {
            owner,
            name: String::new(),
            message: String::new(),
            schedule: "0 9 * * 1-5".into(),
            base: now(),
            preview: None,
            monitor: None,
        });
        Ok(())
    }
    pub(super) fn begin_monitor_form(&mut self) -> Result<(), String> {
        self.begin_cron_form()?;
        self.automation.create.as_mut().unwrap().monitor = Some(Monitor {
            command: String::new(),
            interval: "60".into(),
            timeout: "30".into(),
            acknowledged: false,
        });
        Ok(())
    }
    fn save_cron_form(&mut self, form: &Form) -> Result<(), String> {
        let owner = self.cron_owner()?;
        if owner.active != form.owner.active
            || owner.record.path != form.owner.record.path
            || owner.record.cwd != form.owner.record.cwd
        {
            return Err("Conversation owner changed. Reopen the form.".into());
        }
        if self.automation.jobs.len() >= 128 {
            return Err("128 retained jobs reached.".into());
        }
        reschedule::validate_content(&form.name, &form.message)?;
        let (trigger, next) = if let Some(monitor) = &form.monitor {
            let (interval_seconds, timeout_seconds) = validate_monitor(monitor)?;
            (
                Trigger::Monitor {
                    command: monitor.command.clone(),
                    interval_seconds,
                    timeout_seconds,
                },
                now() + interval_seconds as i64,
            )
        } else {
            let schedule = form.schedule.trim();
            (
                Trigger::Cron {
                    schedule: schedule.into(),
                },
                reschedule::preview(schedule, now())?[0],
            )
        };
        let job = Job {
            id: uuid::Uuid::new_v4().to_string(),
            name: form.name.trim().into(),
            creator: owner.record,
            trigger,
            message: form.message.trim().into(),
            enabled: false,
            completed: false,
            next_run: next,
            last_run: None,
            evidence: None,
            previous_checks: Vec::new(),
            deliveries: 0,
            pending: None,
            dispatching: false,
            error: None,
        };
        job.validate()?;
        let id = job.id.clone();
        self.automation.jobs.push(job);
        if let Err(error) = self.automation.persist() {
            self.automation.jobs.retain(|j| j.id != id);
            return Err(error);
        }
        Ok(())
    }
    pub(super) fn cron_form(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.automation.create.take() else {
            return;
        };
        if !self.automation.open || self.cron_owner().is_err() || egui::Popup::is_any_open(ctx) {
            return;
        }
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("create-recurring-prompt"),
        );
        if prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            return;
        }
        ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        let response = prompt_ui::modal(
            ctx,
            egui::Id::new("create-recurring-prompt"),
            420.0,
            |ui| {
                prompt_ui::heading(
                    ui,
                    if form.monitor.is_some() {
                        "New shell monitor"
                    } else {
                        "New recurring prompt"
                    },
                    "Created paused. Nothing is sent now. Resume separately; delivered prompts wake this conversation and use tokens. Runs only while Pui is open; missed times are skipped.",
                );
                ui.add(
                    egui::Label::new(theme::small(
                        ui,
                        format!(
                            "Creator: {}\n{}",
                            form.owner.record.name,
                            form.owner.record.path.display()
                        ),
                    ))
                    .wrap(),
                );
                ui.label(theme::small(ui, "Name"));
                ui.add(
                    egui::TextEdit::singleline(&mut form.name)
                        .id_salt("cron-new-name")
                        .char_limit(320)
                        .desired_width(ui.available_width()),
                );
                ui.label(theme::small(ui, "Prompt"));
                ui.add(
                    egui::TextEdit::multiline(&mut form.message)
                        .id_salt("cron-new-message")
                        .char_limit(6000)
                        .desired_rows(3)
                        .desired_width(ui.available_width()),
                );
                if let Some(monitor) = &mut form.monitor {
                    ui.add(
                        egui::Label::new(theme::small(
                            ui,
                            format!("Shell workspace: {}", form.owner.record.cwd.display()),
                        ))
                        .wrap(),
                    );
                    ui.label(theme::small(
                        ui,
                        "Exact shell command · use side-effect-free checks",
                    ));
                    let command = ui.add(
                        egui::TextEdit::multiline(&mut monitor.command)
                            .id_salt("monitor-new-command")
                            .char_limit(16000)
                            .desired_rows(2)
                            .desired_width(ui.available_width()),
                    );
                    if command.changed() {
                        monitor.acknowledged = false;
                    }
                    ui.label(theme::small(
                        ui,
                        "Interval seconds (10–86400) / timeout seconds (1–300)",
                    ));
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut monitor.interval)
                                .id_salt("monitor-new-interval")
                                .char_limit(5)
                                .desired_width(80.0),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut monitor.timeout)
                                .id_salt("monitor-new-timeout")
                                .char_limit(3)
                                .desired_width(80.0),
                        );
                    });
                    ui.checkbox(&mut monitor.acknowledged,"I understand Resume runs this shell command with local OS permissions, not in a sandbox.");
                    ui.add(egui::Label::new(theme::small(ui,"Exit 0 triggers once; exit 1 waits; other failures appear on the job. No check runs while creating." )).wrap());
                    form.preview = Some(validate_monitor(monitor).map(|_| vec![]));
                } else {
                    ui.label(theme::small(ui, "Schedule · five numeric fields · UTC"));
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut form.schedule)
                            .id_salt("cron-new-schedule")
                            .char_limit(256)
                            .desired_width(ui.available_width()),
                    );
                    if field.changed() {
                        form.preview = None;
                    }
                    if form.preview.is_none() {
                        form.preview = Some(reschedule::preview(&form.schedule, form.base));
                    }
                }
                match form.preview.as_ref().unwrap() {
                    Ok(times) => {
                        time_preview::show(ui, times);
                    }
                    Err(error) => {
                        ui.add(egui::Label::new(theme::small(ui, error)).wrap());
                    }
                }
                let valid = reschedule::validate_content(&form.name, &form.message).is_ok()
                    && form.preview.as_ref().is_some_and(|r| r.is_ok());
                prompt_ui::footer(ui, |ui| {
                    let save =
                        prompt_ui::button(ui, "Create paused", prompt_ui::Button::Primary, valid);
                    #[cfg(test)]
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(
                            egui::Id::new("create-cron-save"),
                            (save.id, save.rect, save.enabled()),
                        )
                    });
                    let cancel =
                        prompt_ui::button(ui, "Cancel", prompt_ui::Button::Secondary, true);
                    (save.clicked(), cancel.clicked())
                })
            },
        );
        let (save, cancel) = response.inner.inner;
        if save {
            if let Err(error) = self.save_cron_form(&form) {
                self.automation.error = Some(error);
            }
        } else if !cancel && !response.should_close() {
            self.automation.create = Some(form);
        }
        ctx.input_mut(|i| {
            i.events.retain(|e| {
                !matches!(
                    e,
                    egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Paste(_)
                )
            })
        });
    }
}
#[cfg(test)]
mod tests;
