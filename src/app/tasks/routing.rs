use super::*;
use domain::routing::{self, Decision};

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
        self.tasks.block("Reported workflow usage reached the stop threshold. No further stage was launched. In-flight or delayed provider usage may overshoot; this is not a hard billing cap.");
        true
    }
    pub(super) fn task_route(&mut self, role: Role) {
        let Some(task) = self.tasks.live_task() else {
            return;
        };
        // A single workflow approval never authorizes exceptional-price models.
        let mut pool = routing::candidates(&self.state.models, &task.provider);
        pool.retain(|m| !m.family.exceptional());
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
            self.tasks.block("Exceptional-price models are outside this workflow's approval. No exceptional model was launched; continue regular development or discuss a different approach.");
            return;
        }
        self.task_launch_selected(role, selection, None, None);
    }
}
