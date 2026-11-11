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
fn sidebar_shows_active_schedules_and_updates_when_paused_or_completed() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        for width in [220.0, 300.0] {
            for zoom in [0.8, 1.4] {
                let ctx = egui::Context::default();
                let mut a = app(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let cron = create(&mut a, "cron");
                create(&mut a, "monitor");
                a.state.busy = true;
                a.draft = "Keep my draft".into();
                for expected in [2, 1, 0] {
                    assert_eq!(a.automation.active_count(), expected);
                    let mut text = Vec::new();
                    for _ in 0..2 {
                        let output = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    Vec2::new(width, 200.0),
                                )),
                                ..Default::default()
                            },
                            |ctx| {
                                egui::CentralPanel::default()
                                    .show(ctx, |ui| a.sidebar_navigation(ui));
                            },
                        );
                        text.clear();
                        for shape in &output.shapes {
                            labels(&shape.shape, &mut text);
                        }
                    }
                    let badges: Vec<_> = text
                        .iter()
                        .filter(|(label, _)| label.ends_with(" active"))
                        .collect();
                    if expected > 0 {
                        assert_eq!(badges.len(), 1);
                        assert_eq!(badges[0].0, format!("{expected} active"));
                        let scheduled = text
                            .iter()
                            .find(|(label, _)| label == "Scheduled tasks")
                            .unwrap()
                            .1;
                        assert!(scheduled.right() < badges[0].1.left());
                        assert!(badges[0].1.right() <= width);
                    } else {
                        assert!(badges.is_empty());
                    }
                    assert!(a.state.busy);
                    assert_eq!(a.draft, "Keep my draft");
                    if expected == 2 {
                        a.change_job(&cron, "pause").unwrap();
                    } else if expected == 1 {
                        // Completed jobs are never counted, even if an old snapshot
                        // still has their enabled flag set.
                        a.automation.jobs[1].completed = true;
                    }
                }
            }
        }
    }
}

#[test]
fn view_exposes_jobs_and_creator() {
    let ctx = egui::Context::default();
    let mut a = app(&ctx);
    create(&mut a, "cron");
    create(&mut a, "monitor");
    let _ = ctx.run(egui::RawInput::default(), |ctx| a.automation_panel(ctx));
}

fn labels(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(text) => out.push((
            text.galley.job.text.clone(),
            egui::Rect::from_min_size(text.pos, text.galley.size()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                labels(shape, out);
            }
        }
        _ => {}
    }
}

fn delete_frame(
    a: &mut Pui,
    ctx: &egui::Context,
    size: Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| a.automation_panel(ctx),
    )
}

#[test]
fn delete_job_uses_the_shared_bounded_destructive_confirmation() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        for size in [Vec2::new(420.0, 620.0), Vec2::new(1480.0, 960.0)] {
            for zoom in [0.8, 1.0, 1.4] {
                let ctx = egui::Context::default();
                let mut a = app(&ctx);
                mode.apply(&ctx);
                ctx.style_mut(|style| style.animation_time = 0.0);
                ctx.set_zoom_factor(zoom);
                let id = create(&mut a, "cron");
                a.automation.delete = Some(id.clone());
                for _ in 0..4 {
                    delete_frame(&mut a, &ctx, size, vec![]);
                }
                let output = delete_frame(&mut a, &ctx, size, vec![]);
                let rect = ctx
                    .read_response(egui::Id::new("delete-automation"))
                    .unwrap()
                    .rect;
                assert!(
                    ctx.screen_rect().contains_rect(rect),
                    "{rect:?} at {size:?}/{zoom}"
                );
                assert!(
                    rect.width() <= 410.0,
                    "No full-width legacy prompts: {rect:?}"
                );
                let mut text = Vec::new();
                for shape in &output.shapes {
                    labels(&shape.shape, &mut text);
                }
                let cancel = text.iter().find(|(label, _)| label == "Cancel").unwrap().1;
                let confirm = text
                    .iter()
                    .find(|(label, _)| label == "Delete job")
                    .unwrap()
                    .1;
                assert!(cancel.right() < confirm.left());
                assert!((cancel.center().y - confirm.center().y).abs() < 1.0);
                assert!(rect.contains_rect(confirm));
                assert!(
                    text.iter()
                        .any(|(label, bounds)| label == "CI ready" && rect.contains_rect(*bounds))
                );
                assert!(text.iter().any(|(label, _)| label
                    == "The creator chat and any already delivered messages are kept."));
                let danger = theme::palette(&ctx).red;
                assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Rect(button) if button.fill == danger && button.rect.contains_rect(confirm)
                )), "Destructive action must use the shared danger button");
                assert_eq!(a.automation.delete.as_deref(), Some(id.as_str()));
                assert_eq!(a.automation.jobs.len(), 1);
            }
        }
    }
}

#[test]
fn delete_job_only_deletes_on_explicit_enabled_confirmation() {
    for action in [
        "escape",
        "backdrop",
        "cancel",
        "enter",
        "confirm",
        "read-only",
        "demo",
    ] {
        let ctx = egui::Context::default();
        let mut a = app(&ctx);
        let id = create(&mut a, "cron");
        let creator = a.automation.jobs[0].creator.id.clone();
        a.automation.delete = Some(id);
        a.draft = "Keep my draft".into();
        if action == "read-only" {
            a.automation.failed = true;
        }
        if action == "demo" {
            a.demo = true;
        }
        let size = Vec2::new(860.0, 620.0);
        for _ in 0..4 {
            delete_frame(&mut a, &ctx, size, vec![]);
        }
        let output = delete_frame(&mut a, &ctx, size, vec![]);
        let mut text = Vec::new();
        for shape in &output.shapes {
            labels(&shape.shape, &mut text);
        }
        let events = if matches!(action, "escape" | "enter") {
            vec![egui::Event::Key {
                key: if action == "escape" {
                    egui::Key::Escape
                } else {
                    egui::Key::Enter
                },
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]
        } else {
            let pos = if action == "backdrop" {
                egui::pos2(5.0, 5.0)
            } else {
                let label = if action == "cancel" {
                    "Cancel"
                } else {
                    "Delete job"
                };
                text.iter()
                    .find(|(text, _)| text == label)
                    .unwrap()
                    .1
                    .center()
            };
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        delete_frame(&mut a, &ctx, size, events);
        assert_eq!(
            a.automation.jobs.is_empty(),
            action == "confirm",
            "{action}"
        );
        if matches!(action, "escape" | "backdrop" | "cancel" | "confirm") {
            assert!(
                a.automation.delete.is_none(),
                "{action} must dismiss the dialog"
            );
        }
        assert!(a.agents.nodes.contains_key(&creator));
        assert_eq!(a.draft, "Keep my draft");
    }
}
