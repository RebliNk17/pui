use super::*;
use tests::{app, home, session};

struct Tree {
    home: PathBuf,
    paths: Vec<PathBuf>,
    ids: Vec<String>,
}
fn tree(app: &mut Pui) -> Tree {
    let home = home();
    let paths: Vec<_> = ["root", "child", "grandchild", "sibling", "unrelated"]
        .iter()
        .map(|name| session(&home, name))
        .collect();
    let ids: Vec<_> = (1..=5).map(|id| format!("agent-{id}")).collect();
    app.prefs.agent_tree = paths
        .iter()
        .enumerate()
        .map(|(i, path)| {
            serde_json::from_value(json!({"id":ids[i], "parent":match i {
            1 | 3 => Some(ids[0].clone()), 2 => Some(ids[1].clone()), _ => None
        }, "name":format!("fixture-{i}"), "cwd":home, "path":path,
        "access":"workspace", "worktree":home.join("kept-worktree")}))
            .unwrap()
        })
        .collect();
    app.restore_agents();
    app.sessions = paths
        .iter()
        .enumerate()
        .map(|(i, path)| Session {
            path: path.clone(),
            cwd: home.clone(),
            project: home.clone(),
            title: format!("fixture-{i}"),
            created: None,
            modified: std::time::SystemTime::now(),
        })
        .collect();
    for path in &paths {
        app.prefs.chats.insert(path.clone(), Default::default());
    }
    app.prefs.cwd = home.clone();
    Tree { home, paths, ids }
}
fn complete(app: &mut Pui) {
    let (target, rx) = app.deletion.jobs.remove(0);
    let result = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (tx, rx) = mpsc::channel();
    app.deletion.jobs.push((target, rx));
    tx.send(result).unwrap();
    app.poll_deletions();
}

#[test]
fn recursive_delete_removes_nested_transcripts_and_saved_records_not_projects_or_workflows() {
    let mut app = app();
    let tree = tree(&mut app);
    let project = tree.home.join("project.txt");
    let worktree = tree.home.join("kept-worktree");
    std::fs::write(&project, "keep").unwrap();
    std::fs::create_dir_all(&worktree).unwrap();
    // Link workflow history to a grandchild, but keep its separate run transcript.
    app.state.session_file = Some(tree.paths[2].display().to_string());
    app.new_task();
    app.agents.nodes.get_mut(&app.agents.active).unwrap().status = "Idle".into();
    app.park_tree();
    app.tasks.open = false;
    app.pending_session = None;
    app.loading_chat = false;
    let workflow = app.tasks.records[0].id.clone();
    app.prefs.last_session = Some(tree.paths[2].clone());
    app.begin_delete(Target::Chat(tree.paths[0].clone()))
        .unwrap();
    complete(&mut app);
    for i in 0..4 {
        assert!(!tree.paths[i].exists());
        assert!(!app.agents.nodes.contains_key(&tree.ids[i]));
        assert!(!app.agents.parked.contains_key(&tree.ids[i]));
        assert!(!app.prefs.agent_tree.iter().any(|n| n.id == tree.ids[i]));
        assert!(!app.prefs.chats.contains_key(&tree.paths[i]));
    }
    assert!(tree.paths[4].exists());
    assert!(app.agents.nodes.contains_key(&tree.ids[4]));
    assert!(project.exists() && worktree.exists());
    assert!(app.prefs.last_session.is_none());
    assert_eq!(app.tasks.records[0].id, workflow);
    assert!(app.tasks.records[0].parent_session.is_none());
}

#[test]
fn deleting_child_keeps_parent_sibling_and_unrelated_chat() {
    let mut app = app();
    let tree = tree(&mut app);
    app.begin_delete(Target::Chat(tree.paths[1].clone()))
        .unwrap();
    complete(&mut app);
    for i in [0, 3, 4] {
        assert!(tree.paths[i].exists());
        assert!(app.agents.nodes.contains_key(&tree.ids[i]));
    }
    for i in [1, 2] {
        assert!(!tree.paths[i].exists());
        assert!(!app.agents.nodes.contains_key(&tree.ids[i]));
    }
    assert_eq!(
        app.agents.nodes[&tree.ids[0]].children,
        vec![tree.ids[3].clone()]
    );
    assert_eq!(
        app.agents.nodes[&tree.ids[3]].parent.as_ref(),
        Some(&tree.ids[0])
    );
}

#[test]
fn busy_or_queued_grandchild_refuses_entire_tree_before_changes() {
    for queued in [false, true] {
        let mut app = app();
        let tree = tree(&mut app);
        if queued {
            let node = app.agents.nodes.get_mut(&tree.ids[2]).unwrap();
            node.stopped = false;
            node.status = "Queued".into();
        } else {
            app.agents.parked.get_mut(&tree.ids[2]).unwrap().state.busy = true;
        }
        assert!(
            app.begin_delete(Target::Chat(tree.paths[0].clone()))
                .is_err()
        );
        assert!(tree.paths.iter().all(|p| p.exists()));
        assert_eq!(app.agents.nodes.len(), 5);
        assert!(app.deletion.jobs.is_empty());
    }
}

#[test]
fn invalid_grandchild_or_lock_failure_leaves_every_transcript_and_client() {
    let mut app = app();
    let tree = tree(&mut app);
    std::fs::write(&tree.paths[2], "{\"type\":\"message\"}\n").unwrap();
    app.agents.parked.get_mut(&tree.ids[0]).unwrap().client = Some(rpc::Client::stub());
    assert!(
        app.begin_delete(Target::Chat(tree.paths[0].clone()))
            .is_err()
    );
    assert!(app.agents.parked[&tree.ids[0]].client.is_some());
    assert!(tree.paths.iter().all(|p| p.exists()));
    let mut lock_attempts = 0;
    assert!(
        chat_deletion_locks(&app.chat_delete_scope(&tree.paths[0]).paths, |_| {
            lock_attempts += 1;
            Ok(())
        })
        .is_err()
    );
    assert_eq!(
        lock_attempts, 0,
        "Validate every transcript before any lock or mutation"
    );
    session(&tree.home, "grandchild");
    let scope = app.chat_delete_scope(&tree.paths[0]);
    let mut attempts = 0;
    let result = chat_deletion_locks(&scope.paths, |_| {
        attempts += 1;
        if attempts == 2 {
            Err("another window owns descendant".into())
        } else {
            Ok(())
        }
    });
    assert!(result.is_err());
    assert_eq!(attempts, 2);
    assert!(tree.paths.iter().all(|p| p.exists()));
    assert!(app.deletion.jobs.is_empty());
    app.shutdown_agents();
}

#[test]
fn active_grandchild_navigates_out_and_snapshot_blocks_reopening_whole_tree() {
    let mut app = app();
    let tree = tree(&mut app);
    let active = app.agents.parked.remove(&tree.ids[2]).unwrap();
    app.agents.active = tree.ids[2].clone();
    app.put_chat(active);
    app.client = Some(rpc::Client::stub());
    app.begin_delete(Target::Chat(tree.paths[0].clone()))
        .unwrap();
    let unrelated = tree.paths[4].to_str();
    assert_eq!(app.state.session_file.as_deref(), unrelated);
    for i in 0..4 {
        assert!(app.deletion.deleting_chat(&tree.paths[i]));
        app.view_agent(&tree.ids[i]);
        app.open_chat(
            app.sessions
                .iter()
                .find(|s| s.path == tree.paths[i])
                .unwrap()
                .clone(),
        );
        assert_eq!(app.state.session_file.as_deref(), unrelated);
    }
    // Completion must use the original set, not a tree rebuilt while work runs.
    app.agents.nodes.get_mut(&tree.ids[2]).unwrap().parent = None;
    app.draft = "retain unrelated draft".into();
    app.poll_other_agents();
    complete(&mut app);
    assert!(!app.agents.nodes.contains_key(&tree.ids[2]));
    assert_eq!(app.state.session_file.as_deref(), unrelated);
    assert_eq!(app.draft, "retain unrelated draft");
    app.shutdown_agents();
}

#[test]
fn persisted_only_descendant_is_included_and_unrelated_persisted_record_survives() {
    let mut app = app();
    let tree = tree(&mut app);
    app.agents.nodes.remove(&tree.ids[2]);
    app.agents.parked.remove(&tree.ids[2]);
    let extra = session(&tree.home, "persisted-only-unrelated");
    let record = serde_json::from_value(json!({"id":"agent-999", "name":"saved", "parent":null,
        "cwd":tree.home, "path":extra, "access":"workspace", "worktree":null}))
    .unwrap();
    app.prefs.agent_tree.push(record);
    app.begin_delete(Target::Chat(tree.paths[0].clone()))
        .unwrap();
    complete(&mut app);
    assert!(!tree.paths[2].exists());
    assert!(!app.prefs.agent_tree.iter().any(|n| n.id == tree.ids[2]));
    assert!(app.prefs.agent_tree.iter().any(|n| n.id == "agent-999"));
    let persisted: super::super::Preferences =
        serde_json::from_value(serde_json::to_value(&app.prefs).unwrap()).unwrap();
    assert!(!persisted.agent_tree.iter().any(|n| n.id == tree.ids[2]));
    assert!(persisted.agent_tree.iter().any(|n| n.id == "agent-999"));
    assert!(extra.exists());
}

#[test]
fn partial_filesystem_failure_cleans_only_successes_and_keeps_parent_retryable() {
    let mut app = app();
    let tree = tree(&mut app);
    let scope = app.chat_delete_scope(&tree.paths[0]);
    let blocked = scope.paths[1].clone();
    let first = scope.paths[0].clone();
    let mut deleted = Vec::new();
    let error = delete_chat_paths(&scope.paths, |path| {
        deleted.push(path.to_path_buf());
        if deleted.len() == 1 {
            std::fs::write(&blocked, "invalidated after preflight").unwrap();
        }
    })
    .unwrap_err();
    let (progress, completed) = mpsc::channel();
    for path in deleted {
        progress.send(path).unwrap();
    }
    app.deletion.chats.insert(
        sync::identity(&tree.paths[0]),
        ChatScope {
            completed: Some(completed),
            ..scope
        },
    );
    let (tx, rx) = mpsc::channel();
    app.deletion
        .jobs
        .push((Target::Chat(tree.paths[0].clone()), rx));
    tx.send(Err(error)).unwrap();
    app.poll_deletions();
    assert!(!first.exists());
    assert!(
        !app.agents
            .nodes
            .values()
            .any(|n| n.path.as_ref() == Some(&first))
    );
    assert!(blocked.exists() && tree.paths[0].exists());
    assert!(app.agents.nodes.contains_key(&tree.ids[0]));
    assert!(app.state.error.as_ref().unwrap().contains("Deleted 1 of"));
    assert!(app.notifications.sent.is_empty());
    std::fs::write(&blocked, "{\"type\":\"session\"}\n").unwrap();
    app.begin_delete(Target::Chat(tree.paths[0].clone()))
        .unwrap();
    complete(&mut app);
    assert!(!tree.paths[0].exists());
    assert!(tree.paths[4].exists());
}

#[test]
fn pathless_descendant_is_guarded_and_removed_with_its_saved_ancestor() {
    let mut app = app();
    let tree = tree(&mut app);
    app.agents.nodes.get_mut(&tree.ids[2]).unwrap().path = None;
    app.agents
        .parked
        .get_mut(&tree.ids[2])
        .unwrap()
        .state
        .session_file = None;
    app.prefs.agent_tree.retain(|n| n.id != tree.ids[2]);
    app.begin_delete(Target::Chat(tree.paths[0].clone()))
        .unwrap();
    assert!(app.deletion.deleting_agent(&tree.ids[2]));
    app.view_agent(&tree.ids[2]);
    assert_ne!(app.agents.active, tree.ids[2]);
    complete(&mut app);
    assert!(!app.agents.nodes.contains_key(&tree.ids[2]));
}

#[test]
fn descendant_scheduled_jobs_are_paused_before_deletion_and_cannot_restore_owners() {
    let mut app = app();
    let tree = tree(&mut app);
    let active = app.agents.parked.remove(&tree.ids[2]).unwrap();
    app.agents.active = tree.ids[2].clone();
    app.put_chat(active);
    app.ultracode_ready = true;
    let source = app.agents.active.clone();
    app.automation_control(
        &source,
        &json!({"action":"automation_create", "kind":"cron",
        "name":"grandchild cron", "message":"follow up", "schedule":"*/5 * * * *"}),
    )
    .unwrap();
    app.begin_delete(Target::Chat(tree.paths[0].clone()))
        .unwrap();
    complete(&mut app);
    let jobs = app
        .automation_control(&source, &json!({"action":"automation_list"}))
        .unwrap();
    assert_eq!(jobs["jobs"][0]["enabled"], false);
    assert_eq!(
        jobs["jobs"].as_array().unwrap().len(),
        1,
        "Keep job history"
    );
    app.restore_job_owners();
    assert!(
        !app.agents
            .nodes
            .values()
            .any(|n| n.path.as_ref() == Some(&tree.paths[2]))
    );
    app.shutdown_agents();
}

#[test]
fn recursive_confirmation_explains_scope_and_cancels_in_both_themes_and_zoom() {
    for dark in [false, true] {
        for zoom in [1.0, 1.5] {
            let mut app = app();
            let tree = tree(&mut app);
            app.ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            app.ctx.set_zoom_factor(zoom);
            app.request_delete(Target::Chat(tree.paths[0].clone()));
            let ctx = app.ctx.clone();
            let mut copy = String::new();
            fn text(shape: &egui::Shape, copy: &mut String) {
                match shape {
                    egui::Shape::Text(t) => {
                        copy.push_str(&t.galley.job.text);
                        copy.push('\n');
                    }
                    egui::Shape::Vec(shapes) => {
                        for shape in shapes {
                            text(shape, copy);
                        }
                    }
                    _ => {}
                }
            }
            for _ in 0..3 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(420.0, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| app.deletion_dialog(ctx),
                );
                for shape in output.shapes {
                    text(&shape.shape, &mut copy);
                }
            }
            assert!(copy.contains("every descendant worker chat"));
            assert!(copy.contains("4 saved transcript(s)"));
            assert!(copy.contains("Workflow history"));
            let _ = ctx.run(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                },
                |ctx| app.deletion_dialog(ctx),
            );
            assert!(app.deletion.pending.is_none());
            assert!(tree.paths.iter().all(|p| p.exists()));
        }
    }
}
