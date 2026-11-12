use super::*;
use domain::routing::{self, Decision};

impl Pui {
    /// Use the workflow creator's catalog even while another chat is visible.
    /// Never substitute the current chat when a retained owner is unavailable.
    pub(super) fn task_routing_models(&self, task: &Task) -> Option<&[crate::protocol::Model]> {
        if let Some(owner) = self.tasks.parents.get(&task.id) {
            return if *owner == self.agents.active {
                Some(&self.state.models)
            } else {
                self.agents
                    .parked
                    .get(owner)
                    .map(|chat| chat.state.models.as_slice())
            };
        }
        self.task_in_chat(task)
            .then_some(self.state.models.as_slice())
    }

    /// Must guard every model launch, including pinned/retained workflows.
    /// Checks are host command submission and do not launch a model.
    pub(super) fn task_validate_model_launch(
        &mut self,
        role: Role,
        selection: &routing::Selection,
        is_router: bool,
    ) -> bool {
        if role == Role::Checks {
            return true;
        }
        let result = self
            .tasks
            .live_task()
            .ok_or_else(|| "No live workflow.".to_string())
            .and_then(|task| {
                let models = self
                    .task_routing_models(task)
                    .ok_or("The workflow owner's model catalog is unavailable.")?;
                routing::validate_launch(task, models, selection, is_router)
            });
        if let Err(error) = result {
            self.tasks.block(error);
            false
        } else {
            true
        }
    }

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
        let Some(models) = self.task_routing_models(task) else {
            self.tasks.block("The workflow owner's model catalog is unavailable. No stage was launched; return to its owning chat before starting a new workflow.");
            return;
        };
        let mut pool = routing::candidates(models, &task.provider);
        pool.retain(|m| !m.family.exceptional());
        let router = match routing::router(&pool) {
            Ok(s) => s,
            Err(e) => {
                self.tasks.block(e);
                return;
            }
        };
        let prompt = routing::prompt(task, role, &pool);
        if !self.task_validate_model_launch(role, &router, true) {
            return;
        }
        self.task_launch_selected(role, router, Some(role), Some(prompt));
    }
    pub(super) fn task_route_finished(&mut self, role: Role, answer: &str) {
        let Some(task) = self.tasks.live_task() else {
            return;
        };
        let Some(models) = self.task_routing_models(task) else {
            self.tasks.block(
                "The workflow owner's model catalog is unavailable. No routed stage was launched.",
            );
            return;
        };
        let pool = routing::candidates(models, &task.provider);
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
        if !self.task_validate_model_launch(role, &selection, false) {
            return;
        }
        self.task_launch_selected(role, selection, None, None);
    }
}
