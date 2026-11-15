use super::*;

fn fixture() -> Pui {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.sessions.clear();
    app.state = State {
        session_file: Some("/fixture/unread-parent.jsonl".into()),
        ..Default::default()
    };
    app.demo_agent_tree();
    app.projects = false;
    app.prefs.sidebar_view.group = sidebar_view::Group::None;
    for node in app.agents.nodes.values_mut() {
        node.name =
            "Long Hebrew שלום and English title that must elide independently of unread and age"
                .into();
        node.status = "Idle".into();
        node.last_activity = Some(std::time::SystemTime::now() - Duration::from_secs(300));
    }
    app
}
fn render(app: &mut Pui, events: Vec<egui::Event>) -> egui::FullOutput {
    app.prefs.color_styles.apply(&app.ctx, app.prefs.theme);
    app.ctx.clone().run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(480.0, 780.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| app.sidebar(ctx),
    )
}
fn row_rect(app: &Pui, key: &str) -> egui::Rect {
    app.ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("sidebar-row-rect", key))))
        .unwrap()
}
fn marker(out: &egui::FullOutput, row: egui::Rect) -> Option<egui::Rect> {
    out.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Rect(rect)
            if rect.rect.size() == egui::vec2(3.0, 7.0) && row.contains_rect(rect.rect) =>
        {
            Some(rect.rect)
        }
        _ => None,
    })
}

#[test]
fn exact_owner_row_never_inherits_children_and_history_does_not_become_unread() {
    let mut app = fixture();
    let root = app.agents.active.clone();
    let child = app.agents.nodes[&root].children[0].clone();
    app.attention.observe(
        Owner::Runtime(child.clone()),
        &app.prefs.cwd,
        Kind::NeedsInput,
    );
    let rows = sidebar_tree::forest(&app);
    let parent = rows
        .iter()
        .find(|row| matches!(&row.target, sidebar_tree::Target::Agent(id) if id == &root))
        .unwrap();
    assert!(!app.sidebar_unread(parent));
    assert!(app.sidebar_unread(&parent.children[0]));
    assert!(!app.sidebar_unread(&parent.children[1]));
    assert!(!app.saved_unread(Path::new("/fixture/unknown-history.jsonl")));
    let alias = Path::new("/fixture/./promoted.jsonl");
    app.attention.promote(&child, alias);
    app.agents.nodes.get_mut(&child).unwrap().path = Some(alias.into());
    let rows = sidebar_tree::forest(&app);
    let parent = rows
        .iter()
        .find(|row| matches!(&row.target, sidebar_tree::Target::Agent(id) if id == &root))
        .unwrap();
    assert!(!app.sidebar_unread(parent));
    assert!(app.sidebar_unread(&parent.children[0]));
    app.attention
        .mark_read(&Owner::Saved(lexical_identity(alias)));
    let snapshot = app.attention.metadata(alias).unwrap().unread_snapshot();
    app.attention.import_for(alias, snapshot);
    assert!(
        !app.sidebar_unread(&parent.children[0]),
        "Mirrored same revision preserves read"
    );
    app.attention.observe(
        Owner::Saved(lexical_identity(alias)),
        &app.prefs.cwd,
        Kind::Completed,
    );
    assert!(app.sidebar_unread(&parent.children[0]));
    app.attention.forget_path(alias);
    assert!(!app.sidebar_unread(&parent.children[0]));
    app.attention
        .observe(Owner::Runtime(child.clone()), &app.prefs.cwd, Kind::Failed);
    app.attention.forget_agents(&[child].into_iter().collect());
    assert_eq!(app.attention.unread(), 0);
}

#[test]
fn unread_own_lane_survives_disclosure_title_elision_age_and_narrow_theme_zoom() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.4] {
            for metadata in [false, true] {
                let mut app = fixture();
                app.prefs.theme = theme;
                app.prefs.sidebar_view.metadata = metadata;
                app.ctx.set_zoom_factor(zoom);
                app.attention.observe(
                    Owner::Saved("/fixture/unread-parent.jsonl".into()),
                    &app.prefs.cwd,
                    Kind::Completed,
                );
                let root = app.agents.active.clone();
                let child = app.agents.nodes[&root].children[0].clone();
                app.attention
                    .observe(Owner::Runtime(child), &app.prefs.cwd, Kind::NeedsInput);
                for _ in 0..5 {
                    render(&mut app, vec![]);
                }
                let rows = sidebar_tree::forest(&app);
                let parent = rows
                    .iter()
                    .find(
                        |row| matches!(&row.target, sidebar_tree::Target::Agent(id) if id == &root),
                    )
                    .unwrap();
                let row = row_rect(&app, &parent.key);
                let child_row = row_rect(&app, &parent.children[0].key);
                let out = render(&mut app, vec![]);
                let unread = marker(&out, row).expect("Own parent marker");
                assert!(marker(&out, child_row).is_some());
                assert!(marker(&out, row_rect(&app, &parent.children[1].key)).is_none());
                let disclosure = app
                    .ctx
                    .data(|data| {
                        data.get_temp::<egui::Rect>(egui::Id::new((
                            "sidebar-disclosure-rect",
                            &parent.key,
                        )))
                    })
                    .unwrap();
                assert!(unread.left() > disclosure.right());
                assert!(unread.right() < row.left() + 32.0);
                assert!(row.right() - 68.0 > unread.right());
                let out = render(&mut app, vec![egui::Event::PointerMoved(row.center())]);
                assert_eq!(
                    marker(&out, row),
                    Some(unread),
                    "Hover disclosure does not hide unread"
                );
                assert!(out.shapes.iter().any(|s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text == "5m" && t.pos.x > unread.right())));
                assert_eq!(row_rect(&app, &parent.key), row);
                assert_eq!(child_row.height(), 28.0);
                assert!((child_row.top() - row.bottom() - 2.0).abs() < 0.1);
                assert_eq!(
                    app.attention.unread(),
                    2,
                    "Rendering and hovering never mark read"
                );
            }
        }
    }
}

#[test]
fn saved_session_root_uses_same_marker_lane_and_unknown_saved_history_stays_blank() {
    let mut app = fixture();
    let path = PathBuf::from("/fixture/unread-parent.jsonl");
    app.sessions.push(Session {
        path: path.clone(),
        cwd: app.prefs.cwd.clone(),
        project: app.prefs.cwd.clone(),
        title: "Saved root with unread observation".into(),
        created: None,
        modified: std::time::SystemTime::now() - Duration::from_secs(300),
    });
    for _ in 0..4 {
        render(&mut app, vec![]);
    }
    let branch = sidebar_tree::forest(&app)
        .into_iter()
        .find(|row| row.path.as_ref() == Some(&path))
        .unwrap();
    assert!(matches!(branch.target, sidebar_tree::Target::Session(_)));
    let row = row_rect(&app, &branch.key);
    assert!(marker(&render(&mut app, vec![]), row).is_none());
    app.attention
        .observe(Owner::Saved(path.clone()), &app.prefs.cwd, Kind::Completed);
    let marked = render(&mut app, vec![]);
    let unread = marker(&marked, row).expect("Saved session own unread");
    assert_eq!(unread.center().x, row.left() + 23.0);
    let hover = render(&mut app, vec![egui::Event::PointerMoved(row.center())]);
    assert_eq!(marker(&hover, row), Some(unread));
    app.attention.mark_read(&Owner::Saved(path));
    assert!(marker(&render(&mut app, vec![]), row).is_none());
    assert_eq!(row_rect(&app, &branch.key), row);
}

#[test]
fn marked_row_pointer_and_disclosure_keyboard_do_not_answer_or_stop_in_full_app() {
    let mut app = fixture();
    app.demo = false;
    app.state.busy = true;
    app.draft = "Retained draft".into();
    app.focus_composer = false;
    app.state.dialogs.push_back(json!({"type":"extension_ui_request","method":"confirm","id":"unread-approval","title":"Allow bash?","message":"{}\n\nApprove this call only."}));
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.attention.observe(
        Owner::Saved("/fixture/unread-parent.jsonl".into()),
        &app.prefs.cwd,
        Kind::NeedsInput,
    );
    let root = app.agents.active.clone();
    let ctx = app.ctx.clone();
    for _ in 0..4 {
        crate::app::files::tests::frame(&ctx, &mut app, vec![]);
    }
    let branch = sidebar_tree::forest(&app)
        .into_iter()
        .find(|row| matches!(&row.target,sidebar_tree::Target::Agent(id) if id == &root))
        .unwrap();
    let row = row_rect(&app, &branch.key);
    let toggle = ctx
        .data(|data| {
            data.get_temp::<egui::Rect>(egui::Id::new(("sidebar-disclosure-rect", &branch.key)))
        })
        .unwrap();
    for pressed in [true, false] {
        crate::app::files::tests::frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(toggle.center()),
                egui::Event::PointerButton {
                    pos: toggle.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(app.prefs.closed_threads.contains(&branch.key));
    crate::app::files::tests::frame(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(!app.prefs.closed_threads.contains(&branch.key));
    let marker_center = egui::pos2(row.left() + 23.0, row.center().y);
    app.attention.observe(
        Owner::Saved("/fixture/unread-parent.jsonl".into()),
        &app.prefs.cwd,
        Kind::NeedsInput,
    );
    for pressed in [true, false] {
        let out = crate::app::files::tests::frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(marker_center),
                egui::Event::PointerButton {
                    pos: marker_center,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        if pressed {
            assert!(
                marker(&out, row).is_some(),
                "Pointer used an actual decorated row"
            );
        }
    }
    assert_eq!(app.agents.active, root);
    assert_eq!(app.draft, "Retained draft");
    assert!(app.state.busy && app.state.dialogs.len() == 1 && app.pending_dialog.is_none());
    assert!(!commands.try_iter().any(|record| matches!(
        protocol::string(&record, "type"),
        "prompt" | "abort" | "extension_ui_response"
    )));
    app.shutdown_agents();
}

#[test]
fn visible_read_transition_refreshes_sidebar_once_then_settles() {
    let mut app = fixture();
    let ctx = app.ctx.clone();
    app.loading_chat = false;
    app.ensure_agent();
    app.observe_attention(Kind::Completed);
    let first = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.attention_rendered(ui));
    });
    assert_eq!(app.attention.unread(), 0);
    assert_eq!(
        first.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
        Duration::ZERO
    );
    let mut output = None;
    for _ in 0..4 {
        output = Some(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.attention_rendered(ui));
        }));
    }
    assert_eq!(
        output.unwrap().viewport_output[&egui::ViewportId::ROOT].repaint_delay,
        Duration::MAX
    );
}

#[test]
fn static_unread_marker_adds_no_repaint_timer() {
    let ctx = egui::Context::default();
    let mut out = None;
    for _ in 0..4 {
        out = Some(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                paint_unread(
                    ui,
                    egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(220.0, 28.0)),
                    14.0,
                    true,
                )
            });
        }));
    }
    assert_eq!(
        out.unwrap().viewport_output[&egui::ViewportId::ROOT].repaint_delay,
        Duration::MAX
    );
}
