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
fn proposal(app: &mut Pui) {
    app.state.session_file = Some("/fixture/task-parent.jsonl".into());
    app.state.messages = vec![Message {
        role: "assistant".into(),
        text: "Conversation context stays here".into(),
        ..Default::default()
    }];
    app.new_task();
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
    app.tasks.current_mut().unwrap().updated = 10;
    app.state.session_file = Some("/fixture/second-parent.jsonl".into());
    app.new_task();
    let second = app.tasks.selected.clone().unwrap();
    app.tasks.current_mut().unwrap().objective = "Second session workflow".into();
    app.tasks.current_mut().unwrap().phase = Phase::Accepted;
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
    assert!(
        text.iter()
            .any(|(s, _)| s == Phase::AwaitingApproval.label())
    );
    assert!(!text.iter().any(|(s, _)| s == Phase::Accepted.label()));
    app.state.session_file = Some("/fixture/no-tasks.jsonl".into());
    let text = labels(&inspector(&mut app, vec![]));
    assert!(text.iter().any(|(s, _)| s == "No task"));
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
        let title = text.iter().find(|(s, _)| s == "Workflow").unwrap().1;
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
fn task_toolbar_and_picker_use_session_ownership_not_workspace_or_global_selection() {
    let mut app = app();
    proposal(&mut app);
    app.tasks.current_mut().unwrap().objective = "First session only".into();
    let first = app.tasks.selected.clone().unwrap();
    app.state.session_file = Some("/fixture/second-parent.jsonl".into());
    app.new_task();
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
            .any(|(s, _)| s == "Approve acceptance checks · First session only")
    );
    assert!(!text.iter().any(|(s, _)| s.contains("Second session only")));
    app.tasks.open = false;
    app.state.session_file = Some("/fixture/second-parent.jsonl".into());
    app.toggle_chat_tasks();
    assert_eq!(app.tasks.selected.as_deref(), Some(second.as_str()));
    app.tasks.open = false;
    app.state.session_file = Some("/fixture/no-tasks.jsonl".into());
    app.toggle_chat_tasks();
    assert_eq!(app.tasks.records.len(), 3);
    assert_eq!(app.tasks.current().unwrap().phase, Phase::Draft);
    assert!(app.task_in_chat(app.tasks.current().unwrap()));
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
fn unsaved_chats_have_distinct_workflows_and_a_foreign_live_task_does_not_hijack_new_task() {
    let mut app = app();
    app.state.session_file = None;
    app.demo_agent_tree();
    app.new_task();
    let first = app.tasks.selected.clone().unwrap();
    let parent = app.agents.active.clone();
    let child = app.agents.nodes[&parent].children[0].clone();
    app.view_agent(&child);
    assert!(app.workflow_task().is_none());
    app.tasks.live = Some(first.clone());
    app.tasks.live_mut().unwrap().phase = Phase::Building;
    app.new_task();
    assert!(
        !app.tasks.open,
        "New task must not open another chat's running task"
    );
    assert!(app.workflow_task().is_none());
    assert_eq!(app.tasks.live.as_deref(), Some(first.as_str()));
    app.tasks.live = None;
    app.new_task();
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
fn linked_task_shows_chat_composer_and_readable_checks_without_changing_messages_or_approval() {
    let mut app = app();
    proposal(&mut app);
    app.draft = "Unsent chat draft".into();
    let messages = serde_json::to_value(&app.state.messages).unwrap();
    for _ in 0..4 {
        render(&mut app, vec![], false);
    }
    let text = labels(&render(&mut app, vec![], false));
    for expected in [
        "Conversation context stays here",
        "Here's the finish line",
        "The exact draft survives switching away and back",
        "cargo test draft_switch",
        "Edit check",
    ] {
        assert!(
            text.iter().any(|(s, _)| s == expected),
            "missing {expected}: {text:?}"
        );
    }
    assert!(text.iter().any(|(s, _)| s.starts_with("Chat with Pi")));
    assert!(
        !text.iter().any(|(s, _)| s == "Requirement"),
        "editor starts collapsed"
    );
    let edit = text.iter().find(|(s, _)| s == "Edit check").unwrap().1;
    click(&mut app, edit.center(), false);
    assert!(
        labels(&render(&mut app, vec![], false))
            .iter()
            .any(|(s, _)| s == "Requirement")
    );
    assert_eq!(app.draft, "Unsent chat draft");
    assert_eq!(serde_json::to_value(&app.state.messages).unwrap(), messages);
    assert!(app.tasks.current().unwrap().approved.is_none());
    assert!(app.tasks.processes.is_empty());
    assert!(app.tasks.job.is_none());
    assert!(app.tasks.current().unwrap().attempts.is_empty());
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
        .find(|(s, _)| s == "Review checks")
        .expect("task card in creator chat")
        .1;
    click(&mut app, review.center(), false);
    assert!(app.tasks.open);
    assert_eq!(app.tasks.selected, id);
    app.tasks.open = false;
    app.state.session_file = Some("/fixture/unrelated.jsonl".into());
    let text = labels(&render(&mut app, vec![], false));
    assert!(!text.iter().any(|(s, _)| s == "Review checks"));
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
