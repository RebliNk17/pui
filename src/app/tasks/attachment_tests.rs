use super::*;
use std::fs;

fn app() -> Pui {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(egui::Context::default()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.tasks = Tasks::default();
    app.state = State::default();
    app.state.session_file = Some("/fixture/new-conversation.jsonl".into());
    app.ensure_agent();
    app
}
fn fixture(app: &mut Pui, home: &std::path::Path) -> Task {
    let mut task = Task::new(
        home.into(),
        "provider".into(),
        "model".into(),
        "high".into(),
    );
    task.phase = Phase::Blocked;
    task.workspace = Some(home.into());
    task.parent_session = Some("/fixture/old-conversation.jsonl".into());
    task.objective = "Retain every field".into();
    task.approved = Some(Contract {
        criteria: vec![domain::Criterion {
            id: "C1".into(),
            description: "Retained evidence".into(),
            command: "never execute me".into(),
        }],
    });
    let mut attempt = Attempt::new(0);
    attempt.fingerprint = Some("captured".into());
    attempt.note = "Original evidence".into();
    task.attempts.push(attempt);
    task.runs.push(Run {
        id: "retained-run".into(),
        role: Role::Checks,
        attempt: 0,
        status: "Blocked".into(),
        session: None,
        summary: "retained".into(),
        cost: 1.25,
        permission_cost: 0.5,
        selection: None,
        routing_for: None,
    });
    domain::store::save(home, &task).unwrap();
    app.tasks.records = vec![task.clone()];
    app.tasks.selected = Some(task.id.clone());
    task
}
fn worker(app: &mut Pui, home: PathBuf) {
    let (store, commands) = mpsc::channel();
    let (errors, error_rx) = mpsc::channel();
    let ctx = app.ctx.clone();
    app.tasks.store_worker = Some(std::thread::spawn(move || {
        run_store(&home, commands, errors, &ctx)
    }));
    app.tasks.store = Some(store);
    app.tasks.errors = Some(error_rx);
}
fn wait_attachment(app: &mut Pui) {
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    while app.tasks.attachment.pending.is_some() && std::time::Instant::now() < deadline {
        app.poll_task_attachment();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.tasks.attachment.pending.is_none());
}
fn request(app: &Pui, task: Task) -> Request {
    Request {
        expected: task,
        parent: PathBuf::from(app.state.session_file.as_ref().unwrap()),
        agent: app.agents.active.clone(),
    }
}
#[test]
fn attachment_acknowledges_before_projection_and_survives_restart_without_evidence_edits() {
    let home = std::env::temp_dir().join(format!("pui-attach-{}", uuid::Uuid::new_v4()));
    let mut app = app();
    let task = fixture(&mut app, &home);
    let (store, commands) = mpsc::channel();
    app.tasks.store = Some(store);
    app.attach_retained(request(&app, task.clone())).unwrap();
    assert_eq!(app.tasks.records[0].parent_session, task.parent_session);
    assert!(!app.task_in_chat(&app.tasks.records[0]));
    let StoreCommand::Attach {
        expected,
        parent,
        reply,
    } = commands.recv().unwrap()
    else {
        panic!()
    };
    let _guard = domain::store::lock(&home, &task.id).unwrap();
    reply
        .send(domain::store::attach(&home, &expected, &parent))
        .unwrap();
    app.poll_task_attachment();
    assert!(app.task_in_chat(&app.tasks.records[0]));
    assert_eq!(app.chat_task_cards().len(), 1);
    let mut actual = serde_json::to_value(&app.tasks.records[0]).unwrap();
    let mut before = serde_json::to_value(&task).unwrap();
    for field in ["parent_session", "updated"] {
        actual.as_object_mut().unwrap().remove(field);
        before.as_object_mut().unwrap().remove(field);
    }
    assert_eq!(actual, before);
    let restarted = domain::store::load(&home).unwrap();
    assert_eq!(restarted[0].parent_session, Some(parent));
    app.state.session_file = Some("/fixture/old-conversation.jsonl".into());
    assert!(app.chat_task_cards().is_empty());
    assert!(app.task_for_chat().is_none());
}
#[test]
fn attachment_conflicts_are_local_and_preserve_original_relationship() {
    for stale in [false, true] {
        let home =
            std::env::temp_dir().join(format!("pui-attach-conflict-{}", uuid::Uuid::new_v4()));
        let mut app = app();
        let task = fixture(&mut app, &home);
        let before = serde_json::to_value(&task).unwrap();
        let foreign = if stale {
            let mut newer = task.clone();
            newer.note = "foreign update within the same second".into();
            domain::store::save(&home, &newer).unwrap();
            None
        } else {
            Some(domain::store::lock(&home, &task.id).unwrap())
        };
        worker(&mut app, home.clone());
        app.attach_retained(request(&app, task.clone())).unwrap();
        wait_attachment(&mut app);
        assert_eq!(serde_json::to_value(&app.tasks.records[0]).unwrap(), before);
        assert!(!app.tasks.storage_failed);
        assert!(
            app.state
                .notice
                .as_deref()
                .unwrap()
                .contains("Could not attach")
        );
        let loaded = domain::store::load(&home).unwrap();
        assert_eq!(loaded[0].parent_session, task.parent_session);
        if stale {
            assert_eq!(loaded[0].note, "foreign update within the same second");
            // Failed optimistic transaction must not retain the writer lock.
            let _guard = domain::store::lock(&home, &task.id).unwrap();
        }
        drop(foreign);
    }
}
#[test]
fn raw_active_foreign_workflow_cannot_attach_even_if_recovered_as_interrupted() {
    let home = std::env::temp_dir().join(format!("pui-attach-active-{}", uuid::Uuid::new_v4()));
    let mut app = app();
    let mut task = fixture(&mut app, &home);
    task.phase = Phase::Checking;
    domain::store::save(&home, &task).unwrap();
    let recovered = domain::store::load(&home).unwrap().remove(0);
    let _lock = domain::store::lock(&home, &task.id).unwrap();
    assert!(
        domain::store::attach(&home, &recovered, std::path::Path::new("/fixture/new")).is_err()
    );
    assert_eq!(
        domain::store::load(&home).unwrap()[0].parent_session,
        task.parent_session
    );
}
#[test]
fn manual_revalidation_claims_current_record_before_any_fingerprint_or_commands() {
    let home = std::env::temp_dir().join(format!("pui-manual-revalidate-{}", uuid::Uuid::new_v4()));
    let mut app = app();
    let task = fixture(&mut app, &home);
    let (store, commands) = mpsc::channel();
    app.tasks.store = Some(store);
    app.task_revalidate_retained(&task.id).unwrap();
    assert!(app.tasks.job.is_none());
    assert!(app.tasks.processes.is_empty());
    let StoreCommand::ClaimCurrent { expected, reply } = commands.recv().unwrap() else {
        panic!()
    };
    assert_eq!(
        serde_json::to_value(expected).unwrap(),
        serde_json::to_value(&task).unwrap()
    );
    reply
        .send(Err(domain::store::LOCK_CONFLICT.into()))
        .unwrap();
    app.poll_tasks();
    assert!(app.tasks.job.is_none());
    assert!(app.tasks.processes.is_empty());
    assert!(!app.tasks.storage_failed);
    assert_eq!(
        serde_json::to_value(&app.tasks.records[0]).unwrap(),
        serde_json::to_value(task).unwrap()
    );
}
#[test]
fn reparent_confirmation_cancel_and_disabled_states_never_send_writes() {
    for dark in [false, true] {
        for width in [320.0, 900.0] {
            let home =
                std::env::temp_dir().join(format!("pui-attach-cancel-{}", uuid::Uuid::new_v4()));
            let mut app = app();
            let task = fixture(&mut app, &home);
            let (store, commands) = mpsc::channel();
            app.tasks.store = Some(store);
            app.ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            app.ctx.set_zoom_factor(1.4);
            app.request_task_attachment(&task.id);
            assert!(app.tasks.attachment.confirm.is_some());
            let ctx = app.ctx.clone();
            let input = |events| egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(width, 700.0),
                )),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input(vec![]), |ctx| app.task_attachment_confirmation(ctx));
            let _ = ctx.run(
                input(vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }]),
                |ctx| app.task_attachment_confirmation(ctx),
            );
            assert!(app.tasks.attachment.confirm.is_none());
            assert!(app.tasks.attachment.pending.is_none());
            assert!(commands.try_recv().is_err());
            assert_eq!(app.tasks.records[0].parent_session, task.parent_session);
            app.demo = true;
            app.request_task_attachment(&task.id);
            assert!(app.tasks.attachment.confirm.is_none());
            assert!(commands.try_recv().is_err());
            assert!(fs::read(home.join(format!("{}.json", task.id))).is_ok());
        }
    }
}
