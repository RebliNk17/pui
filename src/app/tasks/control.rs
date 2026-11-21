//! Background run handles and explicit owner-only controls. Reading a handle
//! never grants execution, changes scope, or replays an uncertain operation.
use super::*;

pub(super) struct Wait {
    pub source: String,
    pub rpc: String,
    pub control: String,
    pub task: String,
    pub deadline: std::time::Instant,
}
pub(super) struct Start {
    pub request: String,
    pub task: String,
    pub reply: Receiver<Result<(), String>>,
    pub owner: String,
    pub permission: settings::PermissionMode,
}
impl Pui {
    pub(super) fn workflow_handle_control(&mut self, record: &Value, value: &Value) -> bool {
        if value["action"] == "workflow_tool_guard" {
            // Serialize workflow ownership conservatively across this window,
            // including parked chats and restored handles. Pi still owns tool
            // execution and its ordinary permission checks.
            let blocked = self.tasks.live.is_some()
                || self.tasks.job.is_some()
                || self.tasks.start.is_some()
                || !self.tasks.retiring.is_empty()
                || self.task_operations_busy()
                || self.task_operations_confirming()
                || self.tasks.processes.iter().any(|process| !process.ended);
            let source = self.ensure_agent();
            self.workflow_send(&source,protocol::string(record,"id"),json!({"blocked":blocked,"reason":"A workflow owns this window's execution resources. Discussion and inspection remain available; wait, pause or cancel before conflicting tools. This is not an OS sandbox."}));
            return true;
        }
        if value["action"] != "workflow_control" {
            return false;
        }
        let rpc = protocol::string(record, "id").to_owned();
        let source = self.ensure_agent();
        let id = protocol::string(value, "workflow_id");
        if value["operation"] == "close_wait" {
            self.tasks.waits.retain(|wait| {
                wait.source != source || wait.rpc != protocol::string(value, "wait_id")
            });
            return true;
        }
        let action = protocol::string(value, "operation");
        let result = (|| -> Result<Option<Value>, String> {
            let task = self
                .tasks
                .records
                .iter()
                .find(|t| t.id == id)
                .ok_or("Unknown workflow.")?;
            if !self.task_in_chat(task) || self.is_mirror() {
                return Err(
                    "Only the creating owning chat can control or inspect this workflow handle."
                        .into(),
                );
            }
            match action {
                "status" => Ok(Some(domain::outcome::summary(task))),
                "wait" => {
                    if !task.phase.active() {
                        return Ok(Some(domain::outcome::summary(task)));
                    }
                    if self.tasks.waits.len() >= 32 {
                        return Err("Too many workflow waits; use the existing wait.".into());
                    }
                    if self
                        .tasks
                        .waits
                        .iter()
                        .any(|wait| wait.source == source && wait.rpc == rpc)
                    {
                        return Err("Duplicate workflow wait request.".into());
                    }
                    let deadline = std::time::Instant::now()
                        + Duration::from_millis(
                            value["timeout_ms"]
                                .as_u64()
                                .unwrap_or(600000)
                                .clamp(1000, 1800000),
                        );
                    self.tasks.waits.push(Wait {
                        source: source.clone(),
                        rpc: rpc.clone(),
                        control: protocol::string(value, "control_id").into(),
                        task: id.into(),
                        deadline,
                    });
                    self.ctx.request_repaint_after(
                        deadline.saturating_duration_since(std::time::Instant::now()),
                    );
                    Ok(None)
                }
                "continue" => {
                    if self.readonly_worker()
                        || matches!(
                            self.active_permission(),
                            settings::PermissionMode::Plan | settings::PermissionMode::DontAsk
                        )
                        || self.tasks.live.is_some()
                        || self.task_operations_busy()
                    {
                        return Err("Workflow continuation unavailable in this state.".into());
                    }
                    self.task_operation_guard(task)?;
                    let boundary = task.continuation_boundary()?;
                    let detail = format!(
                        "Workflow: {}\nWorkspace: {}\nBoundary: {:?}\nCandidate: {}\nStart only the never-started checks or a NEW paid read-only review of already completed check evidence. Exact command permissions remain separate. No builder, failed-command retry, repair-budget reset, install, apply or commit. Source must match before execution.",
                        task.objective,
                        task.workspace.as_ref().unwrap_or(&task.source).display(),
                        boundary,
                        task.latest().unwrap().fingerprint.as_deref().unwrap()
                    );
                    self.tasks.operations.review_scope = Some(self.operation_scope());
                    self.tasks.operations.decision = Some(operations::Decision::Continue {
                        expected: task.clone(),
                        boundary,
                        detail,
                    });
                    Ok(Some(
                        json!({"workflow_id":id,"review_required":true,"started":false,"verified":false,"note":"Native continuation confirmation opened; execution has not started."}),
                    ))
                }
                "pause" | "cancel" => {
                    if self.readonly_worker()
                        || matches!(
                            self.active_permission(),
                            settings::PermissionMode::Plan | settings::PermissionMode::DontAsk
                        )
                    {
                        return Err("This chat cannot change workflow execution.".into());
                    }
                    if self.tasks.live.as_deref() != Some(id) || !task.phase.active() {
                        return Err("Workflow is not executing here.".into());
                    }
                    if action == "cancel" {
                        self.tasks.stop();
                    } else {
                        self.tasks.live_mut().unwrap().pause_requested = true;
                        self.tasks.save();
                    }
                    Ok(Some(
                        json!({"workflow_id":id,"cancelled":action=="cancel","outcome":if action=="cancel" {"not_verified"} else {"running"},"pause_requested":action=="pause","note":if action=="pause" {"The current operation may finish. No next operation will dispatch; continuation requires an explicit scoped recovery decision."} else {"Stopped; effects and files are retained, not undone."}}),
                    ))
                }
                _ => Err(
                    "Supported workflow controls: status, wait, pause, cancel, continue.".into(),
                ),
            }
        })();
        match result {
            Ok(None) => {}
            Ok(Some(result)) => self.workflow_send(&source, &rpc, result),
            Err(error) => self.workflow_send(&source, &rpc, json!({"error":error})),
        }
        true
    }
    pub(super) fn workflow_send(&mut self, source: &str, rpc: &str, result: Value) {
        let response = json!({"type":"extension_ui_response","id":rpc,"value":result.to_string()});
        if source == self.agents.active {
            if let Some(client) = self.client.as_mut() {
                let _ = client.send(response);
            }
        } else if let Some(client) = self
            .agents
            .parked
            .get_mut(source)
            .and_then(|c| c.client.as_mut())
        {
            let _ = client.send(response);
        }
    }
    pub(super) fn poll_workflow_handles(&mut self) {
        if let Some(start) = self.tasks.start.as_ref() {
            let result = match start.reply.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Workflow start acknowledgment lost; inspect retained state. No replay.".into(),
                )),
            };
            if let Some(result) = result {
                let start = self.tasks.start.take().unwrap();
                if let Some(request) = self.tasks.requests.remove(&start.request) {
                    match result {
                        Ok(())
                            if self.agent_settings(&start.owner).permission == start.permission
                                && self.agents.nodes.get(&start.owner).is_some_and(|node| {
                                    node.access != "read-only" && !node.stopped
                                })
                                && self
                                    .tasks
                                    .records
                                    .iter()
                                    .any(|task| task.id == start.task && task.phase.active()) =>
                        {
                            self.workflow_send(&request.source,&request.rpc_id,json!({"approved":true,"started":true,"workflow_id":start.task,"verified":false,"outcome":"running","instruction":"Workflow runs independently while Pui is open. Continue discussion/read-only work; do not edit its source or candidate concurrently. Use pui_workflow_control wait for an event-driven result, status on demand, or pause/cancel. Starting is not success and does not apply changes."}));
                        }
                        Ok(()) => {
                            if self.tasks.live.as_deref() == Some(&start.task) {
                                self.tasks.stop();
                            }
                            if let Some(task) =
                                self.tasks.records.iter().find(|task| task.id == start.task)
                            {
                                let result = domain::outcome::summary(task);
                                self.workflow_send(&request.source, &request.rpc_id, result);
                            }
                        }
                        Err(error) => {
                            // A failed start acknowledgment is not authority to
                            // save speculative state over the actual writer.
                            if let Some(task) = self
                                .tasks
                                .records
                                .iter()
                                .find(|task| task.id == start.task)
                                .cloned()
                            {
                                self.task_recovery_reconcile(
                                    task.clone(),
                                    task,
                                    error.clone(),
                                    false,
                                );
                            }
                            self.workflow_send(
                                &request.source,
                                &request.rpc_id,
                                json!({"error":error}),
                            );
                        }
                    }
                }
            }
        }
        let waits = std::mem::take(&mut self.tasks.waits);
        for wait in waits {
            let Some(task) = self.tasks.records.iter().find(|t| t.id == wait.task) else {
                self.workflow_send(
                    &wait.source,
                    &wait.rpc,
                    json!({"error":"Workflow no longer available."}),
                );
                continue;
            };
            if !task.phase.active() {
                let result = domain::outcome::summary(task);
                self.workflow_send(&wait.source, &wait.rpc, result);
            } else if std::time::Instant::now() >= wait.deadline {
                self.workflow_send(&wait.source,&wait.rpc,json!({"workflow_id":wait.task,"timed_out":true,"verified":false,"outcome":"running","note":"Wait expired; workflow was not stopped."}));
            } else {
                self.tasks.waits.push(wait);
            }
        }
    }
}
