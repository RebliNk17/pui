//! Pure recovery guidance. No filesystem reads, provider calls or replay authority.
use super::*;

pub(in crate::app) struct Plan {
    pub summary: String,
    pub detail: String,
}
impl Pui {
    pub(in crate::app) fn task_recovery_plan(&self, id: &str) -> Plan {
        let Some(task) = self.tasks.records.iter().find(|task| task.id == id) else {
            return Plan {
                summary: "Workflow unavailable".into(),
                detail: "Nothing can be recovered from an unknown workflow.".into(),
            };
        };
        if task.phase.active() {
            return Plan { summary: "Workflow is still active".into(), detail: "Stop it before inspecting recovery options. Recovery never resumes an interrupted builder or replays an uncertain command.".into() };
        }
        let candidate =
            task.workspace.is_some() && task.latest().is_some_and(|a| a.fingerprint.is_some());
        let summary = if candidate {
            "Retained candidate · inspect or revalidate evidence"
        } else {
            "No captured candidate to revalidate"
        };
        let remaining = task
            .latest()
            .map(|a| 2_u8.saturating_sub(a.number))
            .unwrap_or(2);
        Plan {
            summary: summary.into(),
            detail: format!(
                "{} Revalidation only compares the retained source with its captured fingerprint. It does not rerun checks, clear stale evidence, install dependencies, resume implementation, or replay interrupted shell/tool effects. Deterministic check reruns and automatic environment setup are not supported recovery capabilities. {} repair round(s) remained in this workflow; recovery does not reset the two-repair limit or erase earlier evidence. To continue implementation or run checks after setup/changes, request a separate new workflow from the retained workspace with fresh objective-bound consent and normal command permissions. No new workflow is created by viewing this plan.",
                if candidate {
                    "Existing checks and reviews remain attached to this workflow and candidate."
                } else {
                    "Files/sessions may be retained, but no candidate fingerprint is available."
                },
                remaining
            ),
        }
    }
}
