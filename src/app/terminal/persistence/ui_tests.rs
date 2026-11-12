use super::*;
use crate::app::files::tests::{app, click, frame, text};

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
fn restored_app(ctx: &egui::Context) -> Pui {
    let mut app = app(ctx);
    let root = app.prefs.cwd.clone();
    let saved: SavedTabs = serde_json::from_value(json!({"projects":[
        {"root":root,"labels":[2,4,7],"selected":4},
        {"root":root.join("src"),"labels":[3,8],"selected":8}
    ]}))
    .unwrap();
    app.terminals.restore_tabs(&saved);
    app.terminals
        .restore_saved_layout(ctx, &root, (true, false));
    for _ in 0..3 {
        frame(ctx, &mut app, vec![]);
    }
    app
}

#[test]
fn legacy_start_replaces_reserved_capacity_at_both_caps_but_unknown_root_is_disabled() {
    for full_groups in [false, true] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let root = app.prefs.cwd.clone();
        app.terminals
            .restore_saved_layout(&ctx, &root, (true, false));
        let mut projects = Vec::new();
        if full_groups {
            for index in 0..31 {
                projects.push(
                    json!({"root":root.join(format!("saved-{index}")),"labels":[1],"selected":1}),
                );
            }
        } else {
            for index in 0..4 {
                let count = if index == 3 { 7 } else { 8 };
                projects.push(json!({"root":root.join(format!("saved-{index}")),"labels":(1..=count).collect::<Vec<_>>(),"selected":1}));
            }
        }
        let saved: SavedTabs = serde_json::from_value(json!({"projects":projects})).unwrap();
        app.terminals.restore_tabs(&saved);
        assert_eq!(app.terminals.total_tabs(), tabs::MAX_TOTAL_TABS);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let (_, start, enabled) = ctx
            .data(|data| {
                data.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new(
                    "terminal-legacy-start",
                ))
            })
            .unwrap();
        assert!(
            enabled,
            "selected legacy root owns a reserved tab/group slot"
        );
        click(&ctx, &mut app, start.center());
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.terminals.total_tabs(), tabs::MAX_TOTAL_TABS);
        assert_eq!(app.terminals.projects[&root].tabs.len(), 1);
        assert!(app.terminals.projects[&root].tabs[0].demo.is_some());
        assert!(!app.terminals.restored_roots.contains(&root));
        if full_groups {
            assert_eq!(app.terminals.projects.len(), tabs::MAX_TOTAL_TABS);
        }
        let unknown = root.join("unrelated-unknown");
        app.prefs.cwd = unknown.clone();
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let (_, start, enabled) = ctx
            .data(|data| {
                data.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new(
                    "terminal-unknown-start",
                ))
            })
            .unwrap();
        assert!(
            !enabled,
            "an unrelated root cannot borrow the occupied reservation"
        );
        click(&ctx, &mut app, start.center());
        assert!(!app.terminals.projects.contains_key(&unknown));
        assert_eq!(app.terminals.total_tabs(), tabs::MAX_TOTAL_TABS);
        assert!(
            !app.terminals.running(),
            "demo-only UI fixture must never spawn a process"
        );
    }
}

#[test]
fn clicking_start_activates_only_selected_placeholder_and_ctrl_tab_never_executes_or_answers_approval()
 {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let mut app = restored_app(&ctx);
        app.prefs.theme = mode;
        let root = app.prefs.cwd.clone();
        let ids = app.terminals.projects[&root]
            .tabs
            .iter()
            .map(|tab| tab.id)
            .collect::<Vec<_>>();
        assert_eq!(app.terminals.projects[&root].selected, 1);
        assert_eq!(app.terminals.total_tabs(), 5);
        assert!(!app.terminals.running());
        let output = frame(&ctx, &mut app, vec![]);
        text(&output, "No commands or processes were replayed");
        let start = ctx
            .data(|data| {
                data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("terminal-restored-start"))
            })
            .unwrap();
        click(&ctx, &mut app, start.1.center());
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.terminals.projects[&root].tabs[1].id, ids[1]);
        assert!(app.terminals.projects[&root].tabs[1].demo.is_some());
        assert!(!app.terminals.projects[&root].tabs[1].restored);
        assert!(app.terminals.projects[&root].tabs[0].restored);
        assert!(app.terminals.projects[&root].tabs[2].restored);
        assert_eq!(app.terminals.total_tabs(), 5);
        app.state.busy = true;
        app.draft = "keep draft".into();
        app.state.dialogs.push_back(json!({"id":"restored-approval","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo safe\"}\n\nApprove this call only."}));
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::CTRL)],
        );
        assert_eq!(app.terminals.projects[&root].selected, 2);
        assert!(
            ctx.memory(|memory| memory.has_focus(app.terminals.projects[&root].tabs[2].input_id()))
        );
        frame(
            &ctx,
            &mut app,
            vec![
                key(egui::Key::Escape, egui::Modifiers::NONE),
                key(egui::Key::Enter, egui::Modifiers::COMMAND),
                key(egui::Key::Backspace, egui::Modifiers::COMMAND),
            ],
        );
        assert!(app.terminals.projects[&root].tabs[2].restored);
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(app.state.busy && commands.try_recv().is_err());
        assert_eq!(app.draft, "keep draft");
        let disabled = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("terminal-context-button")))
            .unwrap();
        click(&ctx, &mut app, disabled.center());
        assert!(
            !egui::Popup::is_any_open(&ctx),
            "placeholder output cannot become capture context"
        );
        app.terminals.close(&ctx);
        app.terminals.toggle(&ctx, &root, true);
        app.prefs.cwd = root.join("src");
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        assert!(
            app.terminals.projects[&app.prefs.cwd]
                .tabs
                .iter()
                .all(|tab| tab.restored)
        );
        app.prefs.cwd = root.clone();
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.terminals.projects[&root].tabs[1].id, ids[1]);
        assert!(app.terminals.projects[&root].tabs[1].demo.is_some());
        assert_eq!(app.state.dialogs.len(), 1);
    }
}

#[test]
fn focused_start_button_keys_are_owned_and_space_explicitly_starts_only_that_tab() {
    let ctx = egui::Context::default();
    let mut app = restored_app(&ctx);
    let root = app.prefs.cwd.clone();
    let (id, _) = ctx
        .data(|data| {
            data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("terminal-restored-start"))
        })
        .unwrap();
    ctx.memory_mut(|memory| {
        memory.request_focus_with_filter(
            id,
            egui::EventFilter {
                escape: true,
                ..Default::default()
            },
        )
    });
    app.state.busy = true;
    app.draft = "keep".into();
    app.state.dialogs.push_back(json!({"id":"start-approval","method":"confirm","title":"Allow bash?","message":"Approve this call only."}));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
            key(egui::Key::Escape, egui::Modifiers::NONE),
        ],
    );
    assert!(
        app.terminals.projects[&root]
            .tabs
            .iter()
            .all(|tab| tab.restored)
    );
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.state.busy && commands.try_recv().is_err());
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Space, egui::Modifiers::NONE),
            egui::Event::Text(" ".into()),
        ],
    );
    assert!(app.terminals.projects[&root].tabs[1].demo.is_some());
    assert!(app.terminals.projects[&root].tabs[0].restored);
    assert!(app.terminals.projects[&root].tabs[2].restored);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(commands.try_recv().is_err());
    assert_eq!(app.draft, "keep");
}

#[test]
fn restored_rail_keeps_compact_geometry_in_narrow_themes_zoom_and_settles_without_timers() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let root = Path::new("/fixture/long/project/name");
            let saved: SavedTabs = serde_json::from_value(
                json!({"projects":[{"root":root,"labels":[1,2,3,4,5,6,7,8],"selected":4}]}),
            )
            .unwrap();
            let mut terminal = Terminals::default();
            terminal.restore_tabs(&saved);
            terminal.restore_saved_layout(&ctx, root, (true, false));
            let mut out = egui::FullOutput::default();
            for index in 0..45 {
                out = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(390.0, 650.0),
                        )),
                        time: Some(index as f64 * 0.1),
                        ..Default::default()
                    },
                    |ctx| {
                        terminal.context_inputs(ctx, true);
                        terminal.show(ctx, root, true);
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.label("Chat remains visible");
                        });
                    },
                );
            }
            let rail = ctx
                .data(|data| {
                    data.get_temp::<egui::Rect>(egui::Id::new("terminal-shell-rail-bounds"))
                })
                .unwrap();
            let heading = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("terminal-heading-bounds")))
                .unwrap();
            let (_, start) = ctx
                .data(|data| {
                    data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new(
                        "terminal-restored-start",
                    ))
                })
                .unwrap();
            assert!((104.0..=129.0).contains(&rail.width()));
            assert!(ctx.screen_rect().contains_rect(heading));
            assert!(
                ctx.screen_rect().contains_rect(start),
                "{mode:?}/{zoom}: {start:?}"
            );
            assert!(start.left() > rail.right());
            let (_, first, _) = ctx
                .data(|data| {
                    data.get_temp::<(egui::Id, egui::Rect, u64)>(egui::Id::new(("terminal-tab", 0)))
                })
                .unwrap();
            let (_, second, _) = ctx
                .data(|data| {
                    data.get_temp::<(egui::Id, egui::Rect, u64)>(egui::Id::new(("terminal-tab", 1)))
                })
                .unwrap();
            assert!(first.height() <= 28.0 && second.height() <= 28.0);
            assert!(second.top() >= first.bottom());
            assert!(
                terminal.projects[root]
                    .tabs
                    .iter()
                    .all(|tab| tab.restored && tab.starting.is_none() && tab.session.is_none())
            );
            assert!(
                out.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(1)
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn actual_fixture_input_and_exact_stop_confirmation_survive_checkpoint_hydration() {
    let ctx = egui::Context::default();
    let mut app = restored_app(&ctx);
    let root = app.prefs.cwd.clone();
    let session = Session::fixture(&ctx, &root).unwrap();
    let state = session.state.clone();
    let id = app.terminals.projects[&root].tabs[1].id;
    let tab = &mut app.terminals.projects.get_mut(&root).unwrap().tabs[1];
    tab.restored = false;
    tab.session = Some(session);
    let checkpoint = app.terminals.saved_tabs();
    app.terminals.restore_tabs(&checkpoint);
    assert_eq!(app.terminals.projects[&root].tabs[1].id, id);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    app.terminals.projects[&root].tabs[1].focus(&ctx);
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::Text("printf '%s%s\\n' RESTORED_ FIXTURE_INPUT".into()),
            key(egui::Key::Enter, egui::Modifiers::NONE),
        ],
    );
    let wait = |predicate: &dyn Fn() -> bool| {
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while !predicate() {
            assert!(std::time::Instant::now() < deadline, "fixture PTY timeout");
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    wait(&|| {
        state
            .lock()
            .unwrap()
            .parser
            .screen()
            .contents()
            .contains("RESTORED_FIXTURE_INPUT")
    });
    app.terminals.confirm_stop = Some((root.clone(), id));
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    click(&ctx, &mut app, text(&output, "Keep running"));
    assert!(state.lock().unwrap().exited.is_none());
    assert!(app.terminals.confirm_stop.is_none());
    app.terminals.projects.get_mut(&root).unwrap().selected = 0;
    app.terminals.confirm_stop = Some((root.clone(), id));
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    click(&ctx, &mut app, text(&output, "Stop terminal"));
    wait(&|| state.lock().unwrap().exited.is_some());
    assert_eq!(app.terminals.projects[&root].tabs.len(), 2);
    assert!(
        app.terminals.projects[&root]
            .tabs
            .iter()
            .all(|tab| tab.restored && tab.id != id)
    );
    assert!(app.terminals.confirm_stop.is_none());
}
