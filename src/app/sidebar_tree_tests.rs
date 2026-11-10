use super::*;
fn app() -> Pui {
    Pui::new(
        &eframe::CreationContext::_new_kittest(egui::Context::default()),
        Options {
            demo: true,
            ..Default::default()
        },
    )
}
fn session(path: &str, title: &str) -> Session {
    Session {
        path: path.into(),
        cwd: "/fixture/project".into(),
        project: "/fixture/project".into(),
        title: title.into(),
        modified: SystemTime::now(),
    }
}
fn fixture() -> Pui {
    let mut app = app();
    app.sessions.clear();
    app.state = State {
        session_name: "Parent agent".into(),
        session_file: Some("/fixture/parent.jsonl".into()),
        ..Default::default()
    };
    app.demo_agent_tree();
    let root = app.agents.active.clone();
    let children = app.agents.nodes[&root].children.clone();
    for (index, id) in children.iter().enumerate() {
        let path = PathBuf::from(format!("/fixture/child-{index}.jsonl"));
        app.agents.nodes.get_mut(id).unwrap().path = Some(path.clone());
        app.agents.parked.get_mut(id).unwrap().state.session_file =
            Some(path.display().to_string());
        app.sessions.push(session(
            path.to_str().unwrap(),
            &format!("Duplicated child {index}"),
        ));
    }
    app.sessions
        .push(session("/fixture/parent.jsonl", "Parent agent"));
    app
}
fn labels(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(t) => out.push((
            t.galley.job.text.clone(),
            egui::Rect::from_min_size(t.pos, t.galley.size()),
        )),
        egui::Shape::Vec(v) => {
            for s in v {
                labels(s, out)
            }
        }
        _ => {}
    }
}
fn render(app: &mut Pui, events: Vec<egui::Event>) -> egui::FullOutput {
    app.ctx.clone().run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 850.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| app.sidebar(ctx),
    )
}
fn click(app: &mut Pui, point: egui::Pos2) {
    for pressed in [true, false] {
        render(
            app,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}
#[test]
fn agent_sessions_are_deduplicated_and_nested_even_when_child_is_more_recent() {
    let app = fixture();
    let forest = forest(&app);
    assert_eq!(forest.len(), 1);
    assert_eq!(forest[0].title, "Parent agent");
    assert_eq!(forest[0].children.len(), 2);
    assert!(
        forest[0]
            .children
            .iter()
            .all(|c| matches!(&c.target, Target::Agent(_)))
    );
    assert!(forest[0].matches("model capabilities"));
    assert!(!forest[0].matches("missing result"));
}
#[test]
fn agent_tree_is_in_the_project_list_once_and_child_click_keeps_parent_draft() {
    for projects in [false, true] {
        let mut app = fixture();
        app.projects = projects;
        app.demo = false;
        app.client = Some(rpc::Client::stub());
        app.state.busy = true;
        app.draft = "unsent parent".into();
        let parent = app.agents.active.clone();
        let child = app.agents.nodes[&parent].children[0].clone();
        for _ in 0..3 {
            render(&mut app, vec![]);
        }
        let mut text = Vec::new();
        for s in render(&mut app, vec![]).shapes {
            labels(&s.shape, &mut text);
        }
        assert_eq!(text.iter().filter(|(s, _)| s == "Parent agent").count(), 1);
        assert!(!text.iter().any(|(s, _)| s == "AGENT WORKFLOWS"));
        let root = text.iter().find(|(s, _)| s == "Parent agent").unwrap().1;
        let worker = text
            .iter()
            .find(|(s, _)| s == "Inspect model capabilities")
            .unwrap()
            .1;
        assert!(worker.left() > root.left());
        assert!(worker.top() > root.bottom());
        click(&mut app, worker.center());
        assert_eq!(app.agents.active, child);
        assert!(app.agents.parked[&parent].state.busy);
        app.draft = "child follow-up".into();
        app.view_agent(&parent);
        assert_eq!(app.draft, "unsent parent");
        app.view_agent(&child);
        assert_eq!(app.draft, "child follow-up");
    }
}
#[test]
fn disclosure_click_only_collapses_children_and_state_roundtrips() {
    let mut app = fixture();
    for _ in 0..3 {
        render(&mut app, vec![]);
    }
    let root = app.agents.active.clone();
    let mut text = Vec::new();
    for shape in render(&mut app, vec![]).shapes {
        labels(&shape.shape, &mut text);
    }
    let row = text.iter().find(|(s, _)| s == "Parent agent").unwrap().1;
    click(&mut app, egui::pos2(row.left() - 35.0, row.center().y));
    let key = forest(&app)[0].key.clone();
    assert!(app.prefs.closed_threads.contains(&key));
    assert_eq!(app.agents.active, root);
    let mut hidden = Vec::new();
    for shape in render(&mut app, vec![]).shapes {
        labels(&shape.shape, &mut hidden);
    }
    assert!(
        !hidden
            .iter()
            .any(|(s, _)| s == "Inspect model capabilities")
    );
    let saved: Preferences =
        serde_json::from_value(serde_json::to_value(&app.prefs).unwrap()).unwrap();
    assert!(saved.closed_threads.contains(&key));
    click(&mut app, egui::pos2(row.left() - 35.0, row.center().y));
    assert!(!app.prefs.closed_threads.contains(&key));
    let mut shown = Vec::new();
    for shape in render(&mut app, vec![]).shapes {
        labels(&shape.shape, &mut shown);
    }
    assert!(shown.iter().any(|(s, _)| s == "Inspect model capabilities"));
    assert_eq!(app.agents.active, root);
}
#[test]
fn children_are_single_line_compact_rows_in_both_views_and_themes() {
    for projects in [false, true] {
        for mode in [theme::Theme::Dark, theme::Theme::Light] {
            for zoom in [0.8, 1.0, 1.4] {
                let mut app = fixture();
                app.projects = projects;
                mode.apply(&app.ctx);
                app.ctx.set_zoom_factor(zoom);
                for _ in 0..3 {
                    render(&mut app, vec![]);
                }
                let mut text = Vec::new();
                for shape in render(&mut app, vec![]).shapes {
                    labels(&shape.shape, &mut text);
                }
                let first = text
                    .iter()
                    .find(|(s, _)| s == "Inspect model capabilities")
                    .unwrap()
                    .1;
                let second = text
                    .iter()
                    .find(|(s, _)| s == "Review UI interactions")
                    .unwrap()
                    .1;
                let gap = second.center().y - first.center().y;
                assert!(
                    (28.0..=34.0).contains(&gap),
                    "Compact child pitch must not retain a second status line: {gap}"
                );
                assert!(
                    !text.iter().any(|(s, _)| s == "Working…"),
                    "Child status stays in its dot and tooltip, not another line"
                );
                let child = app.agents.nodes[&app.agents.active].children[0].clone();
                click(&mut app, first.center());
                assert_eq!(app.agents.active, child);
            }
        }
    }
}
#[test]
fn disclosure_keyboard_toggles_without_switching_chat() {
    let mut app = fixture();
    for _ in 0..3 {
        render(&mut app, vec![]);
    }
    let root = app.agents.active.clone();
    let mut text = Vec::new();
    for shape in render(&mut app, vec![]).shapes {
        labels(&shape.shape, &mut text);
    }
    let row = text.iter().find(|(s, _)| s == "Parent agent").unwrap().1;
    click(&mut app, egui::pos2(row.left() - 35.0, row.center().y));
    let key = forest(&app)[0].key.clone();
    assert!(app.prefs.closed_threads.contains(&key));
    render(
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(!app.prefs.closed_threads.contains(&key));
    assert_eq!(app.agents.active, root);
}
#[test]
fn collapse_hides_the_whole_subtree_and_search_reveals_ancestors() {
    let mut app = fixture();
    let tree = forest(&app);
    app.prefs.closed_threads.insert(tree[0].key.clone());
    for _ in 0..3 {
        render(&mut app, vec![]);
    }
    let mut text = Vec::new();
    for s in render(&mut app, vec![]).shapes {
        labels(&s.shape, &mut text);
    }
    assert!(!text.iter().any(|(s, _)| s == "Inspect model capabilities"));
    app.search = "model capabilities".into();
    text.clear();
    for s in render(&mut app, vec![]).shapes {
        labels(&s.shape, &mut text);
    }
    assert!(text.iter().any(|(s, _)| s == "Parent agent"));
    assert!(text.iter().any(|(s, _)| s == "Inspect model capabilities"));
    assert!(!text.iter().any(|(s, _)| s == "Review UI interactions"));
}
#[test]
fn task_runs_open_under_their_creator_and_old_unlinked_runs_get_a_task_group() {
    let mut app = app();
    app.sessions = vec![session("/fixture/parent.jsonl", "Parent agent")];
    app.state.session_file = Some("/fixture/parent.jsonl".into());
    app.new_task();
    let task_id = app.tasks.selected.clone().unwrap();
    app.tasks.records[0].objective = "Fix navigation".into();
    app.tasks.records[0].runs.push(crate::tasks::Run {
        id: "run1".into(),
        role: crate::tasks::Role::Proposal,
        attempt: 0,
        status: "Working".into(),
        session: Some("/fixture/task-worker.jsonl".into()),
        summary: String::new(),
        cost: 0.0,
        permission_cost: 0.0,
        selection: None,
        routing_for: None,
    });
    app.sessions.insert(
        0,
        session("/fixture/task-worker.jsonl", "Task abc · Acceptance checks"),
    );
    let roots = forest(&app);
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].children.len(), 1);
    let task = &roots[0].children[0];
    assert_eq!(task.title, "Task · Fix navigation");
    assert_eq!(task.children.len(), 1);
    assert_eq!(
        task.children[0].target,
        Target::Task(task_id.clone(), Some("run1".into()))
    );
    app.tasks.parents.clear();
    app.tasks.records[0].parent_session = None;
    let roots = forest(&app);
    assert_eq!(roots.len(), 2);
    assert!(
        roots
            .iter()
            .any(|b| matches!(b.target, Target::Task(_, None)) && b.children.len() == 1)
    );
    assert!(
        !roots
            .iter()
            .any(|b| b.path.as_deref() == Some(std::path::Path::new("/fixture/task-worker.jsonl")))
    );
}
#[test]
fn clicking_task_worker_opens_existing_supervisor_without_starting_a_chat() {
    let mut app = app();
    app.sessions = vec![session("/fixture/parent.jsonl", "Parent agent")];
    app.state.session_file = Some("/fixture/parent.jsonl".into());
    app.state.session_name = "Parent agent".into();
    app.new_task();
    let task_id = app.tasks.selected.clone().unwrap();
    app.tasks.records[0].objective = "Inspect navigation".into();
    app.tasks.records[0].runs.push(crate::tasks::Run {
        id: "proposal-run".into(),
        role: crate::tasks::Role::Proposal,
        attempt: 0,
        status: "Finished".into(),
        session: Some("/fixture/task-worker.jsonl".into()),
        summary: String::new(),
        cost: 0.0,
        permission_cost: 0.0,
        selection: None,
        routing_for: None,
    });
    app.sessions.insert(
        0,
        session("/fixture/task-worker.jsonl", "Duplicate task worker"),
    );
    app.tasks.open = false;
    app.draft = "parent draft".into();
    let (client, _, requests) = rpc::Client::stub_with_records();
    app.client = Some(client);
    for _ in 0..3 {
        render(&mut app, vec![]);
    }
    let mut text = Vec::new();
    for shape in render(&mut app, vec![]).shapes {
        labels(&shape.shape, &mut text);
    }
    let worker = text
        .iter()
        .find(|(s, _)| s.starts_with("Acceptance proposal"))
        .unwrap()
        .1;
    click(&mut app, worker.center());
    assert!(app.tasks.open);
    assert_eq!(app.tasks.selected.as_deref(), Some(task_id.as_str()));
    assert_eq!(app.tasks.selected_run.as_deref(), Some("proposal-run"));
    assert_eq!(
        app.state.session_file.as_deref(),
        Some("/fixture/parent.jsonl")
    );
    assert_eq!(app.draft, "parent draft");
    assert!(
        requests.try_recv().is_err(),
        "Viewing task evidence must not send new/switch-session or a prompt"
    );
}
#[test]
fn queued_workers_and_multiple_generations_keep_their_exact_parent() {
    let mut app = fixture();
    let root = app.agents.active.clone();
    let child = app.agents.nodes[&root].children[0].clone();
    app.agents.nodes.get_mut(&child).unwrap().path = None;
    app.sessions
        .retain(|s| s.path != PathBuf::from("/fixture/child-0.jsonl"));
    app.agents.active = child.clone();
    app.state.session_file = None;
    app.demo_agent_tree();
    let roots = forest(&app);
    assert_eq!(roots.len(), 1);
    let worker = roots[0]
        .children
        .iter()
        .find(|b| b.target == Target::Agent(child.clone()))
        .unwrap();
    assert_eq!(worker.children.len(), 2);
    assert!(
        worker
            .children
            .iter()
            .all(|c| matches!(c.target, Target::Agent(_)))
    );
}
#[test]
fn macos_private_var_aliases_do_not_duplicate_task_runs() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let mut app = app();
    app.sessions = vec![session("/var/fixture/run.jsonl", "Recorded worker")];
    app.new_task();
    app.tasks.records[0].runs.push(crate::tasks::Run {
        id: "r".into(),
        role: crate::tasks::Role::Proposal,
        attempt: 0,
        status: "Finished".into(),
        session: Some("/private/var/fixture/run.jsonl".into()),
        summary: String::new(),
        cost: 0.0,
        permission_cost: 0.0,
        selection: None,
        routing_for: None,
    });
    let roots = forest(&app);
    assert!(!roots.iter().any(|b| matches!(b.target, Target::Session(_))));
    fn count(rows: &[Branch]) -> usize {
        rows.iter()
            .map(|row| {
                usize::from(
                    row.path
                        .as_ref()
                        .is_some_and(|p| path_key(p) == PathBuf::from("/var/fixture/run.jsonl")),
                ) + count(&row.children)
            })
            .sum()
    }
    assert_eq!(count(&roots), 1);
}
#[test]
fn parent_outside_recent_scan_remains_reachable_above_its_task() {
    let mut app = app();
    app.sessions.clear();
    app.state.session_file = Some("/fixture/older-parent.jsonl".into());
    app.new_task();
    app.agents = Default::default();
    app.tasks.parents.clear();
    let roots = forest(&app);
    assert_eq!(roots.len(), 1);
    assert_eq!(
        roots[0].target,
        Target::Saved("/fixture/older-parent.jsonl".into())
    );
    assert_eq!(roots[0].children.len(), 1);
}
#[test]
fn saved_task_parent_survives_restart_without_reparenting_by_name() {
    let mut app = app();
    app.sessions = vec![
        session("/fixture/parent.jsonl", "Parent agent"),
        session("/fixture/not-a-child.jsonl", "Task abc · Acceptance checks"),
    ];
    app.state.session_file = Some("/fixture/parent.jsonl".into());
    app.new_task();
    let task: crate::tasks::Task =
        serde_json::from_value(serde_json::to_value(&app.tasks.records[0]).unwrap()).unwrap();
    assert_eq!(
        task.parent_session.as_deref(),
        Some(std::path::Path::new("/fixture/parent.jsonl"))
    );
    app.agents = Default::default();
    app.tasks.parents.clear();
    app.tasks.records = vec![task];
    let roots = forest(&app);
    assert_eq!(roots.len(), 2);
    assert_eq!(
        roots
            .iter()
            .find(|r| r.title == "Parent agent")
            .unwrap()
            .children
            .len(),
        1
    );
    assert!(
        roots
            .iter()
            .any(|r| r.title == "Task abc · Acceptance checks")
    );
}
