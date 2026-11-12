//! One host-enforced, objective-bound approval. No workflow exists before consent.
use super::*;

#[cfg(test)]
#[path = "consent_tests.rs"]
mod tests;

pub(super) struct Request {
    source: String,
    rpc_id: String,
    control_id: String,
    objective: String,
    constraints: String,
    cwd: PathBuf,
    provider: String,
    model: String,
    approval: domain::routing::Approval,
    task: Option<String>,
}

impl Pui {
    pub(in crate::app) fn workflow_control(&mut self, record: &Value) -> bool {
        if record["type"] != "extension_ui_request" {
            return false;
        }
        if record["method"] == "setStatus" && record["statusKey"] == "pui-workflow" {
            if let Ok(value) = serde_json::from_str::<Value>(protocol::string(record, "statusText"))
            {
                let closed = protocol::string(&value, "closed");
                let source = self.agents.active.clone();
                let ids: Vec<_> = self
                    .tasks
                    .requests
                    .iter()
                    .filter(|(_, r)| r.source == source && r.control_id == closed)
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in ids {
                    if let Some(request) = self.tasks.requests.remove(&id) {
                        self.state.dialogs.retain(|d| d["id"] != request.rpc_id);
                        if request.task.as_ref() == self.tasks.live.as_ref()
                            && request.task.is_some()
                        {
                            self.tasks.stop();
                        }
                    }
                }
            }
            return true;
        }
        if record["method"] != "input" || record["title"] != "Pui agent control" {
            return false;
        }
        let Ok(value) = serde_json::from_str::<Value>(protocol::string(record, "placeholder"))
        else {
            return false;
        };
        if value["action"] != "workflow_request" {
            return false;
        }
        let rpc_id = protocol::string(record, "id").to_owned();
        let source = self.ensure_agent();
        let objective = protocol::string(&value, "objective").trim();
        let constraints = protocol::string(&value, "constraints");
        let reason = protocol::string(&value, "reason").trim();
        let error = if self.demo || !self.ultracode_ready || self.is_mirror() {
            Some("A ready owning Pui chat is required.")
        } else if matches!(
            self.active_permission(),
            settings::PermissionMode::Plan | settings::PermissionMode::DontAsk
        ) {
            Some("Current permission mode does not allow offering workflows.")
        } else if self
            .agents
            .nodes
            .get(&source)
            .is_none_or(|n| n.access == "read-only")
        {
            Some("Read-only workers cannot start workflows.")
        } else if self.tasks.loading_history()
            || self.tasks.storage_failed
            || self.deletion.in_progress()
        {
            Some("Workflow storage is not ready.")
        } else if self.tasks.live.is_some() || !self.tasks.requests.is_empty() {
            Some("Another workflow or workflow question is already active.")
        } else if objective.is_empty()
            || objective.len() > 16000
            || constraints.len() > 12000
            || reason.is_empty()
            || reason.len() > 240
            || protocol::string(&value, "id").is_empty()
        {
            Some("Supply a bounded objective, constraints and a very short reason.")
        } else if self.state.provider.is_empty() || self.state.model.is_empty() {
            Some("A confirmed chat model is required.")
        } else {
            None
        };
        if let Some(error) = error {
            self.send(json!({"type":"extension_ui_response","id":rpc_id,"value":json!({"error":error}).to_string()}));
            return true;
        }
        // Reject unsupported routing before asking for consent or creating a
        // workflow, rather than spending on a task that cannot start.
        let approval = match domain::routing::Approval::from_request(
            &value,
            &self.state.models,
            &self.state.provider,
            &self.state.model,
            &self.state.thinking,
        ) {
            Ok(approval) => approval,
            Err(error) => {
                self.send(json!({"type":"extension_ui_response","id":rpc_id,"value":json!({"error":format!("Workflow unavailable for this chat: {error} Use regular development or explicitly choose a supported policy; no workflow was created.")}).to_string()}));
                return true;
            }
        };
        let policy_description = approval.description();
        self.tasks.requests.insert(
            rpc_id.clone(),
            Request {
                source,
                rpc_id: rpc_id.clone(),
                control_id: protocol::string(&value, "id").into(),
                objective: objective.into(),
                constraints: constraints.into(),
                cwd: self.prefs.cwd.clone(),
                provider: self.state.provider.clone(),
                model: self.state.model.clone(),
                approval,
                task: None,
            },
        );
        self.state.dialogs.push_back(json!({
            "type":"extension_ui_request", "method":"select", "id":rpc_id,
            "title":"Use a workflow?", "message":format!("{reason}\n\n{objective}\n\nAgent-managed checks, implementation and independent review (up to two repairs). {policy_description}\n\nNormal permissions apply. Read-only prerequisite inspection is bounded, not a guarantee of environment health. Missing ignored dependencies may block; automatic bootstrap/install and command-replay recovery are unsupported. Isolated Git worktree with a snapshot of current files (including uncommitted changes; ignored files excluded), or non-Git in place. Original checkout is left untouched. No commit, merge or deployment."),
            "options":["Use workflow", "Regular development"]
        }));
        true
    }

    /// Called only on an actual UI response, including responses routed from mirrors.
    pub(in crate::app) fn workflow_answer(&mut self, response: &Value) -> bool {
        if response["type"] != "extension_ui_response" {
            return false;
        }
        let id = protocol::string(response, "id");
        let Some(request) = self.tasks.requests.get(id) else {
            return false;
        };
        if request.source != self.agents.active || request.task.is_some() {
            return false;
        }
        let context_changed = request.cwd != self.prefs.cwd
            || request.provider != self.state.provider
            || request.model != self.state.model
            || request
                .approval
                .revalidate(
                    &self.state.models,
                    &self.state.provider,
                    &self.state.model,
                    &self.state.thinking,
                )
                .is_err();
        let id = id.to_owned();
        // Only a still-visible host question may authorize creation. Never trust
        // an agent-supplied approved=true flag or a stale RPC response.
        if !self.state.dialogs.iter().any(|d| d["id"] == id) {
            return false;
        }
        let allow = response["value"] == "Use workflow" && response["cancelled"] != true;
        if !allow {
            let request = self.tasks.requests.remove(&id).unwrap();
            let result = if response["value"] == "Regular development"
                && response["cancelled"] != true
            {
                json!({"approved":false,"regular_development":true,"note":"Continue with regular development. No workflow was created."})
            } else {
                json!({"approved":false,"cancelled":true,"note":"Nothing was approved. Wait for the user's next instruction."})
            };
            self.workflow_reply(&request, result);
            return true;
        }
        if context_changed
            || self.tasks.live.is_some()
            || self.tasks.storage_failed
            || self.deletion.in_progress()
            || matches!(
                self.active_permission(),
                settings::PermissionMode::Plan | settings::PermissionMode::DontAsk
            )
        {
            let request = self.tasks.requests.remove(&id).unwrap();
            self.workflow_reply(
                &request,
                json!({"error":"Workflow can no longer start. Nothing was launched."}),
            );
            return true;
        }
        self.create_workflow();
        let request = self.tasks.requests.get_mut(&id).unwrap();
        let objective = request.objective.clone();
        let constraints = request.constraints.clone();
        let approval = request.approval.clone();
        request.task = self.tasks.selected.clone();
        let Some(task) = self.tasks.current_mut() else {
            return true;
        };
        task.objective = objective;
        task.constraints = constraints;
        task.initial_approved = true;
        if let Err(error) = approval.bind(task) {
            task.phase = Phase::Blocked;
            task.note = error;
            self.tasks.save();
            return true;
        }
        self.tasks.save();
        self.task_propose();
        if let Some(error) = self.tasks.error.clone() {
            if let Some(t) = self.tasks.current_mut() {
                t.phase = Phase::Blocked;
                t.note = error;
            }
            self.tasks.save();
        }
        true
    }

    pub(in crate::app) fn cancel_workflows_for(&mut self, source: &str) {
        let ids: Vec<_> = self
            .tasks
            .requests
            .iter()
            .filter(|(_, r)| r.source == source)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            if let Some(request) = self.tasks.requests.remove(&id) {
                if request.task.is_some() && request.task.as_ref() == self.tasks.live.as_ref() {
                    self.tasks.stop();
                }
            }
        }
    }

    fn workflow_reply(&mut self, request: &Request, result: Value) {
        let response =
            json!({"type":"extension_ui_response","id":request.rpc_id,"value":result.to_string()});
        if request.source == self.agents.active {
            if let Some(client) = self.client.as_mut() {
                let _ = client.send(response);
            }
        } else if let Some(client) = self
            .agents
            .parked
            .get_mut(&request.source)
            .and_then(|c| c.client.as_mut())
        {
            let _ = client.send(response);
        }
    }

    pub(super) fn finish_workflow_requests(&mut self) {
        let finished: Vec<_> = self.tasks.requests.iter().filter_map(|(id, r)| {
            let task = self.tasks.records.iter().find(|t| Some(&t.id) == r.task.as_ref())?;
            (!task.phase.active()).then(|| (id.clone(), json!({
                "workflow_id":task.id, "approved":true, "phase":task.phase.label(), "note":task.note,
                "workspace":task.workspace, "isolated":task.isolated, "reported_cost":task.cost(),
                "checks":task.approved, "attempts":task.attempts.iter().map(|a| json!({
                    "number":a.number, "fingerprint":a.fingerprint, "invalidated":a.invalidated,
                    "checks":a.checks.iter().map(|c| json!({"criterion":c.criterion,"command":c.command,"exit_code":c.exit_code,"cancelled":c.cancelled,"output":c.output.chars().take(2000).collect::<String>(),"truncated":c.truncated || c.output.chars().count()>2000})).collect::<Vec<_>>(),
                    "review":a.review, "note":a.note
                })).collect::<Vec<_>>(),
                "instruction":"Report the evidence and any blockers. Results do not authorize commits, merges or deployment. Do not automatically retry a stopped or blocked workflow."
            })))
        }).collect();
        for (id, result) in finished {
            if let Some(request) = self.tasks.requests.remove(&id) {
                self.workflow_reply(&request, result);
            }
        }
    }
}
