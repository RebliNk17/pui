use super::*;
#[path = "work_item_ui_tests.rs"]
mod work_item_ui_tests;

#[test]
fn display_path_cache_reuses_identity_bounds_retention_and_expires() {
    let cache = DisplayPaths::default();
    let path = Path::new("/fixture/display-chat");
    let resolved = cache.resolve(path);
    let stamp = cache.0.borrow()[path].1;
    for _ in 0..100 {
        assert_eq!(cache.resolve(path), resolved);
    }
    assert_eq!(
        cache.0.borrow()[path].1,
        stamp,
        "cache hit must not re-resolve"
    );
    cache.0.borrow_mut().get_mut(path).unwrap().1 = Instant::now() - Duration::from_secs(3);
    cache.resolve(path);
    assert!(cache.0.borrow()[path].1 > stamp);
    for i in 0..600 {
        cache.resolve(Path::new(&format!("/fixture/display-{i}")));
    }
    assert!(cache.0.borrow().len() <= 512);
}

fn app() -> Pui {
    Pui::new(
        &eframe::CreationContext::_new_kittest(egui::Context::default()),
        Options {
            demo: true,
            ..Default::default()
        },
    )
}
fn labels(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    fn collect(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => out.push((
                t.galley.job.text.clone(),
                t.galley.rect.translate(t.pos.to_vec2()),
            )),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    for shape in &output.shapes {
        collect(&shape.shape, &mut out);
    }
    out
}
fn render(app: &mut Pui, events: Vec<egui::Event>, picker: bool) -> egui::FullOutput {
    let ctx = app.ctx.clone();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1000.0, 1000.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            if picker {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.task_picker(ui);
                });
            } else if app.tasks.open {
                app.task_panel(ctx);
            } else {
                app.conversation(ctx);
            }
        },
    )
}
fn click(app: &mut Pui, pos: egui::Pos2, picker: bool) {
    for pressed in [true, false] {
        render(
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            picker,
        );
    }
}
fn add_workflow(app: &mut Pui) {
    let mut task = Task::new(
        app.prefs.cwd.clone(),
        app.state.provider.clone(),
        app.state.model.clone(),
        app.state.thinking.clone(),
    );
    task.parent_session = app.state.session_file.as_ref().map(PathBuf::from);
    let parent = app.ensure_agent();
    app.tasks.parents.insert(task.id.clone(), parent);
    app.tasks.selected = Some(task.id.clone());
    app.tasks.records.insert(0, task);
    app.tasks.open = true;
    app.tasks.selected_run = None;
    app.prefs.hide_inspector = false;
    app.prefs.cards.hidden.remove(&Card::Workflow);
    app.prefs.cards.collapsed.remove(&Card::Workflow);
}

fn proposal(app: &mut Pui) {
    app.state.session_file = Some("/fixture/task-parent.jsonl".into());
    app.state.messages = vec![Message {
        role: "assistant".into(),
        text: "Conversation context stays here".into(),
        ..Default::default()
    }];
    add_workflow(app);
    let t = app.tasks.current_mut().unwrap();
    t.objective = "Keep drafts while switching chats".into();
    t.phase = Phase::AwaitingApproval;
    t.proposed = Contract {
        criteria: vec![domain::Criterion {
            id: "C1".into(),
            description: "The exact draft survives switching away and back".into(),
            command: "cargo test draft_switch".into(),
        }],
    };
}

fn inspector(app: &mut Pui, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.ctx.clone();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1480.0, 960.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| app.inspector(ctx),
    )
}

#[test]
fn inspector_and_collapsed_summary_follow_session_and_click_the_displayed_task() {
    let mut app = app();
    proposal(&mut app);
    let first = app.tasks.selected.clone().unwrap();
    app.tasks.current_mut().unwrap().objective = "First session workflow".into();
    app.tasks.current_mut().unwrap().phase = Phase::Building;
    app.tasks.current_mut().unwrap().updated = 10;
    app.state.session_file = Some("/fixture/second-parent.jsonl".into());
    add_workflow(&mut app);
    let second = app.tasks.selected.clone().unwrap();
    app.tasks.current_mut().unwrap().objective = "Second session workflow".into();
    app.tasks.current_mut().unwrap().phase = Phase::Verifying;
    app.tasks.open = false;
    // Retained global selection must not decide which session's task is shown.
    app.state.session_file = Some("/fixture/task-parent.jsonl".into());
    assert_eq!(app.tasks.selected.as_deref(), Some(second.as_str()));
    assert_eq!(app.workflow_task().unwrap().id, first);
    for _ in 0..3 {
        inspector(&mut app, vec![]);
    }
    let text = labels(&inspector(&mut app, vec![]));
    assert!(text.iter().any(|(s, _)| s == "First session workflow"));
    assert!(!text.iter().any(|(s, _)| s == "Second session workflow"));
    app.prefs.cards.collapsed.insert(Card::Workflow);
    let text = labels(&inspector(&mut app, vec![]));
    assert!(text.iter().any(|(s, _)| s == Phase::Building.label()));
    assert!(!text.iter().any(|(s, _)| s == Phase::Verifying.label()));
    app.state.session_file = Some("/fixture/no-tasks.jsonl".into());
    let text = labels(&inspector(&mut app, vec![]));
    assert!(!text.iter().any(|(s, _)| s == "First session workflow"));
    assert!(!text.iter().any(|(s, _)| s == Card::Workflow.label()));
    assert!(app.workflow_task().is_none());
    app.prefs.cards.collapsed.remove(&Card::Workflow);
    app.state.session_file = Some("/fixture/second-parent.jsonl".into());
    let text = labels(&inspector(&mut app, vec![]));
    assert!(text.iter().any(|(s, _)| s == "Second session workflow"));
    app.state.session_file = Some("/fixture/task-parent.jsonl".into());
    let text = labels(&inspector(&mut app, vec![]));
    let pos = text.iter().find(|(s, _)| s == "Open").unwrap().1.center();
    for pressed in [true, false] {
        inspector(
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
    assert!(app.tasks.open);
    assert_eq!(app.tasks.selected.as_deref(), Some(first.as_str()));
    // Explicit navigation to a different task is still allowed, even from here.
    app.open_task_from_board(second.clone(), None);
    assert_eq!(app.workflow_task().unwrap().id, second);
    assert!(app.tasks.processes.is_empty());
    assert!(app.tasks.job.is_none());
}

#[test]
fn workflow_header_action_stays_next_to_title_when_collapsed_and_does_not_toggle_or_reorder() {
    for collapsed in [false, true] {
        let mut app = app();
        proposal(&mut app);
        app.tasks.current_mut().unwrap().phase = Phase::Building;
        let id = app.tasks.selected.clone();
        app.tasks.open = false;
        app.tasks.selected_run = Some("old-transcript".into());
        if collapsed {
            app.prefs.cards.collapsed.insert(Card::Workflow);
        }
        let order = app.prefs.cards.order.clone();
        for _ in 0..3 {
            inspector(&mut app, vec![]);
        }
        let text = labels(&inspector(&mut app, vec![]));
        let title = text
            .iter()
            .find(|(s, _)| s == Card::Workflow.label())
            .unwrap()
            .1;
        let buttons: Vec<_> = text.iter().filter(|(s, _)| s == "Open").collect();
        assert_eq!(buttons.len(), 1, "one action, not a second footer button");
        assert!((buttons[0].1.center().y - title.center().y).abs() < 1.0);
        assert!(buttons[0].1.left() > title.right());
        let rect = app
            .ctx
            .data(|d| {
                d.get_temp::<egui::Rect>(egui::Id::new(("inspector-header-action", Card::Workflow)))
            })
            .unwrap();
        for pressed in [true, false] {
            let pos = rect.center();
            inspector(
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
        assert!(app.tasks.open);
        assert_eq!(app.tasks.selected, id);
        assert!(app.tasks.selected_run.is_none());
        assert_eq!(
            app.prefs.cards.collapsed.contains(&Card::Workflow),
            collapsed
        );
        assert_eq!(app.prefs.cards.order, order);
        assert!(app.tasks.processes.is_empty());
    }
    let mut app = app();
    proposal(&mut app);
    app.tasks.open = false;
    app.state.session_file = Some("/fixture/unrelated.jsonl".into());
    let text = labels(&inspector(&mut app, vec![]));
    assert!(!text.iter().any(|(s, _)| s == "Open"));
}

#[test]
fn inspector_keeps_terminal_workflows_visible_and_history_accessible() {
    for phase in [
        Phase::Draft,
        Phase::AwaitingApproval,
        Phase::Ready,
        Phase::Accepted,
        Phase::Failed,
        Phase::Blocked,
        Phase::Interrupted,
        Phase::Cancelled,
        Phase::Stale,
    ] {
        let mut app = app();
        proposal(&mut app);
        app.tasks.current_mut().unwrap().phase = phase;
        let id = app.tasks.selected.clone().unwrap();
        app.tasks.open = false;
        for _ in 0..3 {
            inspector(&mut app, vec![]);
        }
        let text = labels(&inspector(&mut app, vec![]));
        assert!(
            text.iter().any(|(s, _)| s == Card::Workflow.label()),
            "{phase:?} must keep a persistent workflow inspector"
        );
        assert!(
            text.iter()
                .any(|(s, _)| s == "Keep drafts while switching chats")
        );
        app.open_task_from_board(id.clone(), None);
        assert!(app.tasks.open);
        assert_eq!(app.tasks.selected.as_deref(), Some(id.as_str()));
        let text = labels(&render(&mut app, vec![], false));
        assert!(
            text.iter()
                .any(|(s, _)| s == "Keep drafts while switching chats")
        );
        assert!(app.tasks.processes.is_empty());
    }
}

#[test]
fn task_toolbar_and_picker_use_session_ownership_not_workspace_or_global_selection() {
    let mut app = app();
    proposal(&mut app);
    app.tasks.current_mut().unwrap().objective = "First session only".into();
    let first = app.tasks.selected.clone().unwrap();
    app.state.session_file = Some("/fixture/second-parent.jsonl".into());
    add_workflow(&mut app);
    app.tasks.current_mut().unwrap().objective = "Second session only".into();
    let second = app.tasks.selected.clone().unwrap();
    app.tasks.open = false;
    app.state.session_file = Some("/fixture/task-parent.jsonl".into());
    app.toggle_chat_tasks();
    assert_eq!(app.tasks.selected.as_deref(), Some(first.as_str()));
    for _ in 0..3 {
        render(&mut app, vec![], true);
    }
    let rect = app
        .ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("task-picker-rect")))
        .unwrap();
    click(&mut app, rect.center(), true);
    let text = labels(&render(&mut app, vec![], true));
    assert!(
        text.iter()
            .any(|(s, _)| s == &format!("{} · First session only", Phase::AwaitingApproval.label()))
    );
    assert!(!text.iter().any(|(s, _)| s.contains("Second session only")));
    app.tasks.open = false;
    app.state.session_file = Some("/fixture/second-parent.jsonl".into());
    app.toggle_chat_tasks();
    assert_eq!(app.tasks.selected.as_deref(), Some(second.as_str()));
    app.tasks.open = false;
    app.state.session_file = Some("/fixture/no-tasks.jsonl".into());
    app.toggle_chat_tasks();
    assert_eq!(app.tasks.records.len(), 2);
    assert_eq!(app.tasks.selected.as_deref(), Some(second.as_str()));
    assert!(!app.tasks.open);
    assert!(app.workflow_task().is_none());
    assert!(app.tasks.processes.is_empty());
}

#[test]
fn saved_ownership_survives_reload_and_unlinked_tasks_are_not_assigned_by_workspace() {
    let mut app = app();
    proposal(&mut app);
    let first = app.tasks.selected.clone().unwrap();
    app.tasks.records =
        serde_json::from_value(serde_json::to_value(&app.tasks.records).unwrap()).unwrap();
    app.tasks.parents.clear();
    app.tasks.open = false;
    assert_eq!(app.workflow_task().unwrap().id, first);
    app.state.session_file = Some("/fixture/another-session-same-workspace.jsonl".into());
    assert!(app.workflow_task().is_none());
    app.state.session_file = Some("/fixture/task-parent.jsonl".into());
    app.tasks.current_mut().unwrap().parent_session = None;
    assert!(
        app.workflow_task().is_none(),
        "legacy unlinked tasks must not leak into a chat"
    );
    app.open_task_from_board(first.clone(), None);
    assert_eq!(
        app.workflow_task().unwrap().id,
        first,
        "explicit legacy task navigation still works"
    );
}

#[test]
fn unsaved_chats_have_distinct_workflows_and_foreign_live_work_does_not_hijack_navigation() {
    let mut app = app();
    app.state.session_file = None;
    app.demo_agent_tree();
    add_workflow(&mut app);
    let first = app.tasks.selected.clone().unwrap();
    let parent = app.agents.active.clone();
    let child = app.agents.nodes[&parent].children[0].clone();
    app.view_agent(&child);
    assert!(app.workflow_task().is_none());
    app.tasks.live = Some(first.clone());
    app.tasks.live_mut().unwrap().phase = Phase::Building;
    app.toggle_chat_tasks();
    assert!(!app.tasks.open);
    assert!(app.workflow_task().is_none());
    assert_eq!(app.tasks.records.len(), 1);
    assert_eq!(app.tasks.live.as_deref(), Some(first.as_str()));
    app.tasks.live = None;
    add_workflow(&mut app);
    let second = app.tasks.selected.clone().unwrap();
    assert_ne!(first, second);
    app.view_agent(&parent);
    assert_eq!(app.workflow_task().unwrap().id, first);
    app.view_agent(&child);
    assert_eq!(app.workflow_task().unwrap().id, second);
    assert!(app.tasks.processes.is_empty());
}

#[test]
fn picker_popup_bounds_long_spaced_and_unbroken_titles_and_keeps_navigation() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        let mut app = app();
        theme.apply(&app.ctx);
        app.state.session_file = Some("/fixture/picker-parent.jsonl".into());
        for (index, title) in ["Spaced objective ".repeat(150), "UnbrokenTitle".repeat(150)]
            .into_iter()
            .enumerate()
        {
            let mut task = Task::new(
                PathBuf::from("/fixture"),
                "p".into(),
                "m".into(),
                "off".into(),
            );
            task.objective = format!("{index} {title} END");
            task.parent_session = Some("/fixture/picker-parent.jsonl".into());
            app.tasks.records.push(task);
        }
        app.tasks.selected = Some(app.tasks.records[0].id.clone());
        for _ in 0..3 {
            render(&mut app, vec![], true);
        }
        let rect = app
            .ctx
            .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("task-picker-rect")))
            .unwrap();
        assert!(rect.width() <= 321.0, "{rect:?}");
        click(&mut app, rect.center(), true);
        for _ in 0..6 {
            let output = render(&mut app, vec![], true);
            let popup = app
                .ctx
                .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("task-picker-popup-rect")))
                .unwrap();
            assert!(popup.width() <= 321.0, "Popup geometry grew: {popup:?}");
            for (text, bounds) in labels(&output) {
                assert!(bounds.width() < 330.0, "menu grew for {text}: {bounds:?}");
            }
        }
        let output = render(&mut app, vec![], true);
        let rows = labels(&output);
        assert!(
            rows.iter().any(|(t, _)| t.starts_with("Draft · 1 ")),
            "popup must actually be open"
        );
        let row = rows
            .iter()
            .find(|(t, _)| t.starts_with("Draft · 1 "))
            .unwrap()
            .1;
        click(&mut app, row.center(), true);
        assert_eq!(app.tasks.selected.as_ref(), Some(&app.tasks.records[1].id));
        assert!(app.tasks.open);
        assert!(app.tasks.processes.is_empty());
    }
}

#[test]
fn legacy_workflow_shows_composer_and_read_only_checks_without_changing_state() {
    for phase in [Phase::Draft, Phase::AwaitingApproval] {
        let mut app = app();
        proposal(&mut app);
        app.tasks.current_mut().unwrap().phase = phase;
        app.draft = "Unsent chat draft".into();
        let messages = serde_json::to_value(&app.state.messages).unwrap();
        let task = serde_json::to_value(app.tasks.current()).unwrap();
        for _ in 0..4 {
            render(&mut app, vec![], false);
        }
        let text = labels(&render(&mut app, vec![], false));
        for expected in [
            "Conversation context stays here",
            "This legacy workflow is read-only. Ask the agent in chat to start a new workflow.",
            "The exact draft survives switching away and back",
        ] {
            assert!(
                text.iter().any(|(s, _)| s.contains(expected)),
                "missing {expected}: {text:?}"
            );
        }
        assert!(text.iter().any(|(s, _)| s.starts_with("Chat with Pi")));
        for removed in [
            "Edit check",
            "Requirement",
            "New task",
            "Generate acceptance checks",
            "Approve checks & start",
            "Cancel task",
            "Use current chat workspace and model",
            "Accept this result",
            "Revalidate source",
        ] {
            assert!(
                !text.iter().any(|(s, _)| s == removed),
                "unexpected {removed}"
            );
        }
        assert_eq!(app.draft, "Unsent chat draft");
        assert_eq!(serde_json::to_value(&app.state.messages).unwrap(), messages);
        assert_eq!(serde_json::to_value(app.tasks.current()).unwrap(), task);
        assert!(app.tasks.processes.is_empty());
        assert!(app.tasks.job.is_none());
    }
}

#[test]
fn chat_card_reopens_its_task_and_does_not_appear_in_other_conversations() {
    let mut app = app();
    proposal(&mut app);
    let id = app.tasks.selected.clone();
    app.tasks.open = false;
    for _ in 0..3 {
        render(&mut app, vec![], false);
    }
    let output = render(&mut app, vec![], false);
    let text = labels(&output);
    let review = text
        .iter()
        .find(|(s, _)| s == "Open workflow")
        .expect("task card in creator chat")
        .1;
    click(&mut app, review.center(), false);
    assert!(app.tasks.open);
    assert_eq!(app.tasks.selected, id);
    app.tasks.open = false;
    app.state.session_file = Some("/fixture/unrelated.jsonl".into());
    let text = labels(&render(&mut app, vec![], false));
    assert!(!text.iter().any(|(s, _)| s == "Open workflow"));
    app.tasks.open = true;
    let text = labels(&render(&mut app, vec![], false));
    assert!(
        !text
            .iter()
            .any(|(s, _)| s == "Conversation context stays here"),
        "do not mix unrelated chat into task context"
    );
    assert!(!text.iter().any(|(s, _)| s.starts_with("Chat with Pi")));
}

#[test]
fn workflow_results_and_exceptional_routing_are_read_only() {
    for phase in [Phase::Ready, Phase::Accepted, Phase::Stale, Phase::Blocked] {
        let mut app = app();
        proposal(&mut app);
        let task = app.tasks.current_mut().unwrap();
        task.phase = phase;
        task.approved = Some(task.proposed.clone());
        task.pending_route = Some(domain::routing::PendingRoute {
            role: Role::Builder,
            selection: domain::routing::Selection {
                provider: "fixture-provider".into(),
                model: "fixture-exceptional-model".into(),
                exceptional: true,
                ..Default::default()
            },
        });
        let before = serde_json::to_value(app.tasks.current()).unwrap();
        for _ in 0..4 {
            render(&mut app, vec![], false);
        }
        let text = labels(&render(&mut app, vec![], false));
        assert!(
            text.iter()
                .any(|(s, _)| s.contains("Exceptional routing is blocked"))
        );
        assert!(text.iter().any(|(s, _)| s == "The finish line"));
        for removed in [
            "Accept this result",
            "Revalidate source",
            "Approve exceptional model · uses provider",
            "Decline",
        ] {
            assert!(
                !text.iter().any(|(s, _)| s == removed),
                "unexpected {removed}"
            );
        }
        assert_eq!(serde_json::to_value(app.tasks.current()).unwrap(), before);
        assert!(app.tasks.processes.is_empty());
        assert!(app.tasks.job.is_none());
    }
}

#[test]
fn active_workflow_is_not_hidden_by_an_older_selected_history_record() {
    let mut app = app();
    proposal(&mut app);
    let old = app.tasks.selected.clone().unwrap();
    app.tasks.current_mut().unwrap().phase = Phase::Ready;
    add_workflow(&mut app);
    let active = app.tasks.selected.clone().unwrap();
    app.tasks.current_mut().unwrap().phase = Phase::Building;
    app.tasks.open = false;
    app.tasks.selected = Some(old.clone());
    assert_eq!(app.inspector_workflow().unwrap().id, active);
    app.open_task_from_board(old.clone(), None);
    assert_eq!(app.inspector_workflow().unwrap().id, old);
}

#[test]
fn workflow_pages_never_expose_manual_lifecycle_controls() {
    for phase in [
        Phase::Draft,
        Phase::Proposing,
        Phase::AwaitingApproval,
        Phase::Preparing,
        Phase::Building,
        Phase::Capturing,
        Phase::Checking,
        Phase::Verifying,
        Phase::Ready,
        Phase::Accepted,
        Phase::Failed,
        Phase::Blocked,
        Phase::Interrupted,
        Phase::Cancelled,
        Phase::Stale,
    ] {
        let mut app = app();
        proposal(&mut app);
        app.tasks.current_mut().unwrap().phase = phase;
        for _ in 0..3 {
            render(&mut app, vec![], false);
        }
        let text = labels(&render(&mut app, vec![], false));
        for removed in [
            "New task",
            "New workflow",
            "What should change?",
            "Workspace & model settings",
            "Use current chat workspace and model",
            "Choose a model for each stage",
            "Stop at a reported spend threshold",
            "Generate acceptance checks",
            "Here's the finish line",
            "Edit check",
            "Requirement",
            "Exact command · blank means review only",
            "Use committed HEAD only; leave uncommitted changes out of this task",
            "Approve checks & start",
            "Cancel task",
            "Cancel workflow",
            "Approve exceptional model · uses provider",
            "Accept this result",
            "Revalidate source",
        ] {
            assert!(
                !text.iter().any(|(s, _)| s == removed),
                "{phase:?} exposed manual control: {removed}"
            );
        }
        assert!(app.tasks.processes.is_empty());
        assert!(app.tasks.job.is_none());
    }
}

#[test]
fn stop_stays_available_in_workflow_and_transcript_headers() {
    let mut app = app();
    app.demo_task();
    for run in [None, Some("demo-Builder".to_owned())] {
        app.tasks.selected_run = run;
        for _ in 0..3 {
            render(&mut app, vec![], false);
        }
        let text = labels(&render(&mut app, vec![], false));
        assert!(text.iter().any(|(s, _)| s == "Stop workflow"));
        assert!(!text.iter().any(|(s, _)| s == "New task"));
    }
}

#[test]
fn conversation_return_restores_exact_parked_parent_and_draft_without_provider_calls() {
    let mut app = app();
    proposal(&mut app);
    app.draft = "Parent draft".into();
    let parent = app.agents.active.clone();
    app.demo_agent_tree();
    let child = app.agents.nodes[&parent].children[0].clone();
    app.view_agent(&child);
    app.draft = "Other draft".into();
    app.tasks.open = true;
    app.task_parent_chat();
    assert!(!app.tasks.open);
    assert_eq!(app.agents.active, parent);
    assert_eq!(app.draft, "Parent draft");
    assert_eq!(
        app.state.session_file.as_deref(),
        Some("/fixture/task-parent.jsonl")
    );
    assert!(app.tasks.processes.is_empty());
}
