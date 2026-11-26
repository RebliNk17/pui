use super::*;

fn make_app(ctx: &egui::Context) -> Pui {
    tests::app(ctx)
}

fn write_parent_session(dir: &std::path::Path, cwd: &std::path::Path) -> (PathBuf, String) {
    let orig_id = uuid::Uuid::new_v4().to_string();
    let path = dir.join(format!("parent-{orig_id}.jsonl"));
    let header = json!({
        "type": "session",
        "version": 3,
        "id": orig_id,
        "timestamp": "2026-04-01T00:00:00Z",
        "cwd": cwd,
    });
    let msg1 = json!({
        "type": "message",
        "id": "m1",
        "message": { "role": "user", "content": "Investigate the caching regression" }
    });
    let msg2 = json!({
        "type": "message",
        "id": "m2",
        "parentId": "m1",
        "message": { "role": "assistant", "content": [{"type": "text", "text": "Found stale TTL in cache.rs"}] }
    });
    std::fs::write(&path, format!("{header}\n{msg1}\n{msg2}\n")).unwrap();
    (path, orig_id)
}

#[test]
fn context_fork_copies_parent_jsonl_with_fresh_uuid_and_preserves_messages() {
    let ctx = egui::Context::default();
    let mut app = make_app(&ctx);
    let dir = std::env::temp_dir().join(format!("pui-fork-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let parent = app.agents.active.clone();
    let (parent_path, orig_id) = write_parent_session(&dir, &dir);
    app.prefs.cwd = dir.clone();
    app.state.session_file = Some(parent_path.display().to_string());
    app.agents.nodes.get_mut(&parent).unwrap().cwd = dir.clone();
    app.agents.nodes.get_mut(&parent).unwrap().path = Some(parent_path.clone());

    let res = app
        .agent_control(
            &parent,
            &json!({
                "action": "spawn",
                "name": "Forked reader",
                "task": "Continue from parent history",
                "access": "read-only",
                "context": "fork"
            }),
        )
        .unwrap();
    assert_eq!(res["context"], "fork");
    assert_eq!(res["kind"], "session");
    let child_id = res["agent_id"].as_str().unwrap().to_owned();
    let forked_path = app.agents.nodes[&child_id]
        .path
        .clone()
        .expect("forked child must have a session path");
    assert_ne!(forked_path, parent_path);
    assert!(forked_path.is_file());
    assert_eq!(
        app.agents.parked[&child_id].state.session_file.as_deref(),
        Some(forked_path.to_string_lossy().as_ref())
    );

    let raw = std::fs::read_to_string(&forked_path).unwrap();
    let lines: Vec<Value> = raw
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 3, "header + 2 prior messages preserved");
    assert_eq!(lines[0]["type"], "session");
    assert_ne!(lines[0]["id"].as_str().unwrap(), orig_id);
    assert!(uuid::Uuid::parse_str(lines[0]["id"].as_str().unwrap()).is_ok());
    assert_eq!(PathBuf::from(lines[0]["cwd"].as_str().unwrap()), dir);
    assert_eq!(lines[1]["message"]["role"], "user");
    assert_eq!(
        lines[1]["message"]["content"],
        "Investigate the caching regression"
    );
    assert_eq!(lines[2]["message"]["role"], "assistant");
    assert_eq!(
        lines[2]["message"]["content"][0]["text"],
        "Found stale TTL in cache.rs"
    );

    // update_session_header_cwd relocates cwd cleanly without losing prior messages.
    let new_cwd = dir.join("worktree-child");
    update_session_header_cwd(&forked_path, &new_cwd).unwrap();
    let updated: Vec<Value> = std::fs::read_to_string(&forked_path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(updated.len(), 3);
    assert_eq!(PathBuf::from(updated[0]["cwd"].as_str().unwrap()), new_cwd);
    assert_eq!(updated[1]["id"], "m1");
    assert_eq!(updated[2]["id"], "m2");

    app.shutdown_agents();
    let _ = std::fs::remove_file(&forked_path);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn context_fork_falls_back_explicitly_when_parent_has_no_saved_session() {
    let ctx = egui::Context::default();
    let mut app = make_app(&ctx);
    let parent = app.agents.active.clone();
    app.state.session_file = None;
    app.agents.nodes.get_mut(&parent).unwrap().path = None;

    let res = app
        .agent_control(
            &parent,
            &json!({
                "action": "spawn",
                "name": "Unsaved parent fork",
                "task": "Inspect files",
                "access": "read-only",
                "context": "fork"
            }),
        )
        .unwrap();
    assert_eq!(res["context"], "fresh (parent has no saved session yet)");
    let child_id = res["agent_id"].as_str().unwrap();
    assert!(app.agents.nodes[child_id].path.is_none());
    app.shutdown_agents();
}

#[test]
fn handoff_session_spawns_forked_child_with_handed_off_flag_and_notes_dir() {
    let ctx = egui::Context::default();
    let mut app = make_app(&ctx);
    let dir = std::env::temp_dir().join(format!("pui-handoff-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let parent = app.agents.active.clone();
    let (parent_path, _) = write_parent_session(&dir, &dir);
    app.prefs.cwd = dir.clone();
    app.state.session_file = Some(parent_path.display().to_string());
    app.agents.nodes.get_mut(&parent).unwrap().cwd = dir.clone();
    app.agents.nodes.get_mut(&parent).unwrap().path = Some(parent_path.clone());

    let res = app
        .agent_control(
            &parent,
            &json!({
                "action": "handoff",
                "name": "Finish cache fix",
                "task": "Continue the caching refactor while main chat pivots",
                "access": "read-only"
            }),
        )
        .unwrap();
    assert_eq!(res["handed_off"], true);
    assert_eq!(res["kind"], "session");
    assert_eq!(res["context"], "fork");
    let notes = res["family_notes_dir"].as_str().unwrap();
    assert!(PathBuf::from(notes).is_dir());
    let child_id = res["agent_id"].as_str().unwrap().to_owned();
    let forked = app.agents.nodes[&child_id].path.clone().unwrap();
    assert!(forked.is_file());

    // Starting the forked worker populates Launch::family_notes_dir.
    app.start_waiting_agents();
    let launch = app.agents.nodes[&child_id].launch.as_ref().unwrap();
    assert_eq!(
        launch.family_notes_dir.as_deref(),
        Some(PathBuf::from(notes).as_path())
    );
    assert!(
        launch
            .args
            .iter()
            .any(|arg| arg.contains("Shared family notes folder"))
    );

    app.shutdown_agents();
    let _ = std::fs::remove_file(&forked);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn family_notes_dir_resolves_identically_across_family_and_differs_across_families() {
    let ctx = egui::Context::default();
    let mut app = make_app(&ctx);
    let parent = app.agents.active.clone();
    let child_a = tests::spawn(&mut app, &parent);
    let child_b = tests::spawn(&mut app, &parent);

    // Make child_a writable (and non-Plan) so it can spawn a grandchild.
    {
        let node = app.agents.nodes.get_mut(&child_a).unwrap();
        node.access = "worktree".into();
        node.permission = Some(settings::PermissionMode::Auto);
    }
    let grandchild = tests::spawn(&mut app, &child_a);

    let dir_parent = app.family_notes_dir(&parent).unwrap();
    let dir_a = app.family_notes_dir(&child_a).unwrap();
    let dir_b = app.family_notes_dir(&child_b).unwrap();
    let dir_grand = app.family_notes_dir(&grandchild).unwrap();
    assert_eq!(dir_parent, dir_a);
    assert_eq!(dir_parent, dir_b);
    assert_eq!(dir_parent, dir_grand);
    assert!(dir_parent.is_dir());

    // An unrelated root family has a distinct notes folder.
    let other_root = "agent-9999".to_owned();
    app.agents.nodes.insert(
        other_root.clone(),
        Node {
            id: other_root.clone(),
            parent: None,
            name: "Other family".into(),
            cwd: app.prefs.cwd.clone(),
            path: None,
            access: "workspace".into(),
            permission: Some(settings::PermissionMode::Auto),
            status: "Idle".into(),
            last_activity: None,
            created: None,
            children: Vec::new(),
            stopped: false,
            reported: true,
            needs_report: false,
            settled: true,
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
    let dir_other = app.family_notes_dir(&other_root).unwrap();
    assert_ne!(dir_parent, dir_other);
    app.shutdown_agents();
}

#[test]
fn sibling_messaging_succeeds_in_same_family_blocks_readonly_task_and_rejects_other_family() {
    let ctx = egui::Context::default();
    let mut app = make_app(&ctx);
    let parent = app.agents.active.clone();
    let child_a = tests::spawn(&mut app, &parent);
    let child_b = tests::spawn(&mut app, &parent);
    app.start_waiting_agents();

    // Attach command spy to child_b so we inspect delivered messages.
    let (client_b, _, commands_b) = rpc::Client::stub_with_commands();
    app.agents.parked.get_mut(&child_b).unwrap().client = Some(client_b);

    // Read-only sibling CAN send clarification/blocker/result to sibling!
    let ok = app
        .agent_control(
            &child_a,
            &json!({
                "action": "message",
                "agent_id": child_b,
                "kind": "clarification",
                "message": "Schema uses snake_case keys"
            }),
        )
        .unwrap();
    assert_eq!(ok["accepted_for_delivery"], true);
    let delivered = commands_b
        .recv_timeout(Duration::from_secs(2))
        .expect("sibling should receive message");
    let msg = delivered["message"].as_str().unwrap();
    assert!(msg.contains("Schema uses snake_case keys"), "{msg}");
    assert!(msg.contains("Linked sessions now:"), "{msg}");

    // Read-only worker CANNOT send kind="task" assignments.
    let ro_err = app
        .agent_control(
            &child_a,
            &json!({
                "action": "message",
                "agent_id": child_b,
                "kind": "task",
                "message": "Do this work"
            }),
        )
        .unwrap_err();
    assert!(ro_err.contains("Read-only"), "{ro_err}");

    // Unrelated family session is strictly rejected.
    let outsider = "agent-8888".to_owned();
    app.agents.nodes.insert(
        outsider.clone(),
        Node {
            id: outsider.clone(),
            parent: None,
            name: "Unrelated root".into(),
            cwd: app.prefs.cwd.clone(),
            path: None,
            access: "workspace".into(),
            permission: Some(settings::PermissionMode::Auto),
            status: "Idle".into(),
            last_activity: None,
            created: None,
            children: Vec::new(),
            stopped: false,
            reported: true,
            needs_report: false,
            settled: true,
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
    app.agents.parked.insert(
        outsider.clone(),
        Chat {
            client: Some(rpc::Client::stub()),
            connected: true,
            ..Default::default()
        },
    );
    assert!(
        app.agent_control(
            &child_a,
            &json!({
                "action": "message",
                "agent_id": outsider,
                "kind": "clarification",
                "message": "Cross-family leak"
            })
        )
        .is_err()
    );

    // Full family roster in `pui_agents` includes parent, self, sibling (with relations & notes dir) and excludes outsider.
    let roster = app
        .agent_control(&child_a, &json!({"action": "list"}))
        .unwrap();
    assert!(roster["family_notes_dir"].as_str().is_some());
    let agents = roster["agents"].as_array().unwrap();
    assert_eq!(agents.len(), 3);
    let relation_of = |id: &str| {
        agents
            .iter()
            .find(|a| a["agent_id"].as_str() == Some(id))
            .map(|a| a["relation"].as_str().unwrap().to_owned())
    };
    assert_eq!(relation_of(&child_a).as_deref(), Some("self"));
    assert_eq!(relation_of(&parent).as_deref(), Some("parent"));
    assert_eq!(relation_of(&child_b).as_deref(), Some("sibling"));
    assert!(relation_of(&outsider).is_none());
    app.shutdown_agents();
}

#[test]
fn batched_side_sessions_event_holds_while_sibling_works_and_delivers_once_when_all_settle() {
    let ctx = egui::Context::default();
    let mut app = make_app(&ctx);
    let (parent_client, _, parent_commands) = rpc::Client::stub_with_commands();
    app.client = Some(parent_client);
    let parent = app.agents.active.clone();

    let child_a = app
        .agent_control(
            &parent,
            &json!({"action": "spawn", "name": "Alpha audit", "task": "Audit alpha", "access": "read-only"}),
        )
        .unwrap()["agent_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let child_b = app
        .agent_control(
            &parent,
            &json!({"action": "spawn", "name": "Beta audit", "task": "Audit beta", "access": "read-only"}),
        )
        .unwrap()["agent_id"]
        .as_str()
        .unwrap()
        .to_owned();

    app.start_waiting_agents();
    app.agents.nodes.get_mut(&child_a).unwrap().status = "Working…".into();
    app.agents.nodes.get_mut(&child_b).unwrap().status = "Working…".into();

    // Child A finishes while Child B is still Working… -> report is HELD, parent NOT interrupted!
    app.view_agent(&child_a);
    app.connected = true;
    app.state.pending = false;
    app.state.messages.push(Message {
        role: "assistant".into(),
        text: "Alpha findings verified.".into(),
        ..Default::default()
    });
    app.agent_event(&json!({"type": "agent_settled"}));
    app.agents.nodes.get_mut(&child_a).unwrap().status = "Idle".into();
    app.report_agent_result();

    assert!(
        parent_commands.try_recv().is_err(),
        "parent must not be interrupted while sibling Beta is still working"
    );
    assert_eq!(app.agents.pending_side_reports[&parent].len(), 1);
    assert!(
        app.agents.nodes[&child_a]
            .report_summary
            .as_deref()
            .is_some_and(|s| s.contains("Alpha findings verified."))
    );

    // Child B now settles -> ONE consolidated `<side-sessions-event>` delivered for BOTH!
    app.view_agent(&child_b);
    app.connected = true;
    app.state.pending = false;
    app.state.messages.push(Message {
        role: "assistant".into(),
        text: "Beta findings verified.".into(),
        ..Default::default()
    });
    app.agent_event(&json!({"type": "agent_settled"}));
    app.agents.nodes.get_mut(&child_b).unwrap().status = "Idle".into();
    app.report_agent_result();

    let consolidated = parent_commands
        .recv_timeout(Duration::from_secs(2))
        .expect("consolidated side-sessions-event must deliver once all siblings settle");
    assert!(
        parent_commands.try_recv().is_err(),
        "must deliver exactly ONE consolidated wake"
    );
    let body = consolidated["message"].as_str().unwrap();
    assert!(
        body.contains("Side sessions update · 2 side sessions settled"),
        "{body}"
    );
    assert!(body.contains("<side-sessions-event>"), "{body}");
    assert!(
        body.contains(&format!("[Alpha audit](#agent/{child_a})")),
        "{body}"
    );
    assert!(
        body.contains(&format!("[Beta audit](#agent/{child_b})")),
        "{body}"
    );
    assert!(body.contains("Alpha findings verified."), "{body}");
    assert!(body.contains("Beta findings verified."), "{body}");
    assert!(body.contains("Linked sessions now:"), "{body}");
    assert!(app.agents.pending_side_reports.is_empty());

    // Ack marks BOTH children reported.
    let key = app.agents.mail.keys().next().unwrap().clone();
    app.agent_mail_response(&json!({"id": key, "success": true}));
    assert!(app.agents.nodes[&child_a].reported);
    assert!(app.agents.nodes[&child_b].reported);
    app.shutdown_agents();
}

#[test]
fn explicit_pui_wait_agents_clears_held_side_reports_without_duplicating_event() {
    let ctx = egui::Context::default();
    let mut app = make_app(&ctx);
    let (parent_client, _, parent_commands) = rpc::Client::stub_with_commands();
    app.client = Some(parent_client);
    let parent = app.agents.active.clone();

    let child_a = tests::spawn(&mut app, &parent);
    let child_b = tests::spawn(&mut app, &parent);
    app.start_waiting_agents();
    app.agents.nodes.get_mut(&child_b).unwrap().status = "Working…".into();

    // Child A settles early and is held while Child B works.
    app.view_agent(&child_a);
    app.connected = true;
    app.state.pending = false;
    app.state.messages.push(Message {
        role: "assistant".into(),
        text: "A done".into(),
        ..Default::default()
    });
    app.agent_event(&json!({"type": "agent_settled"}));
    app.report_agent_result();
    assert_eq!(app.agents.pending_side_reports[&parent].len(), 1);

    // Parent now explicitly waits on [child_a, child_b].
    app.view_agent(&parent);
    let wait_req = json!({
        "type": "extension_ui_request",
        "method": "input",
        "title": "Pui agent control",
        "id": "wait-both",
        "placeholder": json!({"action": "wait", "agent_ids": [child_a, child_b], "timeout_ms": 60000}).to_string()
    });
    assert!(app.control_request(&wait_req));

    // Child B settles -> resolved directly via wait response, NO duplicate `<side-sessions-event>`!
    app.view_agent(&child_b);
    app.connected = true;
    app.state.pending = false;
    app.state.messages.push(Message {
        role: "assistant".into(),
        text: "B done".into(),
        ..Default::default()
    });
    app.agent_event(&json!({"type": "agent_settled"}));
    app.report_agent_result();
    app.finish_agent_waits();

    let reply = parent_commands
        .recv_timeout(Duration::from_secs(2))
        .expect("wait response should be sent");
    assert_eq!(reply["type"], "extension_ui_response");
    assert_eq!(reply["id"], "wait-both");
    assert!(
        parent_commands.try_recv().is_err(),
        "must not also send a duplicate <side-sessions-event>"
    );
    assert!(app.agents.pending_side_reports.is_empty());
    app.shutdown_agents();
}

#[test]
fn agent_deep_link_in_process_open_links_switches_session_opens_background_or_sets_notice() {
    let ctx = egui::Context::default();
    let mut app = make_app(&ctx);
    let parent = app.agents.active.clone();
    let sidebar_child = tests::spawn(&mut app, &parent);
    let bg_child = app
        .agent_control(
            &parent,
            &json!({"action": "spawn_background", "name": "BG worker", "task": "Check logs"}),
        )
        .unwrap()["agent_id"]
        .as_str()
        .unwrap()
        .to_owned();

    // 1. Clicking `#agent/<sidebar_child>` switches to that sidebar child session.
    let _ = ctx.run(Default::default(), |ctx| {
        ctx.open_url(egui::OpenUrl::new_tab(format!("#agent/{sidebar_child}")));
        app.process_open_links(ctx);
    });
    assert_eq!(app.agents.active, sidebar_child);
    assert!(app.state.notice.is_none());

    // 2. Clicking `#agent/<bg_child>` opens Background tasks Detail View without switching active chat.
    let _ = ctx.run(Default::default(), |ctx| {
        ctx.open_url(egui::OpenUrl::new_tab(format!("#agent/{bg_child}")));
        app.process_open_links(ctx);
    });
    assert_eq!(app.agents.active, sidebar_child);
    assert!(app.background_tasks.open);
    assert_eq!(
        app.background_tasks.selected_agent.as_deref(),
        Some(bg_child.as_str())
    );

    // 3. Clicking a missing session sets a clear notice.
    let _ = ctx.run(Default::default(), |ctx| {
        ctx.open_url(egui::OpenUrl::new_tab("#agent/agent-99999"));
        app.process_open_links(ctx);
    });
    assert_eq!(
        app.state.notice.as_deref(),
        Some("Session 'agent-99999' is no longer open.")
    );
    app.shutdown_agents();
}
