use super::*;

fn app(ctx: &egui::Context) -> Pui {
    Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    )
}

fn frame(app: &mut Pui, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut window = eframe::Frame::_new_kittest();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1400.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut window),
    )
}

fn side_button(button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: egui::pos2(650.0, 150.0),
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn history_moves_both_ways_and_new_navigation_discards_the_forward_branch() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.open_agents_board();
    app.remember_navigation();
    app.open_automation();
    app.remember_navigation();
    assert_eq!(app.navigation.entries.len(), 3);
    app.navigate_history(false);
    assert!(app.kanban.open);
    app.navigate_history(false);
    assert!(!app.kanban.open && !app.automation.open);
    app.navigate_history(true);
    assert!(app.kanban.open);
    app.settings_open = true;
    app.remember_navigation();
    assert!(!app.navigation.can_go(true));
    app.navigate_history(false);
    assert!(app.kanban.open && !app.settings_open);
    app.navigate_history(true);
    assert!(app.settings_open);
}

#[test]
fn observing_frames_or_async_title_and_session_updates_does_not_add_history() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    for _ in 0..5 {
        app.remember_navigation();
    }
    app.state.session_name = "A better title".into();
    app.remember_navigation();
    assert_eq!(app.navigation.entries.len(), 1);
    app.state.session_file = Some("/fixture/new-chat".into());
    app.ensure_agent();
    app.remember_navigation();
    assert_eq!(app.navigation.entries.len(), 1);
}

#[test]
fn saved_identity_arriving_in_settings_preserves_back_forward_and_live_draft() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.client = Some(rpc::Client::stub());
    app.draft = "Keep this live draft".into();
    let original = app.ensure_agent();
    app.settings_open = true;
    app.remember_navigation();
    app.state.session_file = Some("/fixture/arrived-while-in-settings".into());
    app.remember_navigation();
    assert_eq!(app.navigation.entries.len(), 2);
    app.navigate_history(false);
    assert!(!app.settings_open);
    assert_eq!(app.agents.active, original);
    assert_eq!(app.draft, "Keep this live draft");
    assert!(app.client.is_some());
    assert!(app.navigation.can_go(true));
    app.navigate_history(true);
    assert!(app.settings_open);
}

#[test]
fn pathless_parked_chat_is_restored_by_agent_after_background_path_assignment() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.demo = false;
    app.client = Some(rpc::Client::stub());
    app.draft = "Original unsaved draft".into();
    let original = app.ensure_agent();
    app.start_chat_in(app.prefs.cwd.clone());
    app.remember_navigation();
    app.agents
        .parked
        .get_mut(&original)
        .unwrap()
        .state
        .session_file = Some("/fixture/background-assigned".into());
    app.agents.nodes.get_mut(&original).unwrap().path =
        Some(PathBuf::from("/fixture/background-assigned"));
    app.navigate_history(false);
    assert_eq!(app.agents.active, original);
    assert_eq!(app.draft, "Original unsaved draft");
    assert_eq!(
        app.state.session_file.as_deref(),
        Some("/fixture/background-assigned")
    );
    assert!(app.navigation.can_go(true));
    app.shutdown_agents();
}

#[test]
fn stale_blank_locations_never_match_unrelated_saved_chats_in_the_same_folder() {
    let cwd = std::env::temp_dir();
    let blank = Chat {
        agent: String::new(),
        anonymous: Some(1),
        path: None,
        cwd: cwd.clone(),
        title: "Blank".into(),
    };
    let saved = Chat {
        agent: "different-agent".into(),
        anonymous: None,
        path: Some(cwd.join("saved.jsonl")),
        cwd,
        title: "Saved".into(),
    };
    assert!(!blank.same(&saved));
    assert!(!saved.same(&blank));
}

#[test]
fn missing_disconnected_saved_chat_is_skipped_but_connected_chat_remains_available() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.demo = false;
    app.client = Some(rpc::Client::stub());
    let first = app.ensure_agent();
    app.start_chat_in(app.prefs.cwd.clone());
    let missing = app.agents.active.clone();
    let missing_path =
        std::env::temp_dir().join(format!("pui-missing-nav-{}.jsonl", uuid::Uuid::new_v4()));
    app.state.session_file = Some(missing_path.display().to_string());
    app.agents.nodes.get_mut(&missing).unwrap().path = Some(missing_path);
    app.remember_navigation();
    let missing_location = app.navigation_location();
    app.start_chat_in(app.prefs.cwd.clone());
    app.remember_navigation();
    assert!(
        app.location_available(&missing_location),
        "A connected chat remains usable even without its file"
    );
    app.agents.parked.get_mut(&missing).unwrap().client = None;
    assert!(!app.location_available(&missing_location));
    app.navigate_history(false);
    assert_eq!(
        app.agents.active, first,
        "Back must skip the unavailable saved chat"
    );
    app.shutdown_agents();
}

#[test]
fn settings_pages_and_resource_views_restore_without_losing_drafts_or_busy_work() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.busy = true;
    app.draft = "Unsaved chat draft".into();
    app.settings_open = true;
    app.settings_view.select("Appearance");
    app.remember_navigation();
    app.settings_view.select("Notifications");
    app.remember_navigation();
    app.settings_view.select("Resources/Workspace");
    app.remember_navigation();
    app.settings_view.select("Resources/Packages");
    app.remember_navigation();
    app.navigate_history(false);
    assert_eq!(app.settings_view.location(), "Resources/Workspace");
    app.navigate_history(false);
    assert_eq!(app.settings_view.location(), "Notifications");
    app.navigate_history(false);
    assert_eq!(app.settings_view.location(), "Appearance");
    app.navigate_history(false);
    assert!(!app.settings_open);
    assert!(app.state.busy);
    assert_eq!(app.draft, "Unsaved chat draft");
}

#[test]
fn side_mouse_buttons_navigate_once_on_press_not_release() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.open_agents_board();
    app.remember_navigation();
    app.open_automation();
    app.remember_navigation();
    frame(
        &mut app,
        &ctx,
        vec![side_button(egui::PointerButton::Extra1, true)],
    );
    assert!(app.kanban.open);
    frame(
        &mut app,
        &ctx,
        vec![side_button(egui::PointerButton::Extra1, false)],
    );
    assert!(app.kanban.open);
    frame(
        &mut app,
        &ctx,
        vec![side_button(egui::PointerButton::Extra2, true)],
    );
    assert!(app.automation.open);
}

#[test]
fn keyboard_shortcuts_navigate_but_do_not_steal_text_cursor_arrows() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.open_agents_board();
    app.remember_navigation();
    let modifiers = if cfg!(target_os = "macos") {
        egui::Modifiers::MAC_CMD
    } else {
        egui::Modifiers::ALT
    };
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::OpenBracket,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }],
    );
    assert!(!app.kanban.open);
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::CloseBracket,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }],
    );
    assert!(app.kanban.open);
    assert!(app.navigation.can_go(false));
}

#[test]
fn navigation_is_blocked_during_confirmations_approvals_and_image_theater() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.open_agents_board();
    app.remember_navigation();
    app.menu.about = true;
    app.navigate_history(false);
    assert!(app.kanban.open);
    app.menu.about = false;
    app.state
        .dialogs
        .push_back(json!({"id":"approval", "method":"confirm"}));
    frame(
        &mut app,
        &ctx,
        vec![side_button(egui::PointerButton::Extra1, true)],
    );
    assert!(app.kanban.open);
    app.state.dialogs.clear();
    app.navigate_history(false);
    assert!(!app.kanban.open);
}

#[test]
fn workspace_panes_and_terminal_restore_without_stopping_shells() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.terminals.toggle(&ctx, &app.prefs.cwd, true);
    app.remember_navigation();
    app.files.toggle(&ctx, &app.prefs.cwd);
    app.remember_navigation();
    app.open_agents_board();
    app.remember_navigation();
    app.navigate_history(false);
    assert!(app.files.files_open());
    assert!(app.terminals.is_open());
    app.navigate_history(false);
    assert!(!app.files.is_open());
    assert!(app.terminals.is_open());
    app.navigate_history(false);
    assert!(!app.terminals.is_open());
    app.navigate_history(true);
    assert!(app.terminals.is_open());
}

#[test]
fn deleted_tasks_are_skipped_instead_of_recreated() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.demo_task();
    app.remember_navigation();
    app.open_agents_board();
    app.remember_navigation();
    app.tasks.records.clear();
    app.navigate_history(false);
    assert!(!app.kanban.open && !app.tasks.open);
    app.navigate_history(true);
    assert!(app.kanban.open);
}

#[test]
fn chat_history_restores_busy_agents_and_their_independent_drafts() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.demo = false;
    app.client = Some(rpc::Client::stub());
    app.state.session_file = Some("/fixture/first".into());
    app.state.busy = true;
    app.draft = "First draft".into();
    let first = app.ensure_agent();
    app.remember_navigation();
    app.start_chat_in(app.prefs.cwd.clone());
    let second = app.agents.active.clone();
    app.draft = "Second draft".into();
    app.remember_navigation();
    app.navigate_history(false);
    assert_eq!(app.agents.active, first);
    assert_eq!(app.draft, "First draft");
    assert!(app.state.busy && app.client.is_some());
    app.navigate_history(true);
    assert_eq!(app.agents.active, second);
    assert_eq!(app.draft, "Second draft");
    assert!(app.agents.parked[&first].state.busy);
    app.shutdown_agents();
}

#[test]
fn switching_disconnected_chats_preserves_unsent_drafts_and_images() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.demo = false;
    let root = std::env::temp_dir().join(format!("pui-nav-drafts-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let first_path = root.join("first.jsonl");
    let second_path = root.join("second.jsonl");
    std::fs::write(&first_path, "").unwrap();
    std::fs::write(&second_path, "").unwrap();
    app.state.session_file = Some(first_path.display().to_string());
    app.draft = "Disconnected unsent draft".into();
    app.draft_images.set_images(vec![
        crate::media::Attachment::from_bytes(include_bytes!("../../../assets/icon.png")).unwrap(),
    ]);
    app.ensure_agent();
    app.remember_navigation();
    app.open_chat(Session {
        path: second_path,
        cwd: root.clone(),
        project: root,
        title: "Second".into(),
        created: None,
        modified: std::time::SystemTime::now(),
    });
    app.remember_navigation();
    app.navigate_history(false);
    assert_eq!(app.state.session_file.as_deref(), first_path.to_str());
    assert_eq!(app.draft, "Disconnected unsent draft");
    assert_eq!(app.draft_images.images().len(), 1);
    app.shutdown_agents();
}

#[test]
fn focused_browser_keeps_keyboard_page_history_inputs() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.files.toggle_browser(&ctx, &app.prefs.cwd);
    app.remember_navigation();
    let cursor = app.navigation.cursor;
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::OpenBracket,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: if cfg!(target_os = "macos") {
                    egui::Modifiers::MAC_CMD
                } else {
                    egui::Modifiers::ALT
                },
            }],
            ..Default::default()
        },
        |ctx| {
            app.navigation_inputs(ctx);
            assert!(ctx.input(|input| input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::OpenBracket,
                        ..
                    }
                )
            })));
        },
    );
    assert_eq!(app.navigation.cursor, cursor);
    assert!(app.files.browser_open());
}

#[test]
fn different_persisted_chats_are_distinct_even_with_the_same_reused_agent_id() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.session_file = Some("/fixture/first".into());
    app.ensure_agent();
    let first = app.navigation_location();
    app.state.session_file = Some("/fixture/second".into());
    let second = app.navigation_location();
    assert!(!first.same(&second));
    app.navigation.observe(first);
    app.navigation.observe(second);
    assert_eq!(app.navigation.entries.len(), 2);
}

#[test]
fn a_file_pane_on_a_task_restores_the_task_underneath_it() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.demo_task();
    let selected = app.tasks.selected.clone();
    app.files.toggle(&ctx, &app.prefs.cwd);
    app.remember_navigation();
    app.open_agents_board();
    app.remember_navigation();
    app.navigate_history(false);
    assert!(app.files.files_open());
    assert!(app.tasks.open);
    assert_eq!(app.tasks.selected, selected);
    app.files.close();
    assert!(matches!(app.navigation_location().page, Page::Task { .. }));
}

#[test]
fn workflow_tab_survives_back_forward_and_file_overlay() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.demo_task();
    let id = app.tasks.selected.clone().unwrap();
    app.open_work_item_tab(id.clone(), tasks::work_item::Tab::Evidence);
    app.remember_navigation();
    app.open_agents_board();
    app.remember_navigation();
    app.navigate_history(false);
    assert_eq!(app.work_item_tab(&id), tasks::work_item::Tab::Evidence);
    app.open_work_item_tab(id.clone(), tasks::work_item::Tab::Changes);
    app.files.toggle(&ctx, &app.prefs.cwd);
    app.remember_navigation();
    app.open_agents_board();
    app.remember_navigation();
    app.navigate_history(false);
    assert!(app.files.files_open());
    assert_eq!(app.work_item_tab(&id), tasks::work_item::Tab::Changes);
}

#[test]
fn narrow_titlebar_keeps_history_controls_inside_the_window() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.prefs.theme = theme;
        app.loading_chat = true;
        app.open_agents_board();
        for _ in 0..3 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(860.0, 700.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::SidePanel::left("wide-sidebar-fixture")
                        .exact_width(480.0)
                        .show(ctx, |_| {});
                    app.top_bar(ctx);
                },
            );
        }
        let rects: Vec<_> = [false, true]
            .into_iter()
            .map(|forward| {
                ctx.data(|data| {
                    data.get_temp::<egui::Rect>(egui::Id::new(("view-history", forward)))
                        .unwrap()
                })
            })
            .collect();
        assert!(
            rects
                .iter()
                .all(|rect| rect.left() >= 480.0 && rect.right() <= 860.0)
        );
        assert!(!rects[0].intersects(rects[1]));
    }
}

#[test]
fn history_has_a_fixed_bound() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    for index in 0..200 {
        app.settings_open = true;
        app.settings_view.select(if index % 2 == 0 {
            "General"
        } else {
            "Appearance"
        });
        app.remember_navigation();
    }
    assert_eq!(app.navigation.entries.len(), MAX_HISTORY);
    assert_eq!(app.navigation.cursor, MAX_HISTORY - 1);
}

#[test]
fn breadcrumb_chat_link_replaces_the_ad_hoc_back_to_chat_buttons() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.open_agents_board();
    frame(&mut app, &ctx, vec![]);
    let output = frame(&mut app, &ctx, vec![]);
    fn texts(shape: &egui::Shape, text: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(shape) => text.push(shape.galley.text().into()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    texts(shape, text);
                }
            }
            _ => {}
        }
    }
    let mut text = vec![];
    for shape in output.shapes {
        texts(&shape.shape, &mut text);
    }
    assert!(text.iter().any(|text| text == "Chat"));
    assert!(text.iter().any(|text| text == "Agents board"));
    assert!(!text.iter().any(|text| text == "Back to chat"));
    let back = ctx.data(|data| {
        *data
            .get_temp::<egui::Rect>(egui::Id::new(("view-history", false)))
            .as_ref()
            .unwrap()
    });
    for pressed in [true, false] {
        frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(back.center()),
                egui::Event::PointerButton {
                    pos: back.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(!app.kanban.open);
}
