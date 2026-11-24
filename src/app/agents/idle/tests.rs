use super::*;
use crate::app::agents::tests::{app, spawn};
use std::fs;

fn finished(ctx: &egui::Context) -> (Pui, String, String) {
    let mut app = app(ctx);
    let parent = app.agents.active.clone();
    let child = spawn(&mut app, &parent);
    app.start_waiting_agents();
    let path = std::env::temp_dir().join(format!("pui-idle-{}.jsonl", uuid::Uuid::new_v4()));
    fs::write(&path, "{\"type\":\"session\"}\n").unwrap();
    let node = app.agents.nodes.get_mut(&child).unwrap();
    node.path = Some(path.clone());
    node.settled = true;
    node.result = Some("Kept result".into());
    node.reported = true;
    node.needs_report = false;
    node.status = "Idle".into();
    let chat = app.agents.parked.get_mut(&child).unwrap();
    chat.state = State {
        session_file: Some(path.display().to_string()),
        messages: vec![Message {
            role: "assistant".into(),
            text: "Kept result".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    chat.connected = true;
    chat.loading = false;
    chat.ready = true;
    (app, parent, child)
}

fn finish_retirement(app: &mut Pui, id: &str) {
    // Poll like the app does. Consuming the completion message here would make
    // `finish_idle_retirements` depend on the retiring thread having already
    // dropped its sender (Disconnected), which races under CI load.
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.agents.idle.retiring.contains_key(id) {
        assert!(Instant::now() < deadline, "idle retirement did not finish");
        app.finish_idle_retirements();
        std::thread::sleep(Duration::from_millis(5));
    }
    app.finish_idle_retirements();
}

#[test]
fn ten_minute_deadline_preserves_history_draft_result_and_explicit_stop_state() {
    let ctx = egui::Context::default();
    let (mut app, parent, child) = finished(&ctx);
    let path = app.agents.nodes[&child].path.clone().unwrap();
    app.agents.parked.get_mut(&child).unwrap().draft = "Kept draft".into();
    app.agents.nodes.get_mut(&child).unwrap().worktree = Some(app.prefs.cwd.clone());
    let now = Instant::now();
    app.poll_idle_agents(now);
    app.poll_idle_agents(now + IDLE_TIMEOUT - Duration::from_secs(1));
    assert!(app.agents.parked[&child].client.is_some());
    app.poll_idle_agents(now + IDLE_TIMEOUT);
    assert!(app.agents.parked[&child].client.is_none());
    assert!(app.agents.idle.dormant.contains(&child));
    assert!(!app.agents.nodes[&child].stopped);
    assert_eq!(
        app.agents.nodes[&child].result.as_deref(),
        Some("Kept result")
    );
    assert!(app.agents.nodes[&child].reported);
    assert!(app.agents.nodes[&child].worktree.is_some());
    assert_eq!(app.agents.parked[&child].draft, "Kept draft");
    assert_eq!(
        app.agents.parked[&child].state.messages[0].text,
        "Kept result"
    );
    assert_eq!(app.agents.active, parent);
    assert!(path.is_file());
    finish_retirement(&mut app, &child);
    app.view_agent(&child);
    assert!(app.client.is_some());
    assert_eq!(app.draft, "Kept draft");
    assert!(app.loading_chat);
    app.stop_agent_tree(&child);
    assert!(!app.agents.idle.dormant.contains(&child));
    app.view_agent(&parent);
    app.view_agent(&child);
    assert!(
        app.client.is_none(),
        "Navigation must not restart explicit Stop"
    );
    app.open_chat(crate::sessions::Session {
        path: path.clone(),
        cwd: app.prefs.cwd.clone(),
        project: app.prefs.cwd.clone(),
        title: "Explicitly stopped".into(),
        created: None,
        modified: std::time::SystemTime::now(),
    });
    assert!(
        app.client.is_none(),
        "Opening the selected stopped session must not resume it"
    );
    assert!(app.send_agent_message(&child, "Must not restart").is_err());
}

#[test]
fn selected_chat_can_retire_and_new_user_send_queues_only_after_explicit_send() {
    let ctx = egui::Context::default();
    let (mut app, _, child) = finished(&ctx);
    app.view_agent(&child);
    app.state.pending = false;
    app.loading_chat = false;
    app.connected = true;
    app.ultracode_ready = true;
    app.draft = "Send once after wake".into();
    let now = Instant::now();
    app.poll_idle_agents(now);
    app.poll_idle_agents(now + IDLE_TIMEOUT);
    assert!(app.client.is_none());
    assert_eq!(app.draft, "Send once after wake");
    let path = app.agents.nodes[&child].path.clone().unwrap();
    assert!(!app.waiting.contains_key(&path));
    finish_retirement(&mut app, &child);
    app.submit(&ctx);
    assert!(
        app.client.is_some(),
        "error={:?} loading={} state_ready={} dormant={} stopped={} draft={:?}",
        app.state.error,
        app.loading_chat,
        app.state.ready(),
        app.agents.idle.dormant.contains(&child),
        app.agents.nodes[&child].stopped,
        app.draft
    );
    assert!(app.draft.is_empty());
    assert_eq!(app.waiting[&path][0].text, "Send once after wake");
    assert!(
        app.deliveries.is_empty(),
        "Reconnect must not send before adapter readiness"
    );
    app.loading_chat = false;
    app.connected = true;
    app.ultracode_ready = true;
    app.poll_idle_agents(now + IDLE_TIMEOUT + Duration::from_secs(1));
    assert!(app.waiting[&path].is_empty());
    assert_eq!(app.deliveries.len(), 1);
    app.poll_idle_agents(now + IDLE_TIMEOUT + Duration::from_secs(2));
    assert_eq!(app.deliveries.len(), 1, "No duplicate delivery");
}

#[test]
fn running_tools_compaction_questions_queue_reports_and_dependencies_never_expire() {
    for case in 0..17 {
        let ctx = egui::Context::default();
        let (mut app, parent, child) = finished(&ctx);
        let path = app.agents.nodes[&child].path.clone().unwrap();
        let now = Instant::now();
        app.poll_idle_agents(now);
        match case {
            0 => app.agents.parked.get_mut(&child).unwrap().state.busy = true,
            1 => app.agents.parked.get_mut(&child).unwrap().state.compacting = true,
            2 => app
                .agents
                .parked
                .get_mut(&child)
                .unwrap()
                .state
                .dialogs
                .push_back(json!({"method":"input","title":"Question"})),
            3 => app
                .agents
                .parked
                .get_mut(&child)
                .unwrap()
                .state
                .dialogs
                .push_back(json!({"method":"confirm","title":"Approval"})),
            4 => app.agents.parked.get_mut(&child).unwrap().state.pending = true,
            5 => app.agents.nodes.get_mut(&child).unwrap().settled = false,
            6 => {
                app.waiting
                    .insert(path, vec![run_status::WaitingMessage::default()]);
            }
            7 => app.agents.nodes.get_mut(&child).unwrap().reported = false,
            8 => {
                let node = app.agents.nodes.get_mut(&child).unwrap();
                node.reported = false;
                node.needs_report = true;
            }
            9 => {
                app.agents
                    .mail
                    .insert("pending".into(), (child.clone(), None));
            }
            10 => {
                let other = spawn(&mut app, &parent);
                app.agents.nodes.get_mut(&other).unwrap().parent = Some(child.clone());
                app.agents
                    .nodes
                    .get_mut(&child)
                    .unwrap()
                    .children
                    .push(other);
            }
            11 => {
                let other = spawn(&mut app, &parent);
                app.agents
                    .nodes
                    .get_mut(&other)
                    .unwrap()
                    .depends
                    .push(child.clone());
            }
            12 => app.agents.parked.get_mut(&child).unwrap().loading = true,
            13 => {
                app.agents
                    .parked
                    .get_mut(&child)
                    .unwrap()
                    .state
                    .run_observed = true
            }
            14 => app.agents.parked.get_mut(&child).unwrap().state.messages[0]
                .tools
                .push(protocol::Tool {
                    finished: false,
                    ..Default::default()
                }),
            15 => app.agents.parked.get_mut(&child).unwrap().state.retrying = true,
            _ => app
                .agents
                .parked
                .get_mut(&child)
                .unwrap()
                .state
                .pending_messages
                .push(protocol::PendingMessage {
                    id: "uncertain".into(),
                    status: protocol::DeliveryStatus::Uncertain,
                    text: String::new(),
                    images: Vec::new(),
                    observed: 0,
                }),
        }
        if case == 7 {
            app.agents.nodes.get_mut(&child).unwrap().needs_report = true;
        }
        app.poll_idle_agents(now + IDLE_TIMEOUT * 3);
        assert!(app.agents.parked[&child].client.is_some(), "case {case}");
        assert!(
            !app.agents.idle.since.contains_key(&child),
            "case {case} must reset the grace period"
        );
        app.shutdown_agents();
    }
}

#[test]
fn resumed_activity_and_input_wait_restart_the_full_grace_period() {
    let ctx = egui::Context::default();
    let (mut app, parent, child) = finished(&ctx);
    let now = Instant::now();
    app.poll_idle_agents(now);
    app.agents
        .parked
        .get_mut(&child)
        .unwrap()
        .state
        .dialogs
        .push_back(json!({"method":"input"}));
    app.poll_idle_agents(now + IDLE_TIMEOUT * 2);
    app.agents
        .parked
        .get_mut(&child)
        .unwrap()
        .state
        .dialogs
        .clear();
    app.poll_idle_agents(now + IDLE_TIMEOUT * 2);
    app.poll_idle_agents(now + IDLE_TIMEOUT * 3 - Duration::from_secs(1));
    assert!(app.agents.parked[&child].client.is_some());
    app.view_agent(&child);
    app.agent_event(&json!({"type":"agent_start"}));
    app.agent_event(&json!({"type":"agent_settled"}));
    app.view_agent(&parent);
    app.poll_idle_agents(now + IDLE_TIMEOUT * 3);
    assert!(app.agents.parked[&child].client.is_some());
    app.poll_idle_agents(now + IDLE_TIMEOUT * 4);
    assert!(app.agents.parked[&child].client.is_none());
    finish_retirement(&mut app, &child);
}

#[test]
fn managed_followup_waits_for_old_writer_and_adapter_then_sends_exactly_once() {
    let ctx = egui::Context::default();
    let (mut app, parent, child) = finished(&ctx);
    let now = Instant::now();
    app.poll_idle_agents(now);
    app.poll_idle_agents(now + IDLE_TIMEOUT);
    app.agent_control(
        &parent,
        &json!({"action":"message","agent_id":child,"message":"Next scoped task","kind":"task"}),
    )
    .unwrap();
    assert!(
        app.agents.parked[&child].client.is_none(),
        "Wait for old writer to exit"
    );
    assert_eq!(app.agents.idle.mail.len(), 1);
    finish_retirement(&mut app, &child);
    assert!(app.agents.parked[&child].client.is_some());
    let (client, _, commands) = rpc::Client::stub_with_records();
    let chat = app.agents.parked.get_mut(&child).unwrap();
    chat.client = Some(client);
    chat.loading = false;
    chat.connected = true;
    chat.ready = false;
    app.poll_idle_agents(now + IDLE_TIMEOUT);
    assert!(commands.try_recv().is_err());
    app.agents.parked.get_mut(&child).unwrap().ready = true;
    app.poll_idle_agents(now + IDLE_TIMEOUT);
    let record = commands.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(
        record["message"]
            .as_str()
            .unwrap()
            .contains("Next scoped task")
    );
    assert_eq!(app.agents.active, parent);
    assert!(app.agents.idle.mail.is_empty());
    assert_eq!(app.agents.mail.len(), 1);
    assert!(!app.agents.nodes[&child].settled);
    assert!(app.agents.nodes[&child].needs_report);
    assert!(app.agents.nodes[&child].result.is_none());
    app.poll_idle_agents(now + IDLE_TIMEOUT * 2);
    assert!(commands.try_recv().is_err());
}

#[test]
fn finished_root_waits_for_children_and_active_workflow_then_retires_without_stop() {
    let ctx = egui::Context::default();
    let (mut app, parent, child) = finished(&ctx);
    app.view_agent(&child);
    app.stop_agent_tree(&child);
    app.view_agent(&parent);
    let path = app.agents.nodes[&child].path.clone().unwrap();
    app.state.session_file = Some(path.display().to_string());
    app.agents.nodes.get_mut(&parent).unwrap().path = Some(path.clone());
    app.agents.nodes.get_mut(&parent).unwrap().settled = true;
    let mut task = crate::tasks::Task::new(
        app.prefs.cwd.clone(),
        "local".into(),
        "fixture".into(),
        "off".into(),
    );
    task.parent_session = Some(path);
    task.phase = crate::tasks::Phase::Building;
    app.tasks.parents.insert(task.id.clone(), parent.clone());
    app.tasks.records.push(task);
    let now = Instant::now();
    app.poll_idle_agents(now);
    app.poll_idle_agents(now + IDLE_TIMEOUT * 2);
    assert!(app.client.is_some(), "Active workflow protects its root");
    app.tasks.records.last_mut().unwrap().phase = crate::tasks::Phase::Ready;
    app.agents.nodes.get_mut(&child).unwrap().stopped = false;
    app.agents.nodes.get_mut(&child).unwrap().settled = false;
    app.poll_idle_agents(now + IDLE_TIMEOUT * 2);
    assert!(
        !app.agents.idle.since.contains_key(&parent),
        "Unfinished descendant protects parent"
    );
    app.agents.nodes.get_mut(&child).unwrap().settled = true;
    app.poll_idle_agents(now + IDLE_TIMEOUT * 2);
    app.poll_idle_agents(now + IDLE_TIMEOUT * 3);
    assert!(app.client.is_none());
    assert!(!app.agents.nodes[&parent].stopped);
    finish_retirement(&mut app, &parent);
}

#[test]
fn explicit_stop_during_idle_reconnect_discards_unsent_mail_without_restarting() {
    let ctx = egui::Context::default();
    let (mut app, parent, child) = finished(&ctx);
    let now = Instant::now();
    app.poll_idle_agents(now);
    app.poll_idle_agents(now + IDLE_TIMEOUT);
    app.send_agent_message(&child, "Do not deliver after Stop")
        .unwrap();
    app.stop_agent_tree(&child);
    finish_retirement(&mut app, &child);
    assert!(app.agents.parked[&child].client.is_none());
    assert!(app.agents.idle.mail.is_empty());
    assert!(app.agents.idle.wake.is_empty());
    assert!(app.agents.nodes[&child].stopped);
    assert!(
        app.agent_control(
            &parent,
            &json!({"action":"message","agent_id":child,"message":"No automatic restart"})
        )
        .is_err()
    );
}

#[test]
fn unknown_cross_window_presence_prevents_timeout_and_resets_timer() {
    let ctx = egui::Context::default();
    let (mut app, _, child) = finished(&ctx);
    let now = Instant::now();
    app.poll_idle_agents(now);
    let registry = std::env::temp_dir().join(format!("pui-idle-registry-{}", uuid::Uuid::new_v4()));
    app.sync = sync::WindowSync::new_at(&ctx, false, Some(registry));
    assert!(!app.sync.can_evict_owned());
    app.poll_idle_agents(now + IDLE_TIMEOUT * 2);
    assert!(app.agents.parked[&child].client.is_some());
    assert!(app.agents.idle.since.is_empty());
    app.sync = sync::WindowSync::new(&ctx, true);
    app.poll_idle_agents(now + IDLE_TIMEOUT * 2);
    assert!(app.agents.parked[&child].client.is_some());
    app.shutdown_agents();
}

#[test]
fn linked_automation_keeps_a_finished_owner_alive() {
    let ctx = egui::Context::default();
    let (mut app, _, child) = finished(&ctx);
    app.view_agent(&child);
    app.agents.nodes.get_mut(&child).unwrap().access = "worktree".into();
    app.agents.nodes.get_mut(&child).unwrap().permission = Some(settings::PermissionMode::Manual);
    app.automation_control(&child, &json!({"action":"automation_create","kind":"cron","name":"Keep owner","message":"Check later","schedule":"0 9 * * *"})).unwrap();
    let now = Instant::now();
    app.poll_idle_agents(now);
    app.poll_idle_agents(now + IDLE_TIMEOUT * 3);
    assert!(app.client.is_some());
    assert!(!app.agents.idle.since.contains_key(&child));
    app.shutdown_agents();
}

#[cfg(unix)]
#[test]
fn real_finished_process_is_gracefully_retired_and_reaped_off_ui_thread() {
    let ctx = egui::Context::default();
    let (mut app, _, child) = finished(&ctx);
    let mut client = rpc::Client::process_fixture(ctx);
    client.send(json!({"type":"get_state"})).unwrap();
    let rpc::Event::Record(record) = client.events.recv_timeout(Duration::from_secs(5)).unwrap()
    else {
        panic!("Fixture state response")
    };
    let pid = record["data"]["fixturePid"].as_i64().unwrap() as libc::pid_t;
    app.agents.parked.get_mut(&child).unwrap().client = Some(client);
    let now = Instant::now();
    app.poll_idle_agents(now);
    app.poll_idle_agents(now + IDLE_TIMEOUT);
    assert!(app.agents.parked[&child].client.is_none());
    finish_retirement(&mut app, &child);
    // SAFETY: observe only this fixture child; no signal is sent to another process.
    assert_eq!(
        unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn activity_in_selected_chat_resets_only_its_countdown() {
    let ctx = egui::Context::default();
    let (mut app, _, child) = finished(&ctx);
    app.view_agent(&child);
    let now = Instant::now();
    app.poll_idle_agents(now);
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Text("typing in draft".into())],
            ..Default::default()
        },
        |_| app.poll_idle_agents(now + IDLE_TIMEOUT),
    );
    assert!(app.client.is_some());
    assert_eq!(app.agents.idle.since[&child], now + IDLE_TIMEOUT);
}

#[test]
fn reconnect_question_or_error_never_triggers_followup_or_automatic_retry() {
    for failure in [false, true] {
        let ctx = egui::Context::default();
        let (mut app, _, child) = finished(&ctx);
        let now = Instant::now();
        app.poll_idle_agents(now);
        app.poll_idle_agents(now + IDLE_TIMEOUT);
        finish_retirement(&mut app, &child);
        app.send_agent_message(&child, "Only after safe reconnection")
            .unwrap();
        let (client, _, commands) = rpc::Client::stub_with_records();
        let chat = app.agents.parked.get_mut(&child).unwrap();
        chat.client = Some(client);
        chat.connected = true;
        chat.loading = false;
        chat.ready = true;
        if failure {
            chat.state.error = Some("Reconnect failed".into());
        } else {
            chat.state
                .dialogs
                .push_back(json!({"method":"confirm","id":"reconnect-approval"}));
        }
        app.poll_idle_agents(now + IDLE_TIMEOUT * 2);
        app.poll_idle_agents(now + IDLE_TIMEOUT * 3);
        assert!(commands.try_recv().is_err());
        assert!(app.agents.idle.dormant.contains(&child));
        if failure {
            assert!(app.agents.idle.mail.is_empty());
            assert!(app.agents.nodes[&child].failure.is_some());
        } else {
            assert_eq!(app.agents.idle.mail.len(), 1);
            assert_eq!(app.agents.parked[&child].state.dialogs.len(), 1);
        }
        app.shutdown_agents();
    }
}

#[test]
fn managed_followups_keep_fifo_order_and_one_reconnect() {
    let ctx = egui::Context::default();
    let (mut app, _, child) = finished(&ctx);
    let now = Instant::now();
    app.poll_idle_agents(now);
    app.poll_idle_agents(now + IDLE_TIMEOUT);
    finish_retirement(&mut app, &child);
    app.send_agent_message(&child, "first followup").unwrap();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.agents.parked.get_mut(&child).unwrap().client = Some(client);
    app.send_agent_message(&child, "second followup").unwrap();
    assert!(
        commands.try_recv().is_err(),
        "Second followup must not replace the reconnecting client"
    );
    let chat = app.agents.parked.get_mut(&child).unwrap();
    chat.loading = false;
    chat.connected = true;
    chat.ready = true;
    app.poll_idle_agents(now + IDLE_TIMEOUT);
    let first = commands.recv_timeout(Duration::from_secs(3)).unwrap();
    let second = commands.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(
        first["message"]
            .as_str()
            .unwrap()
            .contains("first followup")
    );
    assert!(
        second["message"]
            .as_str()
            .unwrap()
            .contains("second followup")
    );
    assert_eq!(app.agents.mail.len(), 2);
    assert!(app.agents.idle.mail.is_empty());
    app.shutdown_agents();
}

#[test]
fn queued_question_is_drained_before_retirement() {
    let ctx = egui::Context::default();
    let (mut app, _, child) = finished(&ctx);
    let (client, events) = rpc::Client::stub_with_events();
    app.agents.parked.get_mut(&child).unwrap().client = Some(client);
    let now = Instant::now();
    app.poll_idle_agents(now);
    events.send(rpc::Event::Record(json!({"type":"extension_ui_request","method":"input","id":"late-question","title":"Need an answer"}))).unwrap();
    app.poll_idle_agents(now + IDLE_TIMEOUT);
    assert!(app.agents.parked[&child].client.is_some());
    assert_eq!(app.agents.parked[&child].state.dialogs.len(), 1);
}
