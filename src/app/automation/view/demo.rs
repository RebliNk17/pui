//! Fixture jobs for the design preview; never persisted or executed.
use super::*;
impl Pui {
    pub(in crate::app) fn demo_automation(&mut self) {
        let creator = agents::AgentRecord {
            id: "agent-1".into(),
            parent: None,
            name: "Release readiness".into(),
            cwd: PathBuf::from("/workspace/payments"),
            path: PathBuf::from("/example/release-readiness.jsonl"),
            access: "workspace".into(),
            permission: None,
            worktree: None,
        };
        self.automation.jobs = vec![
            Job {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Morning release review".into(),
                creator: creator.clone(),
                trigger: Trigger::Cron {
                    schedule: "0 9 * * 1-5".into(),
                },
                message: "Review open pull requests and summarize release blockers.".into(),
                enabled: true,
                completed: false,
                next_run: cron::Cron::parse("0 9 * * 1-5").unwrap().next(now()).unwrap(),
                last_run: Some(now() - 86400),
                evidence:None,
                previous_checks:Vec::new(),
                deliveries: 3,
                pending: None,
                dispatching: false,
                error: None,
            },
            Job {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Wait for CI to pass".into(),
                creator: agents::AgentRecord {
                    name: "Fix checkout tests".into(),
                    id: "agent-2".into(),
                    ..creator.clone()
                },
                trigger: Trigger::Monitor {
                    command:
                        "gh run view 12345 --json conclusion --jq '.conclusion' | grep -qx success"
                            .into(),
                    interval_seconds: 60,
                    timeout_seconds: 30,
                },
                message: "CI is green. Review the results before proceeding.".into(),
                enabled: true,
                completed: false,
                next_run: now() + 40,
                last_run: Some(now() - 20),
                evidence:None,
                previous_checks:Vec::new(),
                deliveries: 0,
                pending: None,
                dispatching: false,
                error: None,
            },
            Job {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Weekly dependency review".into(),
                creator: creator.clone(),
                trigger: Trigger::Cron {
                    schedule: "30 10 * * 1".into(),
                },
                message: "Inspect dependency advisories and propose updates. Do not modify files."
                    .into(),
                enabled: false,
                completed: false,
                next_run: cron::Cron::parse("30 10 * * 1").unwrap().next(now()).unwrap(),
                last_run: None,
                evidence:None,
                previous_checks:Vec::new(),
                deliveries: 0,
                pending: None,
                dispatching: false,
                error: None,
            },
            Job {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Harbor PR 71 validation completion".into(),
                creator: agents::AgentRecord {
                    name: "Harbor release checks".into(),
                    id: "agent-3".into(),
                    ..creator
                },
                trigger: Trigger::Monitor {
                    command: "gh pr checks 71 --repo example/harbor --required".into(),
                    interval_seconds: 60,
                    timeout_seconds: 20,
                },
                message: "Recheck the exact PR head, all CI/security results and merge rules. Report blockers; do not bypass rules or deploy manually.".into(),
                enabled: false,
                completed: true,
                next_run: now(),
                last_run: Some(now() - 1800),
                evidence:None,
                previous_checks:Vec::new(),
                deliveries: 1,
                pending: None,
                dispatching: false,
                error: None,
            },
        ];
    }
}
