use super::*;
#[path = "consent_policy_tests.rs"]
mod policy_tests;

fn app() -> Pui {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(egui::Context::default()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.ultracode_ready = true;
    app.state.provider = "fixture-provider".into();
    app.state.model = "claude-haiku-5-5".into();
    app.state.thinking = "high".into();
    app.state.models = vec![crate::protocol::Model {
        id: "claude-haiku-5-5".into(),
        name: "Fixture Haiku".into(),
        provider: "fixture-provider".into(),
        context_window: 200000,
        input_cost: 0.1,
        output_cost: 0.5,
    }];
    app.client = Some(rpc::Client::stub());
    app
}
#[test]
fn unsupported_catalog_is_rejected_before_consent_or_task_creation() {
    let mut app = app();
    let (client, _, replies) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.models.clear();
    app.control_request(&request("unsupported"));
    assert!(app.state.dialogs.is_empty());
    assert!(app.tasks.requests.is_empty());
    assert!(app.tasks.records.is_empty());
    assert!(app.tasks.job.is_none());
    let reply = replies.recv_timeout(Duration::from_secs(2)).unwrap();
    let result: Value = serde_json::from_str(reply["value"].as_str().unwrap()).unwrap();
    assert!(
        result["error"]
            .as_str()
            .unwrap()
            .contains("no workflow was created")
    );
}

#[test]
fn catalog_change_after_question_prevents_workflow_creation() {
    let mut app = app();
    app.control_request(&request("ask"));
    app.state.models.clear();
    assert!(app.workflow_answer(
        &json!({"type":"extension_ui_response","id":"ask","value":"Use workflow"})
    ));
    assert!(app.tasks.records.is_empty());
    assert!(app.tasks.job.is_none());
    assert!(app.tasks.requests.is_empty());
}

fn request(id: &str) -> Value {
    json!({"type":"extension_ui_request","method":"input","title":"Pui agent control","id":id,
        "placeholder":json!({"action":"workflow_request","id":id,"objective":"Preserve keyboard navigation","reason":"Separate checks and review help catch focus regressions.","approved":true}).to_string()})
}
#[test]
fn agent_cannot_skip_initial_consent_and_decline_creates_nothing() {
    let mut app = app();
    assert!(app.control_request(&request("ask")));
    assert!(app.tasks.records.is_empty());
    assert!(app.tasks.job.is_none());
    assert!(app.tasks.processes.is_empty());
    assert_eq!(app.state.dialogs[0]["title"], "Use a workflow?");
    assert!(app.workflow_answer(
        &json!({"type":"extension_ui_response","id":"ask","value":"Regular development"})
    ));
    assert!(app.tasks.records.is_empty());
    assert!(app.tasks.requests.is_empty());
}
#[test]
fn stale_cancelled_and_wrong_session_approvals_cannot_create_workflows() {
    let mut app = app();
    app.control_request(&request("ask"));
    assert!(!app.workflow_answer(
        &json!({"type":"extension_ui_response","id":"stale","value":"Use workflow"})
    ));
    let source = app.agents.active.clone();
    app.agents.active = "different-chat".into();
    assert!(!app.workflow_answer(
        &json!({"type":"extension_ui_response","id":"ask","value":"Use workflow"})
    ));
    app.agents.active = source;
    assert!(app.workflow_answer(
        &json!({"type":"extension_ui_response","id":"ask","value":"Use workflow","cancelled":true})
    ));
    assert!(app.tasks.records.is_empty());
    assert!(app.tasks.requests.is_empty());
}
#[test]
fn one_approval_starts_agent_managed_proposal_without_repeating_the_question() {
    let mut app = app();
    app.control_request(&request("ask"));
    assert!(app.workflow_answer(
        &json!({"type":"extension_ui_response","id":"ask","value":"Use workflow"})
    ));
    let t = app.tasks.current().unwrap();
    assert!(t.initial_approved);
    assert_eq!(t.objective, "Preserve keyboard navigation");
    assert_eq!(t.phase, Phase::Proposing);
    assert!(app.tasks.job.is_some());
    app.tasks.stop();
    app.finish_workflow_requests();
    assert!(app.tasks.requests.is_empty());
}
#[test]
fn abort_status_closes_only_matching_question_and_stops_matching_workflow() {
    let mut app = app();
    app.control_request(&request("ask"));
    let close = |id| json!({"type":"extension_ui_request","method":"setStatus","statusKey":"pui-workflow","statusText":json!({"closed":id}).to_string()});
    app.control_request(&close("other"));
    assert_eq!(app.tasks.requests.len(), 1);
    app.workflow_answer(&json!({"type":"extension_ui_response","id":"ask","value":"Use workflow"}));
    app.control_request(&close("ask"));
    assert!(app.tasks.requests.is_empty());
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Cancelled);
    assert!(app.tasks.live.is_none());
}
#[test]
fn consent_never_overrides_plan_or_dont_ask_modes() {
    for mode in [
        settings::PermissionMode::Plan,
        settings::PermissionMode::DontAsk,
    ] {
        let mut app = app();
        app.prefs.settings.permission = mode;
        app.control_request(&request("ask"));
        assert!(app.state.dialogs.is_empty());
        assert!(app.tasks.requests.is_empty());
        assert!(app.tasks.records.is_empty());
    }
}
#[test]
fn initial_question_is_inline_and_readable_in_both_themes_without_setup_actions() {
    fn text(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| text(s, out)),
            _ => {}
        }
    }
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [360.0, 760.0] {
            let mut app = app();
            mode.apply(&app.ctx);
            app.control_request(&request("ask"));
            let ctx = app.ctx.clone();
            let mut labels = Vec::new();
            for _ in 0..3 {
                labels.clear();
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 850.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| app.agent_question(ui));
                    },
                );
                for shape in output.shapes {
                    text(&shape.shape, &mut labels);
                }
            }
            for expected in ["Use a workflow?", "Use workflow", "Regular development"] {
                assert!(
                    labels.iter().any(|s| s == expected),
                    "missing {expected}: {labels:?}"
                );
            }
            assert!(labels.iter().any(|s| s.contains("Separate checks and review help catch focus regressions.")));
            assert!(app.tasks.records.is_empty());
            assert!(app.tasks.processes.is_empty());
        }
    }
}
#[test]
fn initial_approval_allows_dirty_source_for_an_isolated_snapshot() {
    let mut app = app();
    app.control_request(&request("ask"));
    app.workflow_answer(&json!({"type":"extension_ui_response","id":"ask","value":"Use workflow"}));
    app.tasks.job = None;
    // Pin the fixture model: routing policy has its own coverage and needs a catalog.
    app.tasks.current_mut().unwrap().dynamic_routing = false;
    app.task_job_result(Ok(JobResult::Inspected(domain::workspace::Inspection {
        root: app.prefs.cwd.clone(),
        git: true,
        dirty: true,
        head: "head".into(),
    })));
    let job = app.tasks.job.take().expect("read-only prerequisite job");
    app.task_job_result(job.rx.recv_timeout(Duration::from_secs(10)).unwrap());
    let task = app.tasks.current().unwrap();
    assert_eq!(task.phase, Phase::Proposing);
    assert!(task.dirty_source);
    assert_eq!(task.base_head, "head");
    assert_eq!(app.tasks.processes.len(), 1);
    app.tasks.stop();
    app.finish_workflow_requests();
    assert!(app.tasks.requests.is_empty());
}
