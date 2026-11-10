use super::*;
use domain::routing::{self, Decision, PendingRoute};

impl Pui {
    pub(super) fn task_budget_stop(&mut self, inflight: f64) -> bool {
        if !self
            .tasks
            .live_task()
            .is_some_and(|t| routing::budget_reached(t, inflight))
        {
            return false;
        }
        for p in &mut self.tasks.processes {
            let _ = p.client.send(protocol::command("clear_queue"));
            let _ = p.client.send(protocol::command("abort"));
            let _ = p.client.send(protocol::command("abort_bash"));
        }
        self.tasks.block("Reported task usage reached the approved stop threshold. No further stage was launched. In-flight or delayed provider usage may overshoot; this is not a hard billing cap.");
        true
    }
    pub(super) fn task_route(&mut self, role: Role) {
        let Some(task) = self.tasks.live_task() else {
            return;
        };
        let pool = routing::candidates(&self.state.models, &task.provider);
        let router = match routing::router(&pool) {
            Ok(s) => s,
            Err(e) => {
                self.tasks.block(e);
                return;
            }
        };
        let prompt = routing::prompt(task, role, &pool);
        self.task_launch_selected(role, router, Some(role), Some(prompt));
    }
    pub(super) fn task_route_finished(&mut self, role: Role, answer: &str) {
        let Some(task) = self.tasks.live_task() else {
            return;
        };
        let pool = routing::candidates(&self.state.models, &task.provider);
        let selection = match domain::json_result::<Decision>(answer)
            .and_then(|d| routing::validate(d, &pool, task, role))
        {
            Ok(s) => s,
            Err(e) => {
                self.tasks.block(e);
                return;
            }
        };
        self.tasks.retire();
        if selection.exceptional {
            let t = self.tasks.live_mut().unwrap();
            t.pending_route = Some(PendingRoute { role, selection });
            t.phase = Phase::Blocked;
            t.note="Router requested an exceptional model. Review the reason and explicitly approve or decline; no exceptional-model request has been sent.".into();
            self.tasks.save();
            self.tasks.live = None;
            return;
        }
        self.task_launch_selected(role, selection, None, None);
    }
    pub(super) fn task_approve_exceptional(&mut self) {
        if self.demo || self.tasks.live.is_some() {
            return;
        }
        let Some(t) = self.tasks.current() else {
            return;
        };
        if self.tasks.storage_failed {
            self.tasks.error =
                Some("Task storage failed; exceptional execution is disabled.".into());
            return;
        }
        let Some(pending) = t.pending_route.clone() else {
            return;
        };
        let pool = routing::candidates(&self.state.models, &t.provider);
        if !pool
            .iter()
            .any(|m| m.provider == pending.selection.provider && m.model == pending.selection.model)
        {
            self.tasks.error = Some(
                "Requested exceptional model is no longer available. Nothing was substituted."
                    .into(),
            );
            return;
        }
        self.tasks.live = Some(t.id.clone());
        let t = self.tasks.live_mut().unwrap();
        t.pending_route = None;
        t.phase = match pending.role {
            Role::Proposal => Phase::Proposing,
            Role::Builder => Phase::Building,
            Role::Verifier => Phase::Verifying,
            Role::Checks => Phase::Checking,
        };
        t.note = "Exceptional routing choice explicitly approved by you.".into();
        self.tasks.save();
        self.task_launch_selected(pending.role, pending.selection, None, None);
    }
}
