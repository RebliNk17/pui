use super::*;

fn app(ctx: &egui::Context) -> Pui {
    Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_kanban: true,
            ..Default::default()
        },
    )
}
fn frame(
    ctx: &egui::Context,
    app: &mut Pui,
    width: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    frame_size(ctx, app, egui::vec2(width, 850.0), events)
}
fn frame_size(
    ctx: &egui::Context,
    app: &mut Pui,
    size: Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            app.sidebar(ctx);
            app.top_bar(ctx);
            if app.kanban.open {
                app.agents_board(ctx);
            }
        },
    )
}
fn labels(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(t) => out.push((
            t.galley.job.text.clone(),
            egui::Rect::from_min_size(t.pos, t.galley.size()),
        )),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| labels(s, out)),
        _ => {}
    }
}
fn text(output: egui::FullOutput) -> Vec<(String, egui::Rect)> {
    let mut out = Vec::new();
    for shape in output.shapes {
        labels(&shape.shape, &mut out);
    }
    out
}
fn click(ctx: &egui::Context, app: &mut Pui, width: f32, pos: egui::Pos2) {
    frame(
        ctx,
        app,
        width,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    frame(
        ctx,
        app,
        width,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
}

#[test]
fn status_mapping_includes_transitional_and_inactive_agents() {
    for (label, expected) in [
        ("Working…", Status::Working),
        ("Connecting", Status::Working),
        ("Compacting context…", Status::Working),
        ("Preparing worktree", Status::Working),
        ("Changing history…", Status::Working),
        ("Idle", Status::Idle),
        ("Waiting for input", Status::Waiting),
        ("Queued", Status::Queued),
        ("Stopped", Status::Stopped),
        ("Saved", Status::Stopped),
        ("Saved · not connected", Status::Stopped),
        ("Owner unavailable", Status::Error),
        ("Failed: provider unavailable", Status::Error),
    ] {
        assert_eq!(Status::from_label(label), expected, "{label}");
    }
}

#[test]
fn approval_overrides_busy_and_error_and_cards_never_grant_permission() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let approval = app
        .board_cards()
        .into_iter()
        .find(|c| c.title == "Update sharing permissions")
        .unwrap();
    assert_eq!(approval.status, Status::Waiting);
    assert_eq!(approval.action, "Review approval");
    let Target::Agent(id) = approval.target.clone() else {
        panic!()
    };
    app.agents.parked.get_mut(&id).unwrap().state.error = Some("Previous error".into());
    app.agents.parked.get_mut(&id).unwrap().state.busy = true;
    assert_eq!(
        app.board_cards()
            .iter()
            .find(|c| c.title == approval.title)
            .unwrap()
            .status,
        Status::Waiting
    );
    app.open_board_card(approval.target);
    assert!(!app.kanban.open);
    assert_eq!(app.agents.active, id);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(
        app.client.is_none(),
        "Demo card must never launch a process"
    );
}

#[test]
fn questions_preview_the_actual_question_and_errors_are_separate_from_idle() {
    let ctx = egui::Context::default();
    let app = app(&ctx);
    let cards = app.board_cards();
    let question = cards
        .iter()
        .find(|c| c.title == "Refactor session search")
        .unwrap();
    assert_eq!(question.status, Status::Waiting);
    assert_eq!(question.action, "Answer question");
    assert!(!question.activity.is_empty());
    assert_ne!(question.activity, "Pui questions");
    assert_eq!(
        cards
            .iter()
            .find(|c| c.title == "Investigate failing build")
            .unwrap()
            .status,
        Status::Error
    );
}

#[test]
fn opening_board_preserves_busy_chat_draft_and_approvals() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.kanban.open = false;
    app.draft = "Keep this draft".into();
    app.state.dialogs.push_back(questions::demo_dialog());
    let active = app.agents.active.clone();
    let parked = app.agents.parked.len();
    app.open_agents_board();
    assert_eq!(app.agents.active, active);
    assert_eq!(app.agents.parked.len(), parked);
    assert_eq!(app.draft, "Keep this draft");
    assert!(app.state.busy);
    assert_eq!(app.state.dialogs.len(), 1);
    app.open_board_card(Target::Current);
    assert!(!app.kanban.open);
    assert_eq!(app.draft, "Keep this draft");
    assert!(app.state.busy);
}

#[test]
fn sidebar_keeps_overviews_in_menu_instead_of_above_search() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.kanban.open = false;
    for _ in 0..3 {
        frame(&ctx, &mut app, 1440.0, vec![]);
    }
    let output = text(frame(&ctx, &mut app, 1440.0, vec![]));
    let sidebar: Vec<_> = output
        .iter()
        .filter(|(_, rect)| rect.left() < 254.0)
        .collect();
    assert!(sidebar.iter().any(|(s, _)| s == "Search sessions"));
    assert!(sidebar.iter().any(|(s, _)| s == "Pui"));
    assert!(
        !sidebar
            .iter()
            .any(|(s, _)| s == "Agents board" || s.contains("Cron jobs & monitors"))
    );
}

#[test]
fn card_click_switches_to_parked_agent_and_keeps_original_busy() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let active = app.agents.active.clone();
    for _ in 0..4 {
        frame(&ctx, &mut app, 1440.0, vec![]);
    }
    let output = text(frame(&ctx, &mut app, 1440.0, vec![]));
    let card = output
        .iter()
        .find(|(s, r)| s == "Refactor session search" && r.left() > 254.0)
        .unwrap()
        .1;
    click(&ctx, &mut app, 1440.0, card.center());
    assert!(!app.kanban.open);
    assert_eq!(app.state.session_name, "Refactor session search");
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.agents.parked[&active].state.busy);
}

#[test]
fn board_renders_in_both_themes_at_narrow_and_wide_sizes() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        for width in [860.0, 1440.0] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            theme.apply(&ctx);
            for _ in 0..3 {
                frame(&ctx, &mut app, width, vec![]);
            }
            let output = text(frame(&ctx, &mut app, width, vec![]));
            // Narrow windows scroll horizontally; offscreen labels are culled by egui.
            let expected: &[&str] = if width < 1000.0 {
                &["Working", "Waiting for input"]
            } else {
                &["Working", "Waiting for input", "Idle", "Error"]
            };
            for &label in expected {
                assert!(
                    output.iter().any(|(s, _)| s == label),
                    "Missing {label} at {width}"
                );
            }
            let board_left = 254.0;
            for (_, rect) in output.iter().filter(|(s, _)| s == "Working") {
                assert!(rect.left() >= board_left);
            }
        }
    }
}

fn crowded_app(ctx: &egui::Context) -> Pui {
    let mut app = app(ctx);
    app.state.session_name = "Overflow agent 00".into();
    app.state.messages.clear();
    for (index, (id, chat)) in app.agents.parked.iter_mut().enumerate() {
        chat.state.session_name = format!("Overflow agent {:02}", index + 1);
        chat.state.busy = true;
        chat.state.dialogs.clear();
        chat.state.error = None;
        chat.state.messages.clear();
        app.agents.nodes.get_mut(id).unwrap().status = "Working…".into();
    }
    app
}
fn column_scroll(ctx: &egui::Context, status: Status) -> (egui::Id, egui::Rect, Vec2) {
    ctx.data_mut(|data| {
        data.get_temp(egui::Id::new(("kanban-scroll", status.label())))
            .unwrap()
    })
}
#[test]
fn overflowing_column_scrolls_down_to_last_card_without_moving_other_columns() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        for size in [egui::vec2(1440.0, 850.0), egui::vec2(860.0, 620.0)] {
            let ctx = egui::Context::default();
            let mut app = crowded_app(&ctx);
            theme.apply(&ctx);
            for _ in 0..5 {
                frame_size(&ctx, &mut app, size, vec![]);
            }
            let last_title = app
                .board_cards()
                .into_iter()
                .rfind(|c| c.status == Status::Working)
                .unwrap()
                .title;
            let (id, viewport, content) = column_scroll(&ctx, Status::Working);
            assert!(content.y > viewport.height(), "Fixture must overflow");
            let before = text(frame_size(&ctx, &mut app, size, vec![]));
            let header = before.iter().find(|(s, _)| s == "Working").unwrap().1;
            frame_size(
                &ctx,
                &mut app,
                size,
                vec![
                    egui::Event::PointerMoved(viewport.center()),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -3000.0),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            for _ in 0..20 {
                frame_size(&ctx, &mut app, size, vec![]);
            }
            let state = egui::scroll_area::State::load(&ctx, id).unwrap();
            assert!(state.offset.y > 0.0, "Wheel must scroll over agent cards");
            let output = frame_size(&ctx, &mut app, size, vec![]);
            let mut visible = Vec::new();
            for shape in output.shapes {
                let mut entries = Vec::new();
                labels(&shape.shape, &mut entries);
                visible.extend(
                    entries
                        .into_iter()
                        .filter(|(_, r)| shape.clip_rect.contains_rect(*r)),
                );
            }
            assert!(
                visible.iter().any(|(s, _)| s == &last_title),
                "Last card must be fully visible: size={size:?}, offset={:?}, viewport={viewport:?}, content={content:?}, visible={visible:?}",
                state.offset
            );
            assert_eq!(
                visible.iter().find(|(s, _)| s == "Working").unwrap().1,
                header
            );
            let (other_id, _, _) = column_scroll(&ctx, Status::Waiting);
            assert_eq!(
                egui::scroll_area::State::load(&ctx, other_id)
                    .unwrap()
                    .offset
                    .y,
                0.0
            );
            let horizontal = ctx.data_mut(|data| {
                data.get_temp::<egui::Id>(egui::Id::new("kanban-horizontal-scroll"))
                    .unwrap()
            });
            frame_size(
                &ctx,
                &mut app,
                size,
                vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -300.0),
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            for _ in 0..12 {
                frame_size(&ctx, &mut app, size, vec![]);
            }
            assert_eq!(
                egui::scroll_area::State::load(&ctx, horizontal)
                    .unwrap()
                    .offset
                    .x,
                0.0,
                "Scrolling down at the column bottom must not unexpectedly pan the board sideways"
            );
            assert!(
                text(frame_size(&ctx, &mut app, size, vec![]))
                    .iter()
                    .any(|(s, rect)| s.starts_with("Click an agent") && rect.bottom() < size.y)
            );
        }
    }
}
#[test]
fn overflowing_column_has_a_visible_draggable_scrollbar() {
    let ctx = egui::Context::default();
    let mut app = crowded_app(&ctx);
    for _ in 0..6 {
        frame(&ctx, &mut app, 1440.0, vec![]);
    }
    let (id, viewport, _) = column_scroll(&ctx, Status::Working);
    let mut state = egui::scroll_area::State::load(&ctx, id).unwrap();
    state.offset.y = 0.0;
    state.store(&ctx, id);
    // Solid bars reserve 4pt margin + 6pt track immediately to the right.
    let track = egui::pos2(viewport.right() + 7.0, viewport.bottom() - 12.0);
    click(&ctx, &mut app, 1440.0, track);
    assert!(
        egui::scroll_area::State::load(&ctx, id).unwrap().offset.y > 0.0,
        "Mouse users must be able to scroll by clicking the visible track"
    );
}
#[test]
fn narrow_board_can_scroll_to_error_column() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    for _ in 0..4 {
        frame(&ctx, &mut app, 860.0, vec![]);
    }
    frame(
        &ctx,
        &mut app,
        860.0,
        vec![
            egui::Event::PointerMoved(egui::pos2(600.0, 400.0)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(-700.0, 0.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    for _ in 0..12 {
        frame(&ctx, &mut app, 860.0, vec![]);
    }
    let output = text(frame(&ctx, &mut app, 860.0, vec![]));
    assert!(output.iter().any(|(s, _)| s == "Investigate failing build"));
}
#[test]
fn filters_change_visible_cards_and_restore_when_project_disappears() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.kanban.project = Some(PathBuf::from("/example/agent-harbor"));
    for _ in 0..3 {
        frame(&ctx, &mut app, 1440.0, vec![]);
    }
    let output = text(frame(&ctx, &mut app, 1440.0, vec![]));
    assert!(
        output
            .iter()
            .any(|(s, _)| s == "2 agents · 1 waiting for you")
    );
    assert!(
        !output
            .iter()
            .any(|(s, rect)| s == "Inspect model capabilities" && rect.left() > 254.0)
    );
    app.kanban.project = Some(PathBuf::from("/no-longer-live"));
    app.kanban.roots_only = true;
    let output = text(frame(&ctx, &mut app, 1440.0, vec![]));
    assert!(app.kanban.project.is_none());
    assert!(
        !output
            .iter()
            .any(|(s, rect)| s == "Inspect model capabilities" && rect.left() > 254.0)
    );
}

#[test]
fn queued_and_stopped_columns_appear_only_when_needed() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let id = app
        .agents
        .nodes
        .values()
        .find(|n| n.name == "Fix artifact previews")
        .unwrap()
        .id
        .clone();
    app.agents.nodes.get_mut(&id).unwrap().status = "Queued".into();
    assert_eq!(
        app.board_cards()
            .iter()
            .find(|c| c.title == "Fix artifact previews")
            .unwrap()
            .status,
        Status::Queued
    );
    for _ in 0..3 {
        frame(&ctx, &mut app, 2000.0, vec![]);
    }
    let output = text(frame(&ctx, &mut app, 2000.0, vec![]));
    assert!(output.iter().any(|(s, _)| s == "Queued"));
    assert!(!output.iter().any(|(s, _)| s == "Stopped"));
    app.agents.nodes.get_mut(&id).unwrap().status = "Stopped".into();
    app.agents.nodes.get_mut(&id).unwrap().stopped = true;
    let output = text(frame(&ctx, &mut app, 2000.0, vec![]));
    assert!(output.iter().any(|(s, _)| s == "Stopped"));
    assert!(!output.iter().any(|(s, _)| s == "Queued"));
}
#[test]
fn a_single_normal_session_appears_without_creating_agent_nodes() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.agents = Default::default();
    app.state = State {
        session_name: "Single session".into(),
        busy: true,
        ..Default::default()
    };
    let cards = app.board_cards();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].title, "Single session");
    assert_eq!(cards[0].status, Status::Working);
    assert!(app.agents.nodes.is_empty());
}

#[test]
fn status_timer_resets_only_on_status_change() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    frame(&ctx, &mut app, 1440.0, vec![]);
    let key = format!("agent:{}", app.agents.active);
    let original = app.kanban.observed[&key].1;
    frame(&ctx, &mut app, 1440.0, vec![]);
    assert_eq!(app.kanban.observed[&key].1, original);
    app.state.busy = false;
    frame(&ctx, &mut app, 1440.0, vec![]);
    assert_eq!(app.kanban.observed[&key].0, Status::Idle);
    assert!(app.kanban.observed[&key].1 >= original);
}

#[test]
fn empty_board_is_explicit_and_saved_history_is_not_a_live_agent() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.demo = false;
    app.connected = false;
    app.loading_chat = false;
    app.state = State::default();
    app.agents = Default::default();
    assert!(app.board_cards().is_empty());
    let id = app.ensure_agent();
    // A restored saved-only node has no live process or transcript.
    app.agents.nodes.get_mut(&id).unwrap().stopped = true;
    app.agents.nodes.get_mut(&id).unwrap().status = "Saved".into();
    app.park_tree();
    assert!(app.board_cards().is_empty());
    let output = text(frame(&ctx, &mut app, 1440.0, vec![]));
    assert!(output.iter().any(|(s, _)| s.contains("No live agents yet")));
}

#[test]
fn escape_leaves_board_without_stopping_a_busy_agent() {
    use eframe::App;
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (client, _) = rpc::Client::stub_with_events();
    app.client = Some(client);
    app.connected = true;
    let path = PathBuf::from("/example/current.jsonl");
    app.state.session_file = Some(path.display().to_string());
    app.waiting
        .insert(path.clone(), vec![run_status::WaitingMessage::default()]);
    let mut native = eframe::Frame::_new_kittest();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1440.0, 850.0),
            )),
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| app.update(ctx, &mut native),
    );
    assert!(!app.kanban.open);
    assert!(app.state.busy);
    assert_eq!(app.waiting[&path].len(), 1, "Stop must not clear the queue");
}
#[test]
fn elapsed_and_summary_are_bounded_and_unicode_safe() {
    assert_eq!(elapsed(Duration::from_secs(59)), "59s");
    assert_eq!(elapsed(Duration::from_secs(3601)), "1h 0m");
    assert_eq!(short("你好世界", 2), "你好…");
    assert_eq!(short("hello", 5), "hello");
}
