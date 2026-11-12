//! Regressions for ownership boundaries, not screenshot-only success.
use super::*;

#[test]
fn another_workflows_page_cannot_render_or_answer_live_permission() {
    let mut app = app();
    live(&mut app, Phase::Building);
    process(&mut app, Role::Builder);
    let owner = app.tasks.selected.clone().unwrap();
    let (client, _, requests) = rpc::Client::stub_with_records();
    app.tasks.processes[0].client = client;
    app.tasks.processes[0].state.dialogs.push_back(json!({
        "id":"permission", "method":"confirm", "title":"LIVE WORKFLOW WRITE", "message":"Write in live workspace?"
    }));
    let old = Task::new(
        "/old/workspace".into(),
        "fixture-provider".into(),
        "fixture-model".into(),
        "off".into(),
    );
    app.tasks.selected = Some(old.id.clone());
    app.tasks.records.push(old);
    app.tasks.open = true;
    let ctx = app.ctx.clone();
    for _ in 0..3 {
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.task_dialog(ui));
        });
        let mut labels = vec![];
        for shape in output.shapes {
            texts(&shape.shape, &mut labels);
        }
        assert!(
            !labels
                .iter()
                .any(|s| s.contains("LIVE WORKFLOW WRITE") || s == "Confirm")
        );
    }
    app.task_answer("run", "permission", json!({"confirmed":true}));
    assert!(matches!(
        requests.recv_timeout(Duration::from_millis(30)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert_eq!(app.tasks.processes[0].state.dialogs.len(), 1);
    app.tasks.selected = Some(owner);
    app.task_answer("run", "permission", json!({"confirmed":true}));
    assert_eq!(
        requests.recv_timeout(Duration::from_secs(2)).unwrap()["id"],
        "permission"
    );
    assert!(app.tasks.processes[0].state.dialogs.is_empty());
}

#[test]
fn review_permission_only_opens_details_and_foreign_workflow_still_cannot_authorize() {
    let mut app = app();
    live(&mut app, Phase::Checking);
    process(&mut app, Role::Checks);
    app.tasks.open = true;
    app.tasks.processes[0].state.dialogs.push_back(json!({"id":"permission", "method":"confirm", "title":"Exact approved command", "message":"cargo test"}));
    let (client, _, requests) = rpc::Client::stub_with_records();
    app.tasks.processes[0].client = client;
    let ctx = app.ctx.clone();
    let render = |app: &mut Pui, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1000.0, 1200.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.task_content(ui));
            },
        )
    };
    let mut pos = None;
    for _ in 0..3 {
        let output = render(&mut app, vec![]);
        for shape in &output.shapes {
            if let egui::Shape::Text(text) = &shape.shape {
                if text.galley.job.text == "Review permission" {
                    pos = Some(text.pos + text.galley.size() / 2.0);
                }
            }
        }
    }
    let pos = pos.expect("review permission action");
    for pressed in [true, false] {
        render(
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    let output = render(&mut app, vec![]);
    let mut text = vec![];
    for shape in output.shapes {
        texts(&shape.shape, &mut text);
    }
    assert!(text.iter().any(|s| s == "Confirm"));
    assert!(matches!(
        requests.recv_timeout(Duration::from_millis(30)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert_eq!(app.tasks.processes[0].state.dialogs.len(), 1);
}

#[test]
fn workflow_routing_keeps_owner_catalog_when_another_chat_is_visible() {
    let mut app = app();
    live(&mut app, Phase::Proposing);
    app.tasks.live_mut().unwrap().dynamic_routing = true;
    app.state.models = routing_catalog();
    let owner = app.agents.active.clone();
    app.demo_agent_tree();
    let child = app.agents.nodes[&owner].children[0].clone();
    app.view_agent(&child);
    app.state.models.clear();
    app.task_launch(Role::Proposal);
    assert_eq!(app.tasks.processes[0].routing_for, Some(Role::Proposal));
    app.task_route_finished(Role::Proposal, r#"{"model":"gpt-6.1-sol","thinking":"high","difficulty":"standard","reason":"Technical native layout debugging requires a standard model with independent executable checks.","exceptional_reason":null}"#);
    assert_eq!(app.tasks.processes[0].routing_for, None);
    assert_eq!(
        app.tasks
            .live_task()
            .unwrap()
            .runs
            .last()
            .unwrap()
            .selection
            .as_ref()
            .unwrap()
            .model,
        "gpt-6.1-sol"
    );
}

#[test]
fn missing_owner_never_borrows_an_unrelated_chats_catalog() {
    let mut app = app();
    live(&mut app, Phase::Proposing);
    app.tasks.live_mut().unwrap().dynamic_routing = true;
    app.state.models = routing_catalog();
    app.agents.active = "unrelated-chat".into();
    app.task_launch(Role::Proposal);
    assert!(app.tasks.processes.is_empty());
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Blocked);
    assert!(
        app.tasks
            .current()
            .unwrap()
            .note
            .contains("owner's model catalog")
    );
}

#[test]
fn revalidation_claim_conflict_does_not_change_evidence_or_poison_storage() {
    let mut app = app();
    live(&mut app, Phase::Ready);
    passed_attempt(app.tasks.live_mut().unwrap());
    app.tasks.live = None;
    let before = serde_json::to_value(app.tasks.current()).unwrap();
    let (store, commands) = mpsc::channel();
    app.tasks.store = Some(store);
    app.task_fingerprint(FingerprintStep::Revalidate);
    assert!(app.tasks.live.is_none());
    assert!(app.tasks.job.is_none());
    let StoreCommand::Claim { reply, .. } = commands.try_recv().unwrap() else {
        panic!("claim must precede source work");
    };
    reply
        .send(Err(domain::store::LOCK_CONFLICT.into()))
        .unwrap();
    app.poll_tasks();
    assert!(!app.tasks.storage_failed);
    assert!(app.tasks.live.is_none());
    assert!(app.tasks.job.is_none());
    assert_eq!(serde_json::to_value(app.tasks.current()).unwrap(), before);
    assert!(
        app.state
            .notice
            .as_ref()
            .unwrap()
            .contains("Other workflows are still available")
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn real_writer_lock_conflict_is_local_and_next_task_can_still_be_saved() {
    let app = app();
    let home = std::env::temp_dir().join(format!("pui-workflow-claim-{}", uuid::Uuid::new_v4()));
    let locked = Task::new(home.clone(), "p".into(), "m".into(), "off".into());
    let _owner = domain::store::lock(&home, &locked.id).unwrap();
    let next = Task::new(home.clone(), "p".into(), "m".into(), "off".into());
    let (tx, rx) = mpsc::channel();
    let (errors, error_rx) = mpsc::channel();
    let (reply, claimed) = mpsc::channel();
    tx.send(StoreCommand::Claim {
        id: locked.id.clone(),
        reply,
    })
    .unwrap();
    tx.send(StoreCommand::Save(Box::new(next.clone()))).unwrap();
    drop(tx);
    run_store(&home, rx, errors, &app.ctx);
    assert_eq!(
        claimed.recv().unwrap(),
        Err(domain::store::LOCK_CONFLICT.into())
    );
    assert!(error_rx.try_recv().is_err());
    assert!(home.join(format!("{}.json", next.id)).is_file());
    assert!(!home.join(format!("{}.json", locked.id)).exists());
}
