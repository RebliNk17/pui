use super::*;
fn fixture() -> (egui::Context, Pui) {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.prefs.hide_sidebar = true;
    app.focus_composer = false;
    app.spotlight.scope = Scope::Chats;
    (ctx, app)
}
fn event(app: &mut Pui, path: &PathBuf, kind: &str, at: u64) {
    let metadata:attention::Metadata=serde_json::from_value(json!({"path":path,"cwd":app.prefs.cwd,"revision":uuid::Uuid::new_v4().to_string(),"kind":kind,"at":at,"read":false})).unwrap();
    app.attention.import(metadata);
}
fn time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
fn add(app: &mut Pui, name: &str) -> PathBuf {
    let path = app.prefs.cwd.join(format!("{name}.jsonl"));
    std::fs::write(&path, "{\"type\":\"session\"}\n").unwrap();
    app.sessions.push(Session {
        path: path.clone(),
        cwd: app.prefs.cwd.clone(),
        project: app.prefs.cwd.clone(),
        title: name.into(),
        created: None,
        modified: std::time::SystemTime::now(),
    });
    path
}
fn titles(app: &Pui) -> Vec<String> {
    app.spotlight
        .hits
        .iter()
        .filter(|hit| hit.kind == Kind::Chat)
        .map(|hit| hit.candidate.title.clone())
        .collect()
}
#[test]
fn unread_event_priority_and_recency_before_twenty_limit_preserve_matching_and_hidden_rules() {
    let (_, mut app) = fixture();
    app.sessions.clear();
    for n in 0..24 {
        add(&mut app, &format!("ordinary-{n}"));
    }
    let completed = add(&mut app, "completed");
    let failed = add(&mut app, "failed");
    let older = add(&mut app, "older-needs");
    let needs = add(&mut app, "needs");
    let hidden = add(&mut app, "hidden-needs");
    app.prefs.chats.entry(hidden.clone()).or_default().hidden = true;
    let now = time();
    for (path, kind, at) in [
        (&completed, "completed", now),
        (&failed, "failed", now),
        (&older, "needs_input", now - 20),
        (&needs, "needs_input", now),
        (&hidden, "needs_input", now),
    ] {
        event(&mut app, path, kind, at);
    }
    app.open_search();
    app.build_search_hits();
    let result = titles(&app);
    let ordinary = app
        .search_candidates()
        .into_iter()
        .filter(|candidate| {
            !["needs", "older-needs", "failed", "completed"].contains(&candidate.title.as_str())
        })
        .take(16)
        .map(|candidate| candidate.title)
        .collect::<Vec<_>>();
    assert_eq!(&result[4..], ordinary);
    assert_eq!(
        &result[..4],
        ["needs", "older-needs", "failed", "completed"]
    );
    assert_eq!(result.len(), 20);
    assert!(!result.iter().any(|title| title == "hidden-needs"));
    app.spotlight.scope = Scope::All;
    app.build_search_hits();
    assert_eq!(&titles(&app)[..4], &result[..4]);
    app.spotlight.scope = Scope::Chats;
    app.search = "ordinary".into();
    app.build_search_hits();
    assert!(
        titles(&app)
            .iter()
            .all(|title| title.starts_with("ordinary"))
    );
    assert!(app.spotlight.hits.iter().all(|hit| hit.attention.is_none()));
}
#[test]
fn explicit_attention_search_demo_is_frozen_in_memory_and_has_no_provider_or_history_reads() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_search_attention: true,
            ..Default::default()
        },
    );
    let unread = app.attention.unread();
    for _ in 0..5 {
        crate::app::files::tests::frame(&ctx, &mut app, vec![]);
    }
    assert!(app.search_open);
    assert!(!app.attention.open);
    assert_eq!(
        app.spotlight.hits[0].attention.unwrap().label(),
        "Observed: needs input"
    );
    assert_eq!(app.attention.unread(), unread);
    let keys = app
        .spotlight
        .hits
        .iter()
        .map(|hit| format!("{:?}:{:?}", hit.candidate.target, hit.candidate.path))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        keys.len(),
        app.spotlight.hits.len(),
        "unsaved current owner is not duplicated"
    );
    assert_eq!(app.spotlight.hits.len(), 2);
    assert!(app.client.is_none());
    assert!(app.workspace_rx.is_none() && app.diff_rx.is_none());
    assert!(app.spotlight.rx.is_none());
    assert!(app.sessions.is_empty());
}

#[test]
fn new_event_and_mark_read_do_not_move_selected_row_until_explicit_reopen() {
    let (_, mut app) = fixture();
    app.sessions.clear();
    let a = add(&mut app, "alpha");
    let b = add(&mut app, "beta");
    event(&mut app, &b, "completed", time());
    app.open_search();
    app.build_search_hits();
    assert_eq!(titles(&app)[0], "beta");
    app.spotlight.selected = 1;
    let original = titles(&app);
    event(&mut app, &a, "needs_input", time());
    app.attention.forget_path(&b);
    // Simulate ordinary sidebar activity reordering, not just an event update.
    for session in &mut app.sessions {
        if session.path == a {
            session.modified = std::time::SystemTime::now() + Duration::from_secs(60);
        }
    }
    for _ in 0..20 {
        app.build_search_hits();
        assert_eq!(titles(&app), original);
        assert_eq!(app.spotlight.selected, 1);
    }
    assert_eq!(
        app.spotlight.hits[0].attention.unwrap().label(),
        "Observed: completed"
    );
    app.close_search();
    app.open_search();
    app.build_search_hits();
    assert_eq!(titles(&app)[0], "alpha");
    assert!(app.spotlight.hits[1].attention.is_none());
}
#[test]
fn baseline_chat_order_stays_frozen_during_activity_updates_but_hidden_result_is_removed() {
    let (_, mut app) = fixture();
    app.sessions.clear();
    let a = add(&mut app, "alpha");
    let b = add(&mut app, "beta");
    app.open_search();
    app.build_search_hits();
    let original = titles(&app);
    for session in &mut app.sessions {
        if session.path == a {
            session.modified = std::time::SystemTime::now() + Duration::from_secs(60);
        }
    }
    app.build_search_hits();
    assert_eq!(titles(&app), original);
    app.prefs.chats.entry(b.clone()).or_default().hidden = true;
    app.build_search_hits();
    assert!(
        !app.spotlight
            .hits
            .iter()
            .any(|hit| hit.candidate.path.as_ref() == Some(&b))
    );
    app.prefs.chats.entry(b.clone()).or_default().hidden = false;
    app.close_search();
    app.open_search();
    app.build_search_hits();
    let ordinary = app
        .spotlight
        .hits
        .iter()
        .filter_map(|hit| hit.candidate.path.as_ref())
        .filter(|path| *path == &a || *path == &b)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(ordinary, [a, b]);
}

#[test]
fn opening_and_highlighting_never_marks_read_and_native_chat_selection_preserves_draft_run_scope() {
    let (ctx, mut app) = fixture();
    app.sessions.clear();
    let target = add(&mut app, "ranked chat");
    event(&mut app, &target, "completed", time());
    let unread = app.attention.unread();
    app.draft = "Kept unsent".into();
    app.state.busy = true;
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.open_search();
    for _ in 0..5 {
        crate::app::files::tests::frame(&ctx, &mut app, vec![]);
    }
    assert_eq!(app.attention.unread(), unread);
    assert_eq!(app.spotlight.hits[0].candidate.path.as_ref(), Some(&target));
    app.spotlight.selected = 0;
    app.build_search_hits();
    assert_eq!(app.attention.unread(), unread);
    assert_eq!(app.draft, "Kept unsent");
    assert!(commands.try_recv().is_err());
    // A newly arriving real approval blocks activation, even of ranked metadata.
    let hit = app.spotlight.hits[0].clone();
    app.state.dialogs.push_back(questions::demo_dialog());
    app.activate_search_hit(hit);
    assert!(app.search_open);
    assert_eq!(app.state.dialogs.len(), 1);
    assert_eq!(app.attention.unread(), unread);
    assert!(commands.try_recv().is_err());
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

#[test]
fn actual_enter_and_pointer_on_ranked_current_chat_preserve_owner_draft_and_never_prompt_abort() {
    for pointer in [false, true] {
        let (ctx, mut app) = fixture();
        app.state.session_file = Some(
            app.prefs
                .cwd
                .join("current-owner.jsonl")
                .display()
                .to_string(),
        );
        app.state.session_name = "Ranked current owner".into();
        let owner = app.ensure_agent();
        let path = PathBuf::from(app.state.session_file.as_ref().unwrap());
        event(&mut app, &path, "completed", time());
        app.state.busy = true;
        app.draft = "Unsent owner draft".into();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.connected = true;
        app.open_search();
        for _ in 0..4 {
            crate::app::files::tests::frame(&ctx, &mut app, vec![]);
        }
        assert_eq!(app.spotlight.hits[0].candidate.path.as_ref(), Some(&path));
        assert!(app.spotlight.hits[0].attention.is_some());
        if pointer {
            let rect = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("spotlight-row", 0usize))))
                .unwrap();
            crate::app::files::tests::click(&ctx, &mut app, rect.center());
        } else {
            crate::app::files::tests::frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
        }
        assert!(!app.search_open);
        assert_eq!(app.agents.active, owner);
        assert_eq!(app.draft, "Unsent owner draft");
        assert!(app.state.busy);
        assert!(app.client.is_some());
        assert!(commands.try_iter().all(|command| !matches!(
            command["type"].as_str(),
            Some("prompt" | "abort" | "switch_session")
        )));
    }
}

#[test]
fn native_arrow_selection_survives_new_attention_event_and_escape_leaves_all_unread() {
    let (ctx, mut app) = fixture();
    app.sessions.clear();
    let a = add(&mut app, "first");
    let b = add(&mut app, "second");
    event(&mut app, &b, "completed", time());
    app.state.busy = true;
    app.draft = "No send".into();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.open_search();
    for _ in 0..4 {
        crate::app::files::tests::frame(&ctx, &mut app, vec![]);
    }
    crate::app::files::tests::frame(&ctx, &mut app, vec![key(egui::Key::ArrowDown)]);
    let selected = app.spotlight.hits[app.spotlight.selected]
        .candidate
        .path
        .clone();
    let order = titles(&app);
    event(&mut app, &a, "needs_input", time());
    crate::app::files::tests::frame(&ctx, &mut app, vec![]);
    assert_eq!(titles(&app), order);
    assert_eq!(
        app.spotlight.hits[app.spotlight.selected].candidate.path,
        selected
    );
    let unread = app.attention.unread();
    crate::app::files::tests::frame(&ctx, &mut app, vec![key(egui::Key::Escape)]);
    assert!(!app.search_open);
    assert_eq!(app.attention.unread(), unread);
    assert_eq!(app.draft, "No send");
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
}

#[test]
fn observed_badges_stay_single_row_in_narrow_themes_and_do_not_label_message_or_settings() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app) = fixture();
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            app.sessions.clear();
            let target = add(&mut app, "Controlled note title");
            event(&mut app, &target, "needs_input", time());
            app.open_search();
            app.build_search_hits();
            let hit = &app.spotlight.hits[0];
            assert!(hit.attention.is_some());
            let mut rect = None;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(380.0, 200.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| rect = Some(result_row(ui, hit, true).rect));
                },
            );
            assert_eq!(rect.unwrap().height(), 30.0);
            for shape in output.shapes {
                if let egui::Shape::Text(text) = shape.shape {
                    assert_eq!(text.galley.rows.len(), 1);
                }
            }
            app.spotlight.scope = Scope::Settings;
            app.search = "sound".into();
            app.build_search_hits();
            assert!(app.spotlight.hits.iter().all(|hit| hit.attention.is_none()));
        }
    }
}
