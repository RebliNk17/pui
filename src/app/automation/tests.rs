use super::*;
fn app(ctx: &egui::Context) -> Pui {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.automation = Default::default();
    app.state.session_file = Some(
        std::env::temp_dir()
            .join("pui-job-owner.jsonl")
            .display()
            .to_string(),
    );
    app.ultracode_ready = true;
    app.connected = true;
    app.client = Some(rpc::Client::stub());
    app.ensure_agent();
    // Keep record paths absolute on every test runner.
    app.prefs.cwd = std::env::temp_dir();
    let id = app.agents.active.clone();
    app.agents.nodes.get_mut(&id).unwrap().cwd = app.prefs.cwd.clone();
    app
}
fn create(app: &mut Pui, kind: &str) -> String {
    let source = app.agents.active.clone();
    app.automation_control(&source, &json!({"action":"automation_create","kind":kind,"name":"CI ready","message":"Review the result","command":"exit 0","interval_seconds":10,"schedule":"*/5 * * * *"})).unwrap()["job"]["job_id"].as_str().unwrap().into()
}
#[test]
fn creator_scoping_validation_and_pause_resume() {
    let mut a = app(&egui::Context::default());
    let id = create(&mut a, "cron");
    assert_eq!(a.automation.jobs[0].creator.id, a.agents.active);
    assert!(
        a.automation_control(
            "agent-999",
            &json!({"action":"automation_update","job_id":id,"operation":"delete"})
        )
        .is_err()
    );
    let source = a.agents.active.clone();
    a.prefs.settings.permission = settings::PermissionMode::Plan;
    assert!(
        a.automation_control(
            &source,
            &json!({"action":"automation_create","kind":"cron"})
        )
        .is_err()
    );
    a.change_job(&id, "pause").unwrap();
    assert!(!a.automation.jobs[0].enabled);
    a.change_job(&id, "resume").unwrap();
    assert!(a.automation.jobs[0].enabled);
    a.stop_agent_tree(&source);
    assert!(!a.automation.jobs[0].enabled);
}
#[test]
fn scheduled_message_routes_to_creator_and_waits_for_ack() {
    let mut a = app(&egui::Context::default());
    let (client, _, commands) = rpc::Client::stub_with_commands();
    a.client = Some(client);
    create(&mut a, "cron");
    a.automation.jobs[0].next_run = now() - 1;
    a.poll_automation(); // mark due, persist pending
    assert!(a.automation.jobs[0].pending.is_some());
    a.poll_automation(); // enqueue followup
    let command = commands.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(protocol::string(&command, "message").starts_with("/pui-agent-message "));
    assert!(a.automation.jobs[0].dispatching);
    assert_eq!(a.automation.jobs[0].deliveries, 0);
    let key = a.automation.mail.keys().next().unwrap().clone();
    a.automation_response(&key, &json!({"success":true}));
    assert_eq!(a.automation.jobs[0].deliveries, 1);
    assert!(a.automation.jobs[0].pending.is_none());
    a.poll_automation();
    assert!(commands.try_recv().is_err());
}
#[test]
fn monitor_only_sends_on_success_once_and_cancellation_discards_result() {
    let mut a = app(&egui::Context::default());
    let id = create(&mut a, "monitor");
    let (tx, rx) = mpsc::channel();
    tx.send(runner::Check {
        success: false,
        output: "not ready".into(),
        error: None,
    })
    .unwrap();
    a.automation.running.insert(
        id.clone(),
        Running {
            cancel: Arc::new(AtomicBool::new(false)),
            rx,
        },
    );
    a.poll_automation();
    assert!(a.automation.jobs[0].pending.is_none());
    assert!(a.automation.mail.is_empty());
    let (tx, rx) = mpsc::channel();
    tx.send(runner::Check {
        success: true,
        output: "ready".into(),
        error: None,
    })
    .unwrap();
    a.automation.running.insert(
        id.clone(),
        Running {
            cancel: Arc::new(AtomicBool::new(false)),
            rx,
        },
    );
    a.poll_automation();
    let key = a.automation.mail.keys().next().unwrap().clone();
    a.automation_response(&key, &json!({"success":true}));
    assert!(a.automation.jobs[0].completed);
    assert!(!a.automation.jobs[0].enabled);
    a.change_job(&id, "resume").unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let (_, rx) = mpsc::channel();
    a.automation.running.insert(
        id.clone(),
        Running {
            cancel: cancel.clone(),
            rx,
        },
    );
    a.change_job(&id, "delete").unwrap();
    assert!(cancel.load(Ordering::Acquire));
    assert!(a.automation.jobs.is_empty());
}
#[test]
fn persisted_active_jobs_restore_creator_and_reconnect_without_prompt_or_navigation() {
    let ctx = egui::Context::default();
    let mut a = app(&ctx);
    create(&mut a, "cron");
    let creator = a.automation.jobs[0].creator.clone();
    let path = std::env::temp_dir().join(format!("pui-job-{}.jsonl", uuid::Uuid::new_v4()));
    std::fs::write(&path, "").unwrap();
    a.automation.jobs[0].creator.path = path.clone();
    a.automation.jobs[0].creator.id = a.agents.active.clone();
    a.automation.jobs[0].creator.name = "Saved creator".into();
    let visible = a.agents.active.clone();
    let cwd = a.prefs.cwd.clone();
    let draft = "unfinished draft";
    a.draft = draft.into();
    a.restore_job_owners();
    let restored = a.automation.jobs[0].creator.id.clone();
    assert_ne!(restored, visible);
    assert!(a.agents.nodes.contains_key(&restored));
    a.reconnect_job_owner(&restored).unwrap();
    assert!(a.agents.parked[&restored].client.is_some());
    assert_eq!(a.agents.active, visible);
    assert_eq!(a.prefs.cwd, cwd);
    assert_eq!(a.draft, draft);
    assert!(!a.agents.nodes[&restored].stopped);
    assert!(a.automation.mail.is_empty());
    assert_eq!(creator.id, visible);
}
#[test]
fn store_lock_restart_missed_cron_and_uncertain_delivery() {
    let mut a = app(&egui::Context::default());
    create(&mut a, "cron");
    let home = std::env::temp_dir().join(format!("pui-automation-{}", uuid::Uuid::new_v4()));
    let lock = store::lock(&home).unwrap();
    assert!(store::lock(&home).is_err());
    a.automation.jobs[0].next_run = 0;
    store::save(&home, &a.automation.jobs).unwrap();
    drop(lock);
    let mut scheduler = Automation {
        home: Some(home.clone()),
        ..Default::default()
    };
    let resumed_at = now();
    scheduler.refresh();
    assert!(scheduler.jobs[0].enabled);
    assert!(scheduler.jobs[0].next_run > resumed_at);
    scheduler.jobs[0].dispatching = true;
    scheduler.jobs[0].pending = Some("event".into());
    scheduler.persist().unwrap();
    drop(scheduler);
    let mut scheduler = Automation {
        home: Some(home),
        ..Default::default()
    };
    scheduler.refresh();
    assert!(!scheduler.jobs[0].enabled);
    assert!(!scheduler.jobs[0].dispatching);
    assert!(scheduler.jobs[0].pending.is_some());
    assert!(
        scheduler.jobs[0]
            .error
            .as_ref()
            .unwrap()
            .contains("in flight")
    );
}
#[test]
fn busy_creator_coalesces_cron_and_quitting_keeps_jobs_enabled() {
    let mut a = app(&egui::Context::default());
    let (client, _, commands) = rpc::Client::stub_with_commands();
    a.client = Some(client);
    create(&mut a, "cron");
    a.automation.jobs[0].next_run = now() - 1;
    a.state.busy = true;
    a.poll_automation();
    a.poll_automation();
    assert!(a.automation.jobs[0].pending.is_some());
    assert!(a.automation.mail.is_empty());
    assert!(commands.try_recv().is_err());
    a.automation.jobs[0].next_run = now() - 600;
    a.state.busy = false;
    a.poll_automation();
    let key = a.automation.mail.keys().next().unwrap().clone();
    a.automation_response(&key, &json!({"success":true}));
    assert!(a.automation.jobs[0].next_run > now());
    a.agents.exiting = true;
    let owner = a.agents.active.clone();
    a.stop_agent_tree(&owner);
    assert!(a.automation.jobs[0].enabled);
}
#[test]
fn background_delivery_never_navigates_or_sends_to_visible_chat() {
    let mut a = app(&egui::Context::default());
    create(&mut a, "cron");
    let owner = a.agents.active.clone();
    let (client, _, commands) = rpc::Client::stub_with_commands();
    a.client = Some(client);
    a.park_tree();
    a.state.session_file = Some(
        std::env::temp_dir()
            .join("pui-visible-other.jsonl")
            .display()
            .to_string(),
    );
    a.ensure_agent();
    a.connected = true;
    a.ultracode_ready = true;
    a.client = Some(rpc::Client::stub());
    let visible = a.agents.active.clone();
    a.draft = "visible draft".into();
    a.automation.jobs[0].pending = Some("Owner-only event".into());
    a.poll_automation();
    let record = commands.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(protocol::string(&record, "message").contains("Owner-only event"));
    assert_eq!(a.agents.active, visible);
    assert_ne!(visible, owner);
    assert_eq!(a.draft, "visible draft");
    assert!(
        a.automation
            .mail
            .keys()
            .all(|key| key.starts_with(&format!("{owner}:")))
    );
}
#[test]
fn rejected_or_unacknowledged_delivery_does_not_retry() {
    let mut a = app(&egui::Context::default());
    create(&mut a, "cron");
    a.automation.jobs[0].pending = Some("event".into());
    a.poll_automation();
    let key = a.automation.mail.keys().next().unwrap().clone();
    a.automation.mail.get_mut(&key).unwrap().1 =
        std::time::Instant::now() - Duration::from_secs(31);
    a.poll_automation();
    assert!(!a.automation.jobs[0].enabled);
    assert!(a.automation.jobs[0].pending.is_some());
    assert!(!a.automation.jobs[0].dispatching);
    assert!(a.automation.mail.is_empty());
}
#[test]
fn view_exposes_jobs_and_creator() {
    let ctx = egui::Context::default();
    let mut a = app(&ctx);
    create(&mut a, "cron");
    create(&mut a, "monitor");
    let _ = ctx.run(egui::RawInput::default(), |ctx| a.automation_panel(ctx));
}
