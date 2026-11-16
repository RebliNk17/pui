//! Paused cron editing: preview-only until explicit Save; never resumes or delivers.
use super::*;
#[derive(Default)]
pub(super) struct Editor {
    id: String,
    original: String,
    draft: String,
    preview: Option<Result<Vec<i64>, String>>,
    base: i64,
    monitor: Option<(String, String)>, // interval/timeout drafts; command is never editable
    content: Option<(String, String)>,
}
pub(super) fn validate_content(name: &str, message: &str) -> Result<(), String> {
    if name.trim().is_empty()
        || name.len() > 320
        || name.chars().any(char::is_control)
        || message.trim().is_empty()
        || message.len() > 6000
        || message
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err("Supply a nonempty name (320 UTF-8 bytes) and prompt (6,000 UTF-8 bytes), without unsupported controls.".into());
    }
    Ok(())
}
pub(super) fn monitor_timing(interval: &str, timeout: &str) -> Result<(u64, u64), String> {
    let i = interval
        .trim()
        .parse::<u64>()
        .map_err(|_| "Interval must be an integer in seconds.")?;
    let t = timeout
        .trim()
        .parse::<u64>()
        .map_err(|_| "Timeout must be an integer in seconds.")?;
    if !(10..=86400).contains(&i) || !(1..=300).contains(&t) {
        return Err("Use an interval of 10–86,400 seconds and timeout of 1–300 seconds.".into());
    }
    Ok((i, t))
}
pub(super) fn preview(schedule: &str, after: i64) -> Result<Vec<i64>, String> {
    let cron = cron::Cron::parse(schedule)?;
    let mut time = after;
    let mut times = Vec::with_capacity(3);
    for _ in 0..3 {
        time = cron.next(time)?;
        times.push(time);
    }
    Ok(times)
}
impl Pui {
    pub(super) fn begin_reschedule(&mut self, id: &str) -> Result<(), String> {
        if self.demo || !self.automation.writable() {
            return Err("Schedule editing requires the scheduler-owning window.".into());
        }
        let job = self
            .automation
            .jobs
            .iter()
            .find(|j| j.id == id)
            .ok_or("Job no longer available.")?;
        if job.enabled
            || job.pending.is_some()
            || job.dispatching
            || self.automation.running.contains_key(id)
        {
            return Err(
                "Pause the job and finish pending delivery before editing its schedule.".into(),
            );
        }
        let (draft, monitor) = match &job.trigger {
            Trigger::Cron { schedule } => (schedule.clone(), None),
            Trigger::Monitor {
                interval_seconds,
                timeout_seconds,
                ..
            } => (
                String::new(),
                Some((interval_seconds.to_string(), timeout_seconds.to_string())),
            ),
        };
        self.automation.reschedule = Some(Editor {
            id: id.into(),
            original: serde_json::to_string(job).map_err(|_| "Cannot bind job identity.")?,
            draft,
            monitor,
            content: None,
            base: now(),
            preview: None,
        });
        Ok(())
    }
    pub(super) fn begin_job_content(&mut self, id: &str) -> Result<(), String> {
        self.begin_reschedule(id)?;
        let job = self.automation.jobs.iter().find(|j| j.id == id).unwrap();
        self.automation.reschedule.as_mut().unwrap().content =
            Some((job.name.clone(), job.message.clone()));
        Ok(())
    }
    fn save_reschedule(&mut self, editor: &Editor) -> Result<(), String> {
        if self.demo || !self.automation.writable() {
            return Err("Scheduler ownership changed. Nothing rescheduled.".into());
        }
        let job = self
            .automation
            .jobs
            .iter_mut()
            .find(|j| j.id == editor.id)
            .ok_or("Job no longer available.")?;
        if serde_json::to_string(job).map_err(|_| "Cannot bind job.")? != editor.original
            || job.enabled
            || job.pending.is_some()
            || job.dispatching
        {
            return Err("Job changed while editing. Reopen the schedule editor.".into());
        }
        let previous = job.clone();
        if let Some((name, message)) = &editor.content {
            validate_content(name, message)?;
            job.name = name.trim().into();
            job.message = message.trim().into();
        } else {
            match (&editor.monitor, &mut job.trigger) {
                (
                    Some((interval, timeout)),
                    Trigger::Monitor {
                        interval_seconds,
                        timeout_seconds,
                        ..
                    },
                ) => {
                    let (i, t) = monitor_timing(interval, timeout)?;
                    *interval_seconds = i;
                    *timeout_seconds = t;
                    job.next_run = now() + i as i64;
                }
                (None, Trigger::Cron { schedule }) => {
                    let text = editor.draft.trim();
                    let next = preview(text, now())?[0];
                    *schedule = text.into();
                    job.next_run = next;
                }
                _ => return Err("Job trigger changed. Reopen editor.".into()),
            }
        }
        if let Err(error) = self.automation.persist() {
            if let Some(job) = self.automation.jobs.iter_mut().find(|j| j.id == editor.id) {
                *job = previous;
            }
            return Err(error);
        }
        Ok(())
    }
    pub(super) fn reschedule_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut editor) = self.automation.reschedule.take() else {
            return;
        };
        if !self.automation.open
            || self.demo
            || !self.automation.writable()
            || egui::Popup::is_any_open(ctx)
        {
            return;
        }
        let own = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("cron-reschedule"));
        if prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            return;
        }
        let valid = self
            .automation
            .jobs
            .iter()
            .find(|j| j.id == editor.id)
            .is_some_and(|j| {
                serde_json::to_string(j).ok().as_deref() == Some(editor.original.as_str())
            });
        if !valid {
            self.automation.error =
                Some("Job changed while editing. Reopen the schedule editor.".into());
            return;
        }
        ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        let response = prompt_ui::modal(ctx, egui::Id::new("cron-reschedule"), 420.0, |ui| {
            prompt_ui::heading(
                ui,
                if editor.content.is_some() {
                    "Edit paused job prompt"
                } else if editor.monitor.is_some() {
                    "Edit paused monitor timing"
                } else {
                    "Edit paused cron schedule"
                },
                if editor.content.is_some() {
                    "Saving keeps this job paused. Its schedule, command and creator stay unchanged. Nothing is delivered now; future delivered prompts use tokens."
                } else if editor.monitor.is_some() {
                    "Saving keeps this monitor paused. Its command and follow-up stay unchanged; no shell check is run. Resume separately when ready."
                } else {
                    "Five numeric fields in UTC. Saving keeps this job paused; no prompt or check is run. Resume separately when ready."
                },
            );
            if let Some((name, message)) = &mut editor.content {
                ui.label(theme::small(ui, "Job name · up to 320 UTF-8 bytes"));
                ui.add(
                    egui::TextEdit::singleline(name)
                        .id_salt((&editor.id, "job-name"))
                        .char_limit(320)
                        .desired_width(ui.available_width()),
                );
                ui.label(theme::small(
                    ui,
                    "Prompt for the original creator · up to 6,000 UTF-8 bytes",
                ));
                ui.add(
                    egui::TextEdit::multiline(message)
                        .id_salt((&editor.id, "job-message"))
                        .char_limit(6000)
                        .desired_rows(4)
                        .desired_width(ui.available_width()),
                );
                editor.preview = Some(validate_content(name, message).map(|_| vec![]));
            } else if let Some((interval, timeout)) = &mut editor.monitor {
                ui.label(theme::small(ui, "Check interval · 10–86,400 seconds"));
                ui.add(
                    egui::TextEdit::singleline(interval)
                        .id_salt((&editor.id, "monitor-interval"))
                        .char_limit(5)
                        .desired_width(ui.available_width()),
                );
                ui.label(theme::small(ui, "Per-check timeout · 1–300 seconds"));
                ui.add(
                    egui::TextEdit::singleline(timeout)
                        .id_salt((&editor.id, "monitor-timeout"))
                        .char_limit(3)
                        .desired_width(ui.available_width()),
                );
                editor.preview = Some(
                    monitor_timing(interval, timeout).map(|(i, _)| vec![editor.base + i as i64]),
                );
            } else {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut editor.draft)
                        .id_salt((&editor.id, "cron-fields"))
                        .char_limit(256)
                        .desired_width(ui.available_width()),
                );
                if field.changed() {
                    editor.preview = None;
                }
                if editor.preview.is_none() {
                    editor.preview = Some(preview(&editor.draft, editor.base));
                }
            }
            match editor.preview.as_ref().unwrap() {
                Ok(times) => {
                    if editor.content.is_none() {
                        ui.label(theme::small(
                        ui,
                        if editor.monitor.is_some(){"Earliest check after a future Resume uses the saved interval. Saving does not start it."}else{"Next three matching times (UTC) · preview from editor opening"},
                    ));
                    }
                    if editor.monitor.is_none() && editor.content.is_none() {
                        time_preview::show(ui, times);
                    }
                }
                Err(error) => {
                    ui.add(egui::Label::new(theme::small(ui, error)).wrap());
                }
            }
            prompt_ui::footer(ui, |ui| {
                let save = prompt_ui::button(
                    ui,
                    if editor.content.is_some() {
                        "Save prompt"
                    } else if editor.monitor.is_some() {
                        "Save timing"
                    } else {
                        "Save schedule"
                    },
                    prompt_ui::Button::Primary,
                    editor.preview.as_ref().is_some_and(|r| r.is_ok()),
                );
                #[cfg(test)]
                ui.ctx().data_mut(|d| {
                    d.insert_temp(
                        egui::Id::new("cron-save-schedule"),
                        (save.id, save.rect, save.enabled()),
                    )
                });
                let cancel = prompt_ui::button(ui, "Cancel", prompt_ui::Button::Secondary, true);
                (save.clicked(), cancel.clicked())
            })
        });
        let (save, cancel) = response.inner.inner;
        if save {
            if let Err(error) = self.save_reschedule(&editor) {
                self.automation.error = Some(error);
            }
        } else if !cancel && !response.should_close() {
            self.automation.reschedule = Some(editor);
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
