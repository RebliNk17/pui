//! A lead-authored plan reviewed before implementation, not a script or permission grant.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPlan {
    pub steps: Vec<String>,
    pub checks: Contract,
}
impl ExecutionPlan {
    pub fn validate(&self) -> Result<(), String> {
        if self.steps.is_empty()
            || self.steps.len() > 12
            || self
                .steps
                .iter()
                .any(|step| step.trim().is_empty() || step.len() > 2000 || step.contains('\0'))
            || self.steps.iter().map(String::len).sum::<usize>() > 12000
        {
            return Err(
                "Supply 1–12 concrete implementation steps (2,000 bytes each; 12,000 total)."
                    .into(),
            );
        }
        self.checks.validate()
    }
    /// Planning input is untrusted. Neither an approved flag nor extra fields can
    /// start work; the owning host must still present the full plan for consent.
    pub fn from_request(value: &serde_json::Value) -> Result<Option<Self>, String> {
        let Some(value) = value.get("plan") else {
            return Ok(None);
        };
        let plan: Self = serde_json::from_value(value.clone())
            .map_err(|_| "Workflow plan must contain only bounded steps and checks.criteria.")?;
        plan.validate()?;
        Ok(Some(plan))
    }
    pub fn description(&self) -> String {
        let steps = self
            .steps
            .iter()
            .enumerate()
            .map(|(i, step)| format!("{}. {step}", i + 1))
            .collect::<Vec<_>>()
            .join("\n");
        let checks = self
            .checks
            .criteria
            .iter()
            .map(|criterion| {
                format!(
                    "{} · {}\n{}",
                    criterion.id,
                    criterion.description,
                    if criterion.command.trim().is_empty() {
                        "Independent inspection (not an executed check)"
                    } else {
                        &criterion.command
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        format!("Implementation plan\n{steps}\n\nVerification\n{checks}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plan_is_explicit_bounded_and_never_an_approval_flag() {
        assert!(
            ExecutionPlan::from_request(&serde_json::json!({"approved":true}))
                .unwrap()
                .is_none()
        );
        let valid = serde_json::json!({"plan":{"steps":["Inspect focus handling", "Add keyboard regression"], "checks":{"criteria":[{"id":"C1","description":"Keyboard navigation works","command":"cargo test keyboard"}]}}});
        let plan = ExecutionPlan::from_request(&valid).unwrap().unwrap();
        assert!(plan.description().contains("cargo test keyboard"));
        for value in [
            serde_json::json!(null),
            serde_json::json!({"steps":[],"checks":{"criteria":[]}}),
            serde_json::json!({"steps":["x"],"checks":{"criteria":[]},"approved":true}),
        ] {
            assert!(ExecutionPlan::from_request(&serde_json::json!({"plan":value})).is_err());
        }
        let mut invalid = plan;
        invalid.steps = vec!["x".repeat(2001)];
        assert!(invalid.validate().is_err());
    }
}
