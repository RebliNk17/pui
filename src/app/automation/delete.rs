//! Confirmed UI deletes route to the scheduler owner, not the creator chat.
use super::*;

impl Pui {
    pub(in crate::app) fn can_delete_scheduled_job(&self) -> bool {
        !self.demo
            && !self.agents.exiting
            && !self.automation.failed
            && self.automation.deleting.is_none()
            && (self.automation.writable()
                || (self.automation.home.is_some()
                    && self.sync.enabled()
                    && self.sync.error.is_none()))
    }

    pub(super) fn request_scheduled_job_delete(&mut self, id: &str) -> Result<(), String> {
        if !self.can_delete_scheduled_job() {
            return Err("Scheduled job deletion is unavailable in this window.".into());
        }
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid job ID")?;
        if self.automation.writable() {
            self.delete_scheduled_job(id)?;
            self.automation.error = None;
            return Ok(());
        }
        let home = self
            .automation
            .home
            .clone()
            .ok_or("Automation storage unavailable")?;
        self.sync.delete_scheduled_job(home, id.into())?;
        self.automation.deleting = Some(id.into());
        self.automation.error = None;
        Ok(())
    }

    pub(in crate::app) fn delete_remote_scheduled_job(
        &mut self,
        home: &std::path::Path,
        id: &str,
    ) -> Result<(), String> {
        if self.demo || self.agents.exiting || self.automation.home.as_deref() != Some(home) {
            return Err("This window does not own the requested scheduler.".into());
        }
        self.delete_scheduled_job(id)
    }

    pub(super) fn delete_scheduled_job(&mut self, id: &str) -> Result<(), String> {
        if !self.automation.writable() {
            return Err(
                "Scheduler ownership changed; review Scheduled tasks and try again.".into(),
            );
        }
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid job ID")?;
        // Persist first so failure cannot hide a job that still exists on disk.
        let mut jobs = self.automation.jobs.clone();
        jobs.retain(|job| job.id != id);
        if let Some(home) = &self.automation.home {
            store::save(home, &jobs)?;
        }
        self.automation.jobs = jobs;
        self.automation.running.remove(id); // cancels only this job's shell check
        self.cancel_deleted_monitor_test(id);
        self.automation.mail.retain(|_, (job, _)| job != id);
        // Already accepted creator messages stay untouched, as the dialog promises.
        Ok(())
    }

    pub(in crate::app) fn scheduled_job_delete_result(
        &mut self,
        id: &str,
        result: Result<(), String>,
    ) {
        if self.automation.deleting.as_deref() != Some(id) {
            return;
        }
        self.automation.deleting = None;
        match result {
            Ok(()) => {
                // Refresh the durable result, not an optimistic local store write.
                self.automation.refresh();
                if !self.automation.failed {
                    self.automation.error = None;
                }
            }
            Err(error) => {
                self.automation.error = Some(format!(
                    "Delete was not acknowledged: {error} It was not retried. Refresh/review the task before trying again."
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests;
