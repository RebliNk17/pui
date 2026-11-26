use super::*;
use std::time::Instant;

fn test_app() -> (egui::Context, Pui) {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.ultracode_ready = true;
    let root = app.ensure_agent();
    app.agents.nodes.get_mut(&root).unwrap().permission = Some(settings::PermissionMode::Bypass);
    app.state.provider = "anthropic".into();
    app.state.model = "claude-opus-5-5".into();
    (ctx, app)
}

fn collect_texts(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
        egui::Shape::Vec(shapes) => {
            for s in shapes {
                collect_texts(s, out);
            }
        }
        _ => {}
    }
}

#[test]
fn stopped_background_shell_blocks_update_until_process_completion_is_observed() {
    let (_, mut app) = test_app();
    let owner = app.agents.active.clone();
    let (tx, rx) = mpsc::channel();
    app.background_tasks.shells.push(ShellTask {
        id: "bg-shell-update-test".into(),
        owner: owner.clone(),
        root: owner,
        workflow_id: None,
        title: "Retiring shell".into(),
        command: "fixture only".into(),
        cwd: app.prefs.cwd.clone(),
        started: SystemTime::now(),
        finished: None,
        status: ShellStatus::Stopped,
        output: String::new(),
        truncated: false,
        error: None,
        cancel: Arc::new(AtomicBool::new(true)),
        rx: Some(rx),
        notified: true,
    });
    assert!(app.background_tasks.update_restart_blocked());
    app.clear_finished_background_tasks();
    assert_eq!(
        app.background_tasks.shells.len(),
        1,
        "don't forget a stopped but unreaped shell"
    );
    tx.send(ShellEvent::Done {
        exit_code: None,
        error: None,
        stopped: true,
    })
    .unwrap();
    app.poll_background_tasks();
    assert!(!app.background_tasks.update_restart_blocked());
    app.clear_finished_background_tasks();
    assert!(app.background_tasks.shells.is_empty());
}

#[test]
fn sidebar_child_sessions_and_background_subagents_are_strictly_separated() {
    let (_ctx, mut app) = test_app();
    let root = app.agents.active.clone();

    // 1. Spawn a sidebar child session (`pui_spawn_agent`).
    let session_res = app
        .agent_control(
            &root,
            &json!({
                "action": "spawn",
                "name": "Fix North Korea tank gun alignment",
                "task": "Fix the turret alignment on a dedicated branch.",
                "access": "read-only"
            }),
        )
        .expect("spawn sidebar child session");
    let session_id = session_res["agent_id"].as_str().unwrap().to_owned();
    assert_eq!(session_res["kind"], "session");
    assert!(!app.agents.nodes[&session_id].background);

    // 2. Spawn a lightweight background subagent (`pui_background_agent`).
    let bg_res = app
        .agent_control(
            &root,
            &json!({
                "action": "spawn_background",
                "name": "Groups: left order column + groups popup",
                "task": "Implement the mobile left order column.",
                "model": "anthropic/claude-sonnet-4-6"
            }),
        )
        .expect("spawn background subagent");
    let bg_id = bg_res["agent_id"].as_str().unwrap().to_owned();
    assert_eq!(bg_res["kind"], "background");
    assert_eq!(bg_res["access"], "workspace");
    assert_eq!(bg_res["model"], "anthropic/claude-sonnet-4-6");
    assert!(app.agents.nodes[&bg_id].background);

    // 3. Add an unrelated top-level chat in `agents.nodes` to ensure it never leaks.
    let other_id = "agent-999".to_owned();
    app.agents.nodes.insert(
        other_id.clone(),
        agents::Node {
            id: other_id.clone(),
            parent: None,
            name: "Unrelated other project chat".into(),
            cwd: app.prefs.cwd.clone(),
            path: None,
            access: "workspace".into(),
            permission: Some(settings::PermissionMode::Auto),
            status: "Working".into(),
            last_activity: None,
            created: Some(SystemTime::now()),
            children: Vec::new(),
            stopped: false,
            reported: true,
            needs_report: false,
            settled: false,
            launch: None,
            failure: None,
            result: None,
            report_summary: None,
            task: String::new(),
            depends: Vec::new(),
            setup: None,
            worktree: None,
            cloud_host: None,
            cloud_repo: None,
            background: false,
            workflow_id: None,
            model: None,
            finished_at: None,
        },
    );

    // Sidebar listing contains the child session, NEVER the background subagent.
    let (pinned, rows) = sidebar_tree::listing(&app);
    let mut titles = Vec::new();
    fn walk(branches: &[sidebar_tree::Branch], out: &mut Vec<String>) {
        for b in branches {
            out.push(b.title.clone());
            walk(&b.children, out);
        }
    }
    walk(&pinned, &mut titles);
    walk(&rows, &mut titles);
    assert!(
        titles
            .iter()
            .any(|t| t == "Fix North Korea tank gun alignment"),
        "sidebar child session must appear in sidebar: {titles:?}"
    );
    assert!(
        !titles
            .iter()
            .any(|t| t == "Groups: left order column + groups popup"),
        "background subagent must never clutter the left sidebar: {titles:?}"
    );

    // Kanban board excludes background subagents.
    let board = app.board_cards();
    assert!(
        !board
            .iter()
            .any(|c| matches!(&c.target, kanban::Target::Agent(id) if id == &bg_id)),
        "background subagent should not appear on the Kanban board"
    );

    // Background counts include ONLY the 1 background subagent (not the sidebar child, not the unrelated chat).
    assert_eq!(app.active_background_counts(), (1, 0));

    // Clicking/viewing a background subagent opens the Background tasks detail view,
    // without switching `agents.active`.
    app.view_agent(&bg_id);
    assert_eq!(app.agents.active, root);
    assert!(app.background_tasks.open);
    assert_eq!(
        app.background_tasks.selected_agent.as_deref(),
        Some(bg_id.as_str())
    );

    // Ephemeral background subagents are not persisted into `agent_records()`.
    let records = app.agent_records();
    assert!(!records.iter().any(|r| r.id == bg_id));
}

#[test]
fn background_shell_runs_captures_output_handles_failure_and_stops_cleanly() {
    let (_ctx, mut app) = test_app();
    let root = app.agents.active.clone();

    // 1. Successful background shell.
    let ok_res = app
        .background_tasks_control(
            &root,
            &json!({
                "action": "bg_bash",
                "command": "echo hello_bg && echo stderr_line >&2",
                "description": "Echo greeting"
            }),
        )
        .expect("spawn ok background shell");
    let ok_id = ok_res["task_id"].as_str().unwrap().to_owned();

    // 2. Failing background shell.
    let fail_res = app
        .background_tasks_control(
            &root,
            &json!({
                "action": "bg_bash",
                "command": "echo fail_msg; exit 7",
                "description": "Failing check"
            }),
        )
        .expect("spawn failing background shell");
    let fail_id = fail_res["task_id"].as_str().unwrap().to_owned();

    // Wait for both quick commands to finish via poll_background_tasks.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        app.poll_background_tasks();
        let both_done = app
            .background_tasks
            .shells
            .iter()
            .filter(|s| s.id == ok_id || s.id == fail_id)
            .all(|s| !s.status.is_running());
        if both_done {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    let ok_out = app
        .background_tasks_control(
            &root,
            &json!({ "action": "bg_tasks", "operation": "output", "task_id": ok_id }),
        )
        .expect("read ok shell output");
    assert_eq!(ok_out["status"], "completed");
    assert_eq!(ok_out["exit_code"], 0);
    assert!(ok_out["output"].as_str().unwrap().contains("hello_bg"));
    assert!(ok_out["output"].as_str().unwrap().contains("stderr_line"));

    let fail_out = app
        .background_tasks_control(
            &root,
            &json!({ "action": "bg_tasks", "operation": "output", "task_id": fail_id }),
        )
        .expect("read fail shell output");
    assert_eq!(fail_out["status"], "failed");
    assert_eq!(fail_out["exit_code"], 7);

    // 3. Long-running background shell stopped explicitly.
    let slow_res = app
        .background_tasks_control(
            &root,
            &json!({
                "action": "bg_bash",
                "command": "sleep 30",
                "description": "Long sleep"
            }),
        )
        .expect("spawn slow shell");
    let slow_id = slow_res["task_id"].as_str().unwrap().to_owned();
    assert!(app.background_tasks.has_running_shells());
    assert_eq!(app.active_background_counts(), (1, 2));

    let stop_res = app
        .background_tasks_control(
            &root,
            &json!({ "action": "bg_tasks", "operation": "stop", "task_id": slow_id }),
        )
        .expect("stop slow shell");
    assert_eq!(stop_res["stopped"], true);
    assert!(!app.background_tasks.has_running_shells());
    assert_eq!(app.active_background_counts(), (0, 3));

    // Plan mode blocks background shells.
    app.agents.nodes.get_mut(&root).unwrap().permission = Some(settings::PermissionMode::Plan);
    assert!(
        app.background_tasks_control(
            &root,
            &json!({ "action": "bg_bash", "command": "echo nope" })
        )
        .is_err()
    );
}

#[test]
fn nested_child_tasks_roll_up_and_ui_renders_list_and_detail_views_cleanly() {
    let (ctx, mut app) = test_app();
    let root = app.agents.active.clone();

    let bg_res = app
        .agent_control(
            &root,
            &json!({
                "action": "spawn_background",
                "name": "Build 3 real in-game bar layouts",
                "task": "Build three responsive control-group bar layouts and verify them with headless renders.",
                "model": "claude-sonnet-4-6"
            }),
        )
        .unwrap();
    let bg_id = bg_res["agent_id"].as_str().unwrap().to_owned();

    // Populate tool calls & assistant reply on the parked background subagent.
    if let Some(chat) = app.agents.parked.get_mut(&bg_id) {
        chat.state.model_name = "Claude Sonnet 4.6".into();
        chat.state.messages.push(protocol::Message {
            role: "assistant".into(),
            text: "Waiting for the test run to finish.".into(),
            tools: vec![
                protocol::Tool {
                    id: "t1".into(),
                    name: "bash".into(),
                    command: "cargo test".into(),
                    finished: true,
                    duration_ms: Some(1500),
                    ..Default::default()
                },
                protocol::Tool {
                    id: "t2".into(),
                    name: "read".into(),
                    path: "src/app.rs".into(),
                    finished: true,
                    duration_ms: Some(12),
                    ..Default::default()
                },
                protocol::Tool {
                    id: "t3".into(),
                    name: "write".into(),
                    path: "src/new_layout.rs".into(),
                    finished: true,
                    failed: true,
                    duration_ms: Some(25),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
    }

    // Attach two child background shells owned by the background subagent (1 completed, 1 failed).
    let now = SystemTime::now();
    app.background_tasks.shells.push(ShellTask {
        id: "bg-shell-10".into(),
        owner: bg_id.clone(),
        root: root.clone(),
        workflow_id: None,
        title: "Run unit tests".into(),
        command: "cargo test".into(),
        cwd: app.prefs.cwd.clone(),
        started: now - Duration::from_secs(12),
        finished: Some(now),
        status: ShellStatus::Completed,
        output: "all passed".into(),
        truncated: false,
        error: None,
        cancel: Arc::new(AtomicBool::new(false)),
        rx: None,
        notified: true,
    });
    app.background_tasks.shells.push(ShellTask {
        id: "bg-shell-11".into(),
        owner: bg_id.clone(),
        root: root.clone(),
        workflow_id: None,
        title: "Render layout preview".into(),
        command: "godot --headless".into(),
        cwd: app.prefs.cwd.clone(),
        started: now - Duration::from_secs(5),
        finished: Some(now),
        status: ShellStatus::Failed(Some(1)),
        output: "shader error".into(),
        truncated: false,
        error: None,
        cancel: Arc::new(AtomicBool::new(false)),
        rx: None,
        notified: true,
    });

    assert_eq!(app.background_child_counts(&bg_id), (2, 0, 1));
    let summary = summarize_agent_tools(&app.agents.parked[&bg_id].state);
    assert_eq!(
        summary.line,
        "Ran 1 command, read 1 file, created a file, 1 failed"
    );

    // 1. Render dedicated Background tasks pane (list view) in dark & light themes + narrow width.
    app.background_tasks.open_list();
    for (width, dark) in [(1100.0, true), (640.0, false)] {
        ctx.set_visuals(if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 800.0),
                )),
                ..Default::default()
            },
            |ctx| {
                app.background_tasks_panel(ctx);
            },
        );
        let mut texts = Vec::new();
        for s in &out.shapes {
            collect_texts(&s.shape, &mut texts);
        }
        assert!(texts.iter().any(|t| t == "Background tasks"));
        assert!(
            texts
                .iter()
                .any(|t| t.contains("Build 3 real in-game bar layouts"))
        );
        assert!(texts.iter().any(|t| t == "2 tasks"));
        assert!(texts.iter().any(|t| t.contains("1 failed")));
    }

    // 2. Render Detail View for the background subagent.
    app.background_tasks.open_agent(bg_id.clone());
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 800.0),
            )),
            ..Default::default()
        },
        |ctx| {
            app.background_tasks_panel(ctx);
        },
    );
    let mut texts = Vec::new();
    for s in &out.shapes {
        collect_texts(&s.shape, &mut texts);
    }
    assert!(texts.iter().any(|t| t == "Model Claude Sonnet 4.6"));
    assert!(
        texts
            .iter()
            .any(|t| t.contains("Ran 1 command, read 1 file"))
    );
    assert!(
        texts
            .iter()
            .any(|t| t == "Waiting for the test run to finish.")
    );

    // 3. Mark agent settled & verify Clear finished cleans up finished items without touching running ones.
    if let Some(node) = app.agents.nodes.get_mut(&bg_id) {
        node.settled = true;
        node.reported = true;
        node.needs_report = false;
        node.status = "Idle".into();
        node.finished_at = Some(now);
    }
    assert_eq!(app.active_background_counts(), (0, 1));
    app.clear_finished_background_tasks();
    assert_eq!(app.active_background_counts(), (0, 0));
    assert!(!app.agents.nodes.contains_key(&bg_id));
    assert!(app.background_tasks.shells.is_empty());
    assert!(app.background_tasks.selected_agent.is_none());
}

#[test]
fn dynamic_workflow_fans_out_phases_tracks_tokens_surfaces_errors_and_renders_transcript_and_pane_ui()
 {
    let (ctx, mut app) = test_app();
    let root = app.agents.active.clone();

    // Respects Settings → General Dynamic workflows toggle.
    app.prefs.dynamic_workflows_disabled = true;
    assert!(
        app.agent_control(
            &root,
            &json!({
                "action": "dynamic_workflow_start",
                "objective": "Audit and refactor",
                "phases": [{ "title": "Explore", "agents": [{ "label": "Auth", "task": "Read auth" }] }]
            }),
        )
        .is_err()
    );
    app.prefs.dynamic_workflows_disabled = false;

    // Launch a 2-phase workflow with 4 total agents (triggers budget notice).
    let started = app
        .agent_control(
            &root,
            &json!({
                "action": "dynamic_workflow_start",
                "name": "Audit & refactor auth",
                "objective": "Audit and refactor authentication across modules.",
                "phases": [
                    {
                        "title": "Explore",
                        "agents": [
                            { "label": "Inspect middleware", "task": "Inspect middleware", "access": "read-only" },
                            { "label": "Inspect token store", "task": "Inspect token store", "access": "read-only" },
                            { "label": "Inspect tests", "task": "Inspect tests", "access": "read-only" }
                        ]
                    },
                    {
                        "title": "Implement",
                        "agents": [
                            { "label": "Apply refactor", "task": "Implement refactor", "access": "workspace" }
                        ]
                    }
                ]
            }),
        )
        .expect("start dynamic workflow");
    let wf_id = started["workflow_id"].as_str().unwrap().to_owned();
    assert_eq!(started["mode"], "dynamic");
    assert_eq!(started["phases"], 2);
    assert_eq!(started["agents"], 4);

    // Phase 0 launched 3 parallel workflow-owned background agents (none in sidebar, none duplicated as top-level background agents).
    assert_eq!(app.active_background_counts(), (1, 0));
    let wf = &app.background_tasks.workflows[0];
    assert!(wf.phases[0].launched);
    assert!(!wf.phases[1].launched);
    assert!(wf.show_budget_warning());
    let a0 = wf.phases[0].agents[0].agent_id.clone().unwrap();
    let a1 = wf.phases[0].agents[1].agent_id.clone().unwrap();
    let a2 = wf.phases[0].agents[2].agent_id.clone().unwrap();
    assert_eq!(
        app.agents.nodes[&a0].workflow_id.as_deref(),
        Some(wf_id.as_str())
    );

    // Simulate token usage + 2 completed + 1 failed in Phase 0.
    if let Some(chat) = app.agents.parked.get_mut(&a0) {
        chat.state.messages.push(protocol::Message {
            role: "assistant".into(),
            text: "Middleware checked.".into(),
            prompt_usage: protocol::PromptUsage {
                input: 8000.0,
                cache_read: 6200.0,
                cache_write: 0.0,
            },
            ..Default::default()
        });
    }
    for (id, result) in [(&a0, "Middleware ok"), (&a1, "Token store ok")] {
        let n = app.agents.nodes.get_mut(id).unwrap();
        n.settled = true;
        n.reported = true;
        n.needs_report = false;
        n.status = "Idle".into();
        n.result = Some(result.into());
        n.finished_at = Some(SystemTime::now());
    }
    {
        let n = app.agents.nodes.get_mut(&a2).unwrap();
        n.settled = true;
        n.reported = true;
        n.needs_report = false;
        n.failure = Some("fixture missing".into());
        n.status = "Failed: fixture missing".into();
        n.finished_at = Some(SystemTime::now());
    }

    // Polling advances to Phase 1 (because Phase 0 had successes alongside 1 failure) and syncs workflow cards.
    app.poll_background_tasks();
    let wf = &app.background_tasks.workflows[0];
    assert_eq!(wf.current_phase, 1);
    assert!(
        wf.phases[1].launched,
        "Phase 1 must launch once Phase 0 settles"
    );
    assert_eq!(wf.phases[0].agents[0].tokens, 14200);
    assert_eq!(
        wf.phases[0].agents[2].state,
        crate::inline_visual::WorkflowCellState::Error
    );
    assert!(app.transcript.visuals.workflow_cards.contains_key(&wf_id));

    // Render Background tasks pane with Phase 0 collapsed (surfaces error row) and Phase 1 open.
    app.background_tasks
        .expanded_workflow_phases
        .remove(&(wf_id.clone(), 0));
    app.background_tasks.open_list();
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 800.0),
            )),
            ..Default::default()
        },
        |ctx| {
            app.background_tasks_panel(ctx);
        },
    );
    let mut texts = Vec::new();
    for s in &out.shapes {
        collect_texts(&s.shape, &mut texts);
    }
    assert!(texts.iter().any(|t| t == "Audit & refactor auth"));
    assert!(texts.iter().any(|t| t == "Explore"));
    assert!(texts.iter().any(|t| t == "Implement"));
    assert!(
        texts
            .iter()
            .any(|t| t.contains("Inspect tests: fixture missing")),
        "collapsed phase with error should surface failed agent inline: {texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|t| t.contains("Dynamic workflows can use your usage limit quickly")),
        "budget warning should render for >=4 agents: {texts:?}"
    );

    // Inline transcript workflow card renders cleanly and emits OpenWorkflow on click.
    let mut tool = protocol::Tool {
        id: "call_wf_1".into(),
        name: "pui_workflow".into(),
        workflow_id: Some(wf_id.clone()),
        workflow_title: Some("Audit & refactor auth".into()),
        finished: true,
        ..Default::default()
    };
    tool.set_workflow_result(&json!({ "details": { "workflow": { "workflow_id": wf_id } } }));
    app.state.messages = vec![protocol::Message {
        role: "assistant".into(),
        tools: vec![tool],
        ..Default::default()
    }];
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.transcript.show(
                    ui,
                    &app.state.messages,
                    "claude-opus-5-5",
                    "medium",
                    false,
                    "session-1",
                );
            });
        },
    );
    let mut tr_texts = Vec::new();
    for s in &out.shapes {
        collect_texts(&s.shape, &mut tr_texts);
    }
    assert!(
        tr_texts.iter().any(|t| t == "Audit & refactor auth"),
        "inline transcript workflow card should show title: {tr_texts:?}"
    );
    assert!(
        tr_texts.iter().any(|t| t.contains("4 agents")),
        "inline transcript workflow card should show agent count: {tr_texts:?}"
    );

    // Stopping the dynamic workflow stops Phase 1's agent and marks the workflow Stopped.
    let status = app
        .dynamic_workflow_control(
            &root,
            "rpc-stop",
            &json!({ "action": "workflow_control", "workflow_id": wf_id, "operation": "cancel" }),
        )
        .unwrap()
        .unwrap();
    assert_eq!(status["status"], "stopped");
    assert_eq!(app.active_background_counts(), (0, 1));
    app.clear_finished_background_tasks();
    assert_eq!(app.active_background_counts(), (0, 0));
    assert!(app.background_tasks.workflows.is_empty());
}

#[test]
fn detail_view_renders_markdown_stats_footer_warning_callout_and_fallbacks() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let (ctx, mut app) = test_app();
        mode.apply(&ctx);
        let root = app.agents.active.clone();
        let now = SystemTime::now();

        // 1. Completed background agent with Markdown, failed tool warning, tokens, and duration.
        let ok_id = "agent-md-detail".to_owned();
        app.agents.nodes.insert(
            ok_id.clone(),
            agents::Node {
                id: ok_id.clone(),
                parent: Some(root.clone()),
                name: "Audit layout insets".into(),
                cwd: app.prefs.cwd.clone(),
                path: None,
                access: "read-only".into(),
                permission: Some(settings::PermissionMode::Plan),
                status: "Idle".into(),
                last_activity: Some(now),
                created: Some(now - Duration::from_secs(34)),
                children: Vec::new(),
                stopped: false,
                reported: true,
                needs_report: false,
                settled: true,
                launch: None,
                failure: None,
                result: None,
                report_summary: None,
                task: "Audit mobile layout insets.".into(),
                depends: Vec::new(),
                setup: None,
                worktree: None,
                cloud_host: None,
                cloud_repo: None,
                background: true,
                workflow_id: None,
                model: Some("Claude Sonnet 4.6".into()),
                finished_at: Some(now),
            },
        );
        let mut chat = agents::Chat::default();
        chat.state.model_name = "Claude Sonnet 4.6".into();
        chat.state.messages.push(protocol::Message {
            role: "assistant".into(),
            text: "### Inset Findings\n\n- **Header bar** clears the notch.\n- Verified via `safe_area_check`.".into(),
            prompt_usage: protocol::PromptUsage {
                input: 8_200.0,
                cache_read: 6_000.0,
                cache_write: 0.0,
            },
            tools: vec![
                protocol::Tool {
                    id: "t1".into(),
                    name: "read".into(),
                    path: "scenes/hud.gd".into(),
                    finished: true,
                    duration_ms: Some(15),
                    ..Default::default()
                },
                protocol::Tool {
                    id: "t2".into(),
                    name: "bash".into(),
                    command: "false".into(),
                    finished: true,
                    failed: true,
                    duration_ms: Some(40),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        app.agents.parked.insert(ok_id.clone(), chat);

        app.background_tasks.open_agent(ok_id.clone());
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| {
                app.background_tasks_panel(ctx);
            },
        );
        let mut texts = Vec::new();
        for s in &out.shapes {
            collect_texts(&s.shape, &mut texts);
        }
        assert!(
            texts.iter().any(|t| t == "Inset Findings"),
            "Markdown heading should render without raw ### syntax ({mode:?}): {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("### Inset Findings")),
            "Raw Markdown hashes should not appear ({mode:?}): {texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t.contains("1 tool call failed during this run.")),
            "Failed tool warning callout should appear above result ({mode:?}): {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "2 tool uses · 14k tokens · 34s"),
            "Claude-style stats footer should appear when settled ({mode:?}): {texts:?}"
        );

        // 2. Failed agent with no assistant text -> error callout + failed fallback.
        let fail_id = "agent-empty-fail".to_owned();
        app.agents.nodes.insert(
            fail_id.clone(),
            agents::Node {
                id: fail_id.clone(),
                parent: Some(root.clone()),
                name: "Broken run".into(),
                cwd: app.prefs.cwd.clone(),
                path: None,
                access: "read-only".into(),
                permission: Some(settings::PermissionMode::Plan),
                status: "Failed: provider timeout".into(),
                last_activity: Some(now),
                created: Some(now - Duration::from_secs(12)),
                children: Vec::new(),
                stopped: false,
                reported: true,
                needs_report: false,
                settled: true,
                launch: None,
                failure: Some("provider timeout".into()),
                result: None,
                report_summary: None,
                task: "Inspect crash.".into(),
                depends: Vec::new(),
                setup: None,
                worktree: None,
                cloud_host: None,
                cloud_repo: None,
                background: true,
                workflow_id: None,
                model: None,
                finished_at: Some(now),
            },
        );
        app.agents
            .parked
            .insert(fail_id.clone(), agents::Chat::default());
        app.background_tasks.open_agent(fail_id);
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| {
                app.background_tasks_panel(ctx);
            },
        );
        let mut texts = Vec::new();
        for s in &out.shapes {
            collect_texts(&s.shape, &mut texts);
        }
        assert!(texts.iter().any(|t| t == "provider timeout"));
        assert!(
            texts.iter().any(|t| t == "No result. The agent failed."),
            "Expected failed empty fallback ({mode:?}): {texts:?}"
        );

        // 3. Stopped agent with no assistant text -> stopped fallback.
        let stop_id = "agent-empty-stop".to_owned();
        app.agents.nodes.insert(
            stop_id.clone(),
            agents::Node {
                id: stop_id.clone(),
                parent: Some(root.clone()),
                name: "Cancelled run".into(),
                cwd: app.prefs.cwd.clone(),
                path: None,
                access: "read-only".into(),
                permission: Some(settings::PermissionMode::Plan),
                status: "Stopped".into(),
                last_activity: Some(now),
                created: Some(now - Duration::from_secs(5)),
                children: Vec::new(),
                stopped: true,
                reported: true,
                needs_report: false,
                settled: true,
                launch: None,
                failure: None,
                result: None,
                report_summary: None,
                task: "Run audit.".into(),
                depends: Vec::new(),
                setup: None,
                worktree: None,
                cloud_host: None,
                cloud_repo: None,
                background: true,
                workflow_id: None,
                model: None,
                finished_at: Some(now),
            },
        );
        app.agents
            .parked
            .insert(stop_id.clone(), agents::Chat::default());
        app.background_tasks.open_agent(stop_id);
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| {
                app.background_tasks_panel(ctx);
            },
        );
        let mut texts = Vec::new();
        for s in &out.shapes {
            collect_texts(&s.shape, &mut texts);
        }
        assert!(
            texts
                .iter()
                .any(|t| t == "No result. The agent was stopped."),
            "Expected stopped empty fallback ({mode:?}): {texts:?}"
        );

        // 4. Fresh agent with no activity -> "No activity yet".
        let fresh_id = "agent-empty-fresh".to_owned();
        app.agents.nodes.insert(
            fresh_id.clone(),
            agents::Node {
                id: fresh_id.clone(),
                parent: Some(root.clone()),
                name: "Queued run".into(),
                cwd: app.prefs.cwd.clone(),
                path: None,
                access: "read-only".into(),
                permission: Some(settings::PermissionMode::Plan),
                status: "Queued".into(),
                last_activity: Some(now),
                created: Some(now),
                children: Vec::new(),
                stopped: false,
                reported: false,
                needs_report: true,
                settled: false,
                launch: None,
                failure: None,
                result: None,
                report_summary: None,
                task: "Wait to start.".into(),
                depends: Vec::new(),
                setup: None,
                worktree: None,
                cloud_host: None,
                cloud_repo: None,
                background: true,
                workflow_id: None,
                model: None,
                finished_at: None,
            },
        );
        app.agents
            .parked
            .insert(fresh_id.clone(), agents::Chat::default());
        app.background_tasks.open_agent(fresh_id);
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| {
                app.background_tasks_panel(ctx);
            },
        );
        let mut texts = Vec::new();
        for s in &out.shapes {
            collect_texts(&s.shape, &mut texts);
        }
        assert!(
            texts.iter().any(|t| t == "No activity yet"),
            "Expected empty activity fallback ({mode:?}): {texts:?}"
        );
    }
}

#[test]
fn demo_workflow_includes_completed_background_agent_result_card() {
    let (_ctx, mut app) = test_app();
    app.demo_workflow();
    let snap = app
        .transcript
        .visuals
        .agent_cards
        .get("agent-bg-demo-1")
        .expect("demo_workflow should sync completed background agent card");
    assert!(snap.is_background);
    assert!(!snap.running && !snap.failed && !snap.stopped);
    assert_eq!(snap.tool_uses, 3);
    assert_eq!(snap.tokens, 14_200);
    assert!(
        snap.preview
            .as_deref()
            .is_some_and(|p| p.contains("Safe-Area Audit Summary"))
    );
    assert!(
        app.state.messages.iter().any(|m| {
            m.tools.iter().any(|t| {
                t.name == "pui_background_agent" && t.agent_id.as_deref() == Some("agent-bg-demo-1")
            })
        }),
        "demo_workflow should include an inline pui_background_agent tool card"
    );
}

#[test]
fn open_agent_transcript_action_routes_background_to_detail_view_and_session_to_child_chat() {
    let (_ctx, mut app) = test_app();
    let root = app.agents.active.clone();

    let bg_res = app
        .agent_control(
            &root,
            &json!({
                "action": "spawn_background",
                "name": "Background audit",
                "task": "Inspect files."
            }),
        )
        .expect("spawn background");
    let bg_id = bg_res["agent_id"].as_str().unwrap().to_owned();

    let sess_res = app
        .agent_control(
            &root,
            &json!({
                "action": "spawn",
                "name": "Child session",
                "task": "Work on feature.",
                "access": "read-only"
            }),
        )
        .expect("spawn session");
    let sess_id = sess_res["agent_id"].as_str().unwrap().to_owned();

    app.sync_workflow_cards();
    assert!(
        app.transcript
            .visuals
            .agent_cards
            .get(&bg_id)
            .is_some_and(|s| s.is_background && s.running)
    );
    assert!(
        app.transcript
            .visuals
            .agent_cards
            .get(&sess_id)
            .is_some_and(|s| !s.is_background && s.running)
    );

    // Opening a background agent opens Detail View without changing active chat.
    app.view_agent(&bg_id);
    assert_eq!(app.agents.active, root);
    assert!(app.background_tasks.open);
    assert_eq!(
        app.background_tasks.selected_agent.as_deref(),
        Some(bg_id.as_str())
    );

    // Opening a sidebar child session switches to that child chat.
    app.view_agent(&sess_id);
    assert_eq!(app.agents.active, sess_id);
}
