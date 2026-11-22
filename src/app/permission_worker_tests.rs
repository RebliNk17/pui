use super::*;

fn fixture() -> (Pui, String, String, String) {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(egui::Context::default()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.sessions.clear();
    app.agents = Default::default();
    let home = std::env::temp_dir().join(format!("pui-permission-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    for (id, parent, access) in [
        ("agent-1", None, "workspace"),
        ("agent-2", Some("agent-1"), "read-only"),
        ("agent-3", Some("agent-1"), "worktree"),
    ] {
        let path = home.join(format!("{id}.jsonl"));
        std::fs::write(&path, "{\"type\":\"session\",\"cwd\":\"/fixture\"}\n").unwrap();
        app.prefs.agent_tree.push(agents::AgentRecord {
            id: id.into(),
            parent: parent.map(str::to_owned),
            name: id.into(),
            cwd: home.clone(),
            path,
            access: access.into(),
            permission: None,
            worktree: None,
        });
    }
    app.restore_agents();
    app.view_agent("agent-2");
    app.loading_chat = false;
    app.connected = true;
    (app, "agent-1".into(), "agent-2".into(), "agent-3".into())
}
fn render(
    app: &mut Pui,
    events: Vec<egui::Event>,
    dark: bool,
    size: egui::Vec2,
    zoom: f32,
) -> egui::FullOutput {
    let ctx = app.ctx.clone();
    ctx.set_visuals(if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    });
    ctx.set_zoom_factor(zoom);
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.permission_button(ui);
                app.permission_confirmation(ui);
            });
        },
    )
}
fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|s| collect(&s.shape, &mut out));
    out
}
#[test]
fn read_only_never_displays_bypass_and_upgrades_require_explicit_confirmation() {
    for dark in [false, true] {
        for (size, zoom) in [
            (egui::vec2(380.0, 700.0), 1.4),
            (egui::vec2(1000.0, 800.0), 1.0),
        ] {
            let (mut app, parent, child, sibling) = fixture();
            app.prefs.settings.permission = PermissionMode::Bypass;
            let output = render(&mut app, vec![], dark, size, zoom);
            let text = texts(&output.shapes);
            assert!(text.iter().any(|t| t == "Read-only"));
            assert!(!text.iter().any(|t| t == "Bypass"));
            app.choose_permission(PermissionMode::Manual);
            assert!(app.worker_upgrade_pending.is_some());
            assert_eq!(app.agents.nodes[&child].access, "read-only");
            assert!(app.active_permission() == PermissionMode::Plan);
            assert!(app.agent_settings(&parent).permission == PermissionMode::Bypass);
            assert!(app.agent_settings(&sibling).permission == PermissionMode::Auto);
            render(&mut app, vec![], dark, size, zoom);
            render(
                &mut app,
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                dark,
                size,
                zoom,
            );
            assert!(app.worker_upgrade_pending.is_none());
            assert!(app.agents.permission_changes.is_empty());
            assert_eq!(app.agents.nodes[&child].access, "read-only");
            app.shutdown_agents();
        }
    }
}
#[test]
fn worker_modes_are_independent_and_survive_restore_and_reconnect() {
    let (mut app, parent, child, sibling) = fixture();
    app.prefs.settings.permission = PermissionMode::Bypass;
    app.view_agent(&sibling);
    app.choose_permission(PermissionMode::Manual);
    assert!(app.active_permission() == PermissionMode::Manual);
    // The last selection becomes the default for new chats; chats that were
    // inheriting the previous default keep it explicitly.
    assert!(app.prefs.settings.permission == PermissionMode::Manual);
    assert!(app.agent_settings(&child).permission == PermissionMode::Plan);
    assert!(app.agent_settings(&parent).permission == PermissionMode::Bypass);
    let records = app.agent_records();
    assert!(
        records.iter().find(|n| n.id == sibling).unwrap().permission
            == Some(PermissionMode::Manual)
    );
    app.shutdown_agents();
    app.agents = Default::default();
    app.prefs.agent_tree = records;
    app.restore_agents();
    app.view_agent(&sibling);
    assert!(app.active_permission() == PermissionMode::Manual);
    app.view_agent(&child);
    assert!(app.readonly_worker());
    assert!(app.active_permission() == PermissionMode::Plan);
    app.shutdown_agents();
}
#[test]
fn upgrade_keeps_session_and_draft_and_never_replays_a_prompt() {
    let (mut app, parent, child, sibling) = fixture();
    app.draft = "my pending request".into();
    let session = app.state.session_file.clone();
    let transcript = "{\"type\":\"session\",\"id\":\"kept-id\",\"cwd\":\"/old\"}\n{\"type\":\"message\",\"id\":\"kept-message\"}\n";
    std::fs::write(session.as_ref().unwrap(), transcript).unwrap();
    // Reusing a retained worktree avoids creating real Git worktrees in this fixture.
    app.agents.nodes.get_mut(&child).unwrap().worktree = Some(app.prefs.cwd.clone());
    app.begin_worker_upgrade(PermissionMode::Manual).unwrap();
    assert!(app.loading_chat);
    assert_eq!(app.agents.nodes[&child].access, "read-only");
    let result = app.agents.permission_changes[&child]
        .result
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let (tx, rx) = mpsc::channel();
    tx.send(result).unwrap();
    app.agents
        .permission_changes
        .get_mut(&child)
        .unwrap()
        .result = rx;
    app.poll_permission_changes();
    assert_eq!(app.agents.nodes[&child].access, "worktree");
    assert!(app.active_permission() == PermissionMode::Manual);
    assert_eq!(app.state.session_file, session);
    let saved = std::fs::read_to_string(session.unwrap()).unwrap();
    let (header, body) = saved.split_once('\n').unwrap();
    let header: Value = serde_json::from_str(header).unwrap();
    assert_eq!(header["cwd"], json!(app.prefs.cwd));
    assert_eq!(header["id"], "kept-id");
    assert_eq!(body, "{\"type\":\"message\",\"id\":\"kept-message\"}\n");
    assert_eq!(app.draft, "my pending request");
    assert!(!app.state.pending);
    assert!(app.agent_settings(&parent).permission == PermissionMode::Auto);
    assert!(app.agent_settings(&sibling).permission == PermissionMode::Auto);
    app.shutdown_agents();
}
#[test]
fn upgrades_are_disabled_when_busy_and_scope_cannot_follow_navigation() {
    let (mut app, parent, child, _) = fixture();
    app.state.busy = true;
    assert!(app.begin_worker_upgrade(PermissionMode::Bypass).is_err());
    assert_eq!(app.agents.nodes[&child].access, "read-only");
    assert!(app.agents.permission_changes.is_empty());
    app.state.busy = false;
    app.choose_permission(PermissionMode::Bypass);
    app.view_agent(&parent);
    render(&mut app, vec![], true, egui::vec2(700.0, 800.0), 1.0);
    assert!(app.worker_upgrade_pending.is_none());
    assert_eq!(app.agents.nodes[&child].access, "read-only");
    app.shutdown_agents();
}
#[test]
fn accepted_mail_blocks_upgrade_until_the_worker_observes_the_turn() {
    let (mut app, _, child, _) = fixture();
    app.agents.nodes.get_mut(&child).unwrap().stopped = false;
    app.send_agent_message(&child, "New bounded assignment")
        .unwrap();
    assert!(app.state.pending);
    assert!(app.agent_mail_pending(&child));
    assert!(app.begin_worker_upgrade(PermissionMode::Manual).is_err());
    assert_eq!(app.agents.nodes[&child].access, "read-only");
    app.shutdown_agents();
}
#[test]
fn shutdown_waits_for_access_workers_before_releasing_ownership() {
    let (mut app, _, _, _) = fixture();
    let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done = finished.clone();
    let (tx, rx) = mpsc::channel();
    app.agents
        .permission_workers
        .push(std::thread::spawn(move || {
            rx.recv().unwrap();
            done.store(true, std::sync::atomic::Ordering::SeqCst);
        }));
    tx.send(()).unwrap();
    app.shutdown_agents();
    assert!(finished.load(std::sync::atomic::Ordering::SeqCst));
    assert!(app.agents.permission_workers.is_empty());
}
#[test]
fn failed_upgrade_keeps_readonly_access_and_original_workspace() {
    let (mut app, _, child, _) = fixture();
    let cwd = app.prefs.cwd.clone();
    let (tx, result) = mpsc::channel();
    tx.send(Err("fixture setup failure".into())).unwrap();
    app.client = None;
    app.agents.permission_changes.insert(
        child.clone(),
        Change {
            result,
            mode: PermissionMode::Bypass,
        },
    );
    app.poll_permission_changes();
    assert_eq!(app.agents.nodes[&child].access, "read-only");
    assert_eq!(app.prefs.cwd, cwd);
    assert!(app.active_permission() == PermissionMode::Plan);
    assert!(
        app.state
            .error
            .as_ref()
            .unwrap()
            .contains("fixture setup failure")
    );
    app.shutdown_agents();
}
#[test]
fn bypass_is_never_remembered_as_the_default_for_new_chats_or_workers() {
    let (mut app, parent, _child, sibling) = fixture();
    app.view_agent(&sibling);
    app.choose_permission(PermissionMode::Manual);
    assert!(app.prefs.settings.permission == PermissionMode::Manual);
    app.prefs.skip_bypass_confirmation = true;
    app.choose_permission(PermissionMode::Bypass);
    assert!(app.active_permission() == PermissionMode::Bypass);
    assert!(
        app.prefs.settings.permission == PermissionMode::Manual,
        "Bypass must not become the default"
    );
    assert!(app.agent_settings(&parent).permission == PermissionMode::Auto);
    app.shutdown_agents();
}
#[test]
fn saved_bypass_default_is_reset_to_auto_while_existing_chats_keep_bypass() {
    let mut prefs = Preferences::default();
    prefs.settings.permission = PermissionMode::Bypass;
    for (id, access, permission) in [
        ("agent-1", "workspace", None),
        ("agent-2", "read-only", None),
        ("agent-3", "worktree", Some(PermissionMode::Manual)),
    ] {
        prefs.agent_tree.push(agents::AgentRecord {
            id: id.into(),
            parent: None,
            name: id.into(),
            cwd: PathBuf::new(),
            path: PathBuf::new(),
            access: access.into(),
            permission,
            worktree: None,
        });
    }
    prefs.normalize_default_permission();
    assert!(prefs.settings.permission == PermissionMode::Auto);
    assert!(prefs.agent_tree[0].permission == Some(PermissionMode::Bypass));
    assert!(prefs.agent_tree[1].permission.is_none(), "read-only stays read-only");
    assert!(prefs.agent_tree[2].permission == Some(PermissionMode::Manual));
}
