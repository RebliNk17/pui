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
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
fn frame(
    ctx: &egui::Context,
    app: &mut Pui,
    events: Vec<egui::Event>,
    size: Vec2,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            app.search_inputs(ctx);
            app.sidebar(ctx);
            egui::CentralPanel::default().show(ctx, |_| {});
            app.spotlight(ctx);
        },
    )
}
#[test]
fn result_rows_are_single_line_compact_and_metadata_is_on_the_same_baseline() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        mode.apply(&ctx);
        let mut rows = Vec::new();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 400.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (i, kind) in [Kind::Setting, Kind::Chat, Kind::Message]
                        .into_iter()
                        .enumerate()
                    {
                        let hit = Hit {
                            candidate: Candidate {
                                title: format!("Result {i}"),
                                project: PathBuf::from("/fixture/pui"),
                                cwd: PathBuf::new(),
                                path: None,
                                target: sidebar_tree::Target::Agent("a".into()),
                            },
                            detail: if kind == Kind::Setting {
                                "Settings · General".into()
                            } else {
                                "A matching message excerpt".into()
                            },
                            kind,
                            message: None,
                        };
                        rows.push(result_row(ui, &hit, i == 0).rect);
                    }
                });
            },
        );
        assert!(rows.iter().all(|r| r.height() == 30.0));
        assert_eq!(rows[1].top() - rows[0].bottom(), 2.0);
        assert_eq!(rows[2].top() - rows[1].bottom(), 2.0);
        for shape in output.shapes {
            if let egui::Shape::Text(text) = shape.shape {
                assert_eq!(text.galley.rows.len(), 1);
                assert!(
                    rows.iter()
                        .any(|row| row.contains(text.pos + text.galley.size() / 2.0))
                );
            }
        }
    }
}

#[test]
fn spotlight_preserves_sidebar_width_in_both_themes_and_zoom() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.4] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.prefs.color_styles.apply(&ctx, theme);
            ctx.set_zoom_factor(zoom);
            for width in [220.0, 254.0, 360.0] {
                let id = egui::Id::new("sessions-v4");
                ctx.data_mut(|data| {
                    data.insert_persisted(
                        id,
                        egui::containers::panel::PanelState {
                            rect: egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 800.0),
                            ),
                        },
                    )
                });
                let size = egui::vec2(1000.0, 800.0);
                frame(&ctx, &mut app, vec![], size);
                let before = egui::containers::panel::PanelState::load(&ctx, id)
                    .unwrap()
                    .rect
                    .width();
                app.open_search();
                for _ in 0..3 {
                    frame(&ctx, &mut app, vec![], size);
                }
                assert!(app.search_open);
                assert_eq!(
                    egui::containers::panel::PanelState::load(&ctx, id)
                        .unwrap()
                        .rect
                        .width(),
                    before
                );
                frame(&ctx, &mut app, vec![key(egui::Key::Escape)], size);
                assert!(!app.search_open);
                assert_eq!(
                    egui::containers::panel::PanelState::load(&ctx, id)
                        .unwrap()
                        .rect
                        .width(),
                    before
                );
            }
        }
    }
}
#[test]
fn settings_labels_and_aliases_are_searchable_without_values() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.spotlight.scope = Scope::Settings;
    for (query, title) in [
        ("sound", "Completion sound"),
        ("zoom", "Interface scale"),
        ("mcp", "MCP servers"),
        ("auto compact", "Auto compact at"),
        ("permission", "Permissions"),
    ] {
        app.search = query.into();
        app.build_search_hits();
        assert!(
            app.spotlight
                .hits
                .iter()
                .any(|h| h.candidate.title == title),
            "{query}"
        );
    }
    app.search = "sound".into();
    app.build_search_hits();
    let hit = app
        .spotlight
        .hits
        .iter()
        .find(|h| h.candidate.title == "Completion sound")
        .unwrap()
        .clone();
    let draft = "preserve the draft";
    app.draft = draft.into();
    let notifications = app.prefs.notifications;
    app.remember_navigation();
    app.activate_search_hit(hit);
    assert!(app.settings_open);
    assert_eq!(app.settings_view.location(), "Notifications");
    assert_eq!(app.draft, draft);
    assert!(app.prefs.notifications == notifications);
    let size = egui::vec2(1000.0, 800.0);
    frame(&ctx, &mut app, vec![], size);
    frame(&ctx, &mut app, vec![], size);
    app.navigate_history(false);
    assert!(!app.settings_open);
    assert_eq!(app.settings_view.location(), "Notifications");
}
#[test]
fn hidden_chats_are_not_searched_and_query_does_not_filter_sidebar() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let path = app.sessions[0].path.clone();
    app.prefs.chats.entry(path.clone()).or_default().hidden = true;
    assert!(
        !app.search_candidates()
            .iter()
            .any(|c| c.path.as_ref() == Some(&path))
    );
    app.search = "nothing could match this".into();
    app.build_search_hits();
    assert!(app.spotlight.hits.is_empty());
    assert!(!app.search_candidates().is_empty());
}
#[test]
fn blocked_search_never_hides_approvals_or_changes_settings() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state
        .dialogs
        .push_back(json!({"id":"approval","method":"select"}));
    app.open_search();
    assert!(!app.search_open);
    assert_eq!(app.state.dialogs.len(), 1);
    app.spotlight.scope = Scope::Settings;
    app.search = "sound".into();
    app.build_search_hits();
    let hit = app.spotlight.hits[0].clone();
    app.activate_search_hit(hit);
    assert!(!app.settings_open);
}
#[test]
fn stale_message_results_are_discarded_on_query_or_scope_change() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (tx, rx) = mpsc::channel();
    app.search = "old".into();
    app.spotlight.requested = Some(("old".into(), Scope::All, app.search_revision()));
    app.spotlight.rx = Some(rx);
    app.search = "new".into();
    app.spotlight.scope = Scope::Settings;
    app.refresh_search(&ctx);
    assert!(tx.send(Default::default()).is_err());
    assert!(app.spotlight.messages.hits.is_empty());
}

#[test]
fn opening_search_claims_focus_before_composer_can_send_in_the_same_frame() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.draft = "Do not send this".into();
    let mut window = eframe::Frame::_new_kittest();
    let size = egui::vec2(1400.0, 900.0);
    let mut run = |app: &mut Pui, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ctx| eframe::App::update(app, ctx, &mut window),
        )
    };
    for _ in 0..3 {
        run(&mut app, vec![]);
    }
    let before = app.state.messages.len();
    ctx.memory_mut(|m| m.request_focus(egui::Id::new("composer")));
    let mut command_k = key(egui::Key::K);
    if let egui::Event::Key { modifiers, .. } = &mut command_k {
        *modifiers = egui::Modifiers::COMMAND;
    }
    run(
        &mut app,
        vec![
            command_k,
            egui::Event::Text("zoom".into()),
            key(egui::Key::Enter),
        ],
    );
    assert_eq!(app.draft, "Do not send this");
    assert_eq!(app.state.messages.len(), before);
}
#[test]
fn spotlight_keyboard_opens_setting_and_backdrop_cancels_without_navigation() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let size = egui::vec2(1000.0, 800.0);
    app.spotlight.scope = Scope::Settings;
    app.search = "zoom".into();
    app.open_search();
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![], size);
    }
    frame(&ctx, &mut app, vec![key(egui::Key::ArrowDown)], size);
    assert_eq!(app.spotlight.selected, 1);
    frame(&ctx, &mut app, vec![key(egui::Key::Enter)], size);
    assert!(app.settings_open);
    assert_eq!(app.settings_view.location(), "Appearance");
    app.open_search();
    for _ in 0..2 {
        frame(&ctx, &mut app, vec![], size);
    }
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(egui::pos2(3.0, 3.0)),
                egui::Event::PointerButton {
                    pos: egui::pos2(3.0, 3.0),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            size,
        );
    }
    assert!(!app.search_open);
    assert_eq!(app.settings_view.location(), "Appearance");
}
#[test]
fn live_branch_revision_and_visibility_changes_invalidate_workers() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.search = "context".into();
    let old = app.search_revision();
    app.spotlight.requested = Some(("context".into(), Scope::All, old));
    let (tx, rx) = mpsc::channel();
    app.spotlight.rx = Some(rx);
    app.state.transcript_revision += 1;
    app.spotlight.changed_at = 99999.0;
    app.refresh_search(&ctx);
    assert!(tx.send(Default::default()).is_err());
    let revision = app.search_revision();
    let path = app.sessions[0].path.clone();
    app.prefs.chats.entry(path).or_default().hidden = true;
    assert_ne!(revision, app.search_revision());
}

#[test]
fn message_result_opens_current_owner_without_prompt_or_abort() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.session_file = Some("/fixture/live-search.jsonl".into());
    app.state.messages = vec![Message {
        role: "user".into(),
        text: "Find the matching passage".into(),
        timestamp: 42,
        ..Default::default()
    }];
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.connected = true;
    app.state.busy = true;
    app.ensure_agent();
    let owner = app.agents.active.clone();
    app.draft = "unsent draft".into();
    let candidate = app
        .search_candidates()
        .into_iter()
        .find(|c| c.path.as_deref() == Some(std::path::Path::new("/fixture/live-search.jsonl")))
        .unwrap();
    let found = index::search(
        app.search_sources(vec![candidate]),
        "matching",
        &Arc::new(AtomicBool::new(false)),
    );
    assert_eq!(found.hits.len(), 1);
    app.activate_search_hit(found.hits[0].clone());
    app.search_jump();
    assert_eq!(app.agents.active, owner);
    assert!(app.client.is_some() && app.state.busy);
    assert_eq!(app.draft, "unsent draft");
    assert!(app.spotlight.jump.is_none());
    assert!(commands.try_iter().all(|r| !matches!(
        r["type"].as_str(),
        Some("prompt" | "abort" | "switch_session")
    )));
}
#[test]
fn spotlight_bounds_fit_narrow_windows_and_long_unbroken_titles() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.4] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.prefs.color_styles.apply(&ctx, theme);
            ctx.set_zoom_factor(zoom);
            app.sessions[0].title = "unbroken".repeat(300);
            app.spotlight.scope = Scope::Chats;
            app.open_search();
            let size = egui::vec2(360.0, 560.0);
            for _ in 0..3 {
                frame(&ctx, &mut app, vec![], size);
            }
            let rect = egui::AreaState::load(&ctx, egui::Id::new("spotlight-search"))
                .unwrap()
                .rect();
            assert!(
                rect.left() >= 0.0 && rect.right() <= size.x + 1.0,
                "{rect:?}, zoom {zoom}"
            );
            assert!(
                rect.top() >= 0.0 && rect.bottom() <= size.y + 1.0,
                "{rect:?}, zoom {zoom}"
            );
        }
    }
}
