//! A separate no-model check process serves a waiting builder; final acceptance
//! remains independent. Ordinary exact-command permissions are never auto-answered.
use super::*;
#[derive(Default)]
pub(super) struct State {
    pub builder: Option<(String, String)>,
    pub criterion: String,
    pub fingerprint: String,
    pub count: u8,
    pub attempt: u8,
    pub run: Option<String>,
}
impl Pui {
    pub(super) fn task_feedback_request(&mut self, index: usize, record: &Value) -> bool {
        if record["type"] != "extension_ui_request"
            || record["method"] != "input"
            || record["title"] != "Pui workflow check"
        {
            return false;
        }
        let rpc = protocol::string(record, "id").to_owned();
        let error = (|| -> Result<(String, String), String> {
            let task = self
                .tasks
                .live_task()
                .cloned()
                .ok_or("No running workflow.")?;
            if self.tasks.processes[index].role != Role::Builder
                || task.phase != Phase::Building
                || task.pause_requested
                || self.tasks.feedback.builder.is_some()
            {
                return Err("Focused check unavailable for this stage.".into());
            }
            let attempt = task.latest().unwrap().number;
            if self.tasks.feedback.attempt != attempt {
                self.tasks.feedback.count = 0;
                self.tasks.feedback.attempt = attempt;
            }
            if self.tasks.feedback.count >= 4 {
                return Err("Four focused checks used for this attempt; finish implementation for independent acceptance.".into());
            }
            let value: Value = serde_json::from_str(protocol::string(record, "placeholder"))
                .map_err(|_| "Invalid focused check request")?;
            let criterion = protocol::string(&value, "criterion");
            let check = task
                .approved
                .as_ref()
                .unwrap()
                .criteria
                .iter()
                .find(|c| c.id == criterion && !c.command.trim().is_empty())
                .ok_or("Unknown or inspection-only criterion; no command executed.")?;
            Ok((check.id.clone(), self.tasks.processes[index].run.clone()))
        })();
        match error {
            Err(error) => {
                let _=self.tasks.processes[index].client.send(json!({"type":"extension_ui_response","id":rpc,"value":json!({"error":error}).to_string()}));
            }
            Ok((criterion, builder)) => {
                self.tasks.feedback.count += 1;
                self.tasks.feedback.criterion = criterion;
                self.tasks.feedback.builder = Some((builder, rpc));
                self.task_fingerprint(FingerprintStep::FeedbackBefore);
            }
        }
        true
    }
    pub(super) fn task_feedback_fingerprinted(
        &mut self,
        step: FingerprintStep,
        fingerprint: String,
    ) {
        if self.tasks.feedback.builder.is_none() {
            return;
        }
        if step == FingerprintStep::FeedbackBefore {
            self.tasks.feedback.fingerprint = fingerprint;
            self.task_launch(Role::Checks);
        } else {
            if fingerprint != self.tasks.feedback.fingerprint {
                self.tasks.block("Source changed during focused check; no builder continuation or acceptance evidence was authorized.");
                return;
            }
            let Some((builder, rpc)) = self.tasks.feedback.builder.take() else {
                return;
            };
            let feedback = self.tasks.live_task().unwrap().feedback.last().cloned();
            let check_run = self.tasks.feedback.run.take();
            if let Some(run) = check_run {
                if let Some(index) = self.tasks.processes.iter().position(|p| p.run == run) {
                    let process = self.tasks.processes.remove(index);
                    self.tasks.histories.insert(process.run, process.state);
                    process.client.retire();
                }
            }
            if let Some(index) = self
                .tasks
                .processes
                .iter()
                .position(|p| p.run == builder && !p.ended)
            {
                if let Some(pending) = self.tasks.processes[index].pending.as_mut() {
                    pending.started = std::time::Instant::now();
                }
                let _=self.tasks.processes[index].client.send(json!({"type":"extension_ui_response","id":rpc,"value":json!({"feedback":feedback,"acceptance":false,"note":"Focused feedback only; independent acceptance checks and fresh review still follow."}).to_string()}));
            }
        }
    }
}
