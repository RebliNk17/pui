//! Explicit one-shot diagnostic; never enters scheduler delivery state.
use super::*;
pub(super) struct Pending {
    job: String,
    original: String,
    run: Running,
}
impl Pui {
    pub(super) fn request_monitor_test(&mut self, id: &str) -> Result<(), String> {
        if self.active_permission() == settings::PermissionMode::Plan
            || self.demo
            || self.is_mirror()
            || self.agents.exiting
            || !self.automation.writable()
            || self.automation.precheck.is_some()
        {
            return Err("Monitor testing unavailable or already running.".into());
        }
        let job = self
            .automation
            .jobs
            .iter()
            .find(|j| j.id == id)
            .ok_or("Job unavailable.")?;
        if job.enabled
            || job.pending.is_some()
            || job.dispatching
            || !matches!(job.trigger, Trigger::Monitor { .. })
        {
            return Err("Pause a monitor with no pending delivery before testing.".into());
        }
        self.automation.test_confirm = Some((
            id.into(),
            serde_json::to_string(job).map_err(|_| "Invalid job")?,
        ));
        Ok(())
    }
    fn start_monitor_test(&mut self, id: String, original: String) -> Result<(), String> {
        if self.active_permission() == settings::PermissionMode::Plan
            || self.automation.precheck.is_some()
            || self.automation.running.len() >= 4
            || !self.automation.writable()
            || self.demo
            || self.is_mirror()
            || self.agents.exiting
        {
            return Err("Monitor testing is busy or unavailable.".into());
        }
        let job = self
            .automation
            .jobs
            .iter()
            .find(|j| j.id == id)
            .ok_or("Job unavailable.")?;
        if job.enabled
            || job.pending.is_some()
            || job.dispatching
            || serde_json::to_string(job).ok().as_deref() != Some(&original)
        {
            return Err("Monitor changed; inspect again.".into());
        }
        let Trigger::Monitor {
            command,
            timeout_seconds,
            ..
        } = &job.trigger
        else {
            return Err("Not a monitor.".into());
        };
        let command = command.clone();
        let timeout = *timeout_seconds;
        let cwd = job.creator.cwd.clone();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let ctx = self.ctx.clone();
        let worker = std::thread::Builder::new()
            .name("pui-monitor-test".into())
            .spawn(move || {
                let check = runner::run(&cwd, &command, timeout, signal);
                let _ = tx.send(check);
                ctx.request_repaint();
            });
        if worker.is_err() {
            return Err("Cannot start monitor test.".into());
        }
        self.automation.precheck = Some(Pending {
            job: id,
            original,
            run: Running { cancel, rx },
        });
        Ok(())
    }
    pub(super) fn poll_monitor_test(&mut self) {
        let Some(pending) = &self.automation.precheck else {
            return;
        };
        let check = match pending.run.rx.try_recv() {
            Ok(check) => check,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(_) => {
                self.automation.precheck = None;
                self.automation.error = Some("Monitor test ended without evidence.".into());
                return;
            }
        };
        let pending = self.automation.precheck.take().unwrap();
        if let Some(job) = self.automation.jobs.iter_mut().find(|j| {
            j.id == pending.job
                && serde_json::to_string(j).ok().as_deref() == Some(&pending.original)
        }) {
            job.record_check(&check, now());
            job.evidence.as_mut().unwrap().mark_test();
            let _ = self.automation.persist();
        } else {
            self.automation.error = Some(
                "Monitor changed during test; result was not attached to a different job state."
                    .into(),
            );
        }
    }
    pub(super) fn monitor_test_dialog(&mut self, ctx: &egui::Context) {
        let Some((id, original)) = self.automation.test_confirm.clone() else {
            return;
        };
        let own = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("monitor-test-confirm"),
        );
        if !self.automation.open
            || self.demo
            || !self.automation.writable()
            || egui::Popup::is_any_open(ctx)
            || prompt_ui::modal_owns_current_pass(ctx)
            || ctx.memory(|m| m.top_modal_layer().is_some_and(|l| l != own))
        {
            self.automation.test_confirm = None;
            return;
        }
        let Some(job) =
            self.automation.jobs.iter().find(|j| {
                j.id == id && serde_json::to_string(j).ok().as_deref() == Some(&original)
            })
        else {
            self.automation.test_confirm = None;
            return;
        };
        let Trigger::Monitor {
            command,
            timeout_seconds,
            ..
        } = &job.trigger
        else {
            self.automation.test_confirm = None;
            return;
        };
        ctx.input_mut(|i|i.events.retain(|e|!matches!(e,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
        let choice=confirmations::Dialog::new("monitor-test-confirm","Run this monitor once?","Runs the exact shell command with local OS permissions, not in a sandbox. Use side-effect-free checks. Result is retained locally; no follow-up prompt is delivered and the monitor remains paused.","Cancel","Run test").show(ctx,|ui|{ui.add(egui::Label::new(theme::small(ui,format!("{}\nTimeout: {timeout_seconds}s",job.creator.cwd.display()))).wrap());ui.add(egui::Label::new(RichText::new(command).monospace()).wrap().selectable(false));});
        if let Some(choice) = choice {
            self.automation.test_confirm = None;
            if choice == confirmations::Choice::Confirm
                && let Err(error) = self.start_monitor_test(id, original)
            {
                self.automation.error = Some(error);
            }
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
