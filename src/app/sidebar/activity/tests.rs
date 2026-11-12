use super::*;

#[test]
fn compact_age_is_truthful_bounded_and_unknown_is_not_just_now() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100_000_000);
    assert_eq!(label(None, now), "—");
    assert_eq!(label(Some(SystemTime::UNIX_EPOCH), now), "—");
    for (seconds, expected) in [
        (0, "now"),
        (59, "now"),
        (60, "1m"),
        (3599, "59m"),
        (3600, "1h"),
        (86400, "1d"),
        (31_536_000, "1y"),
    ] {
        assert_eq!(
            label(Some(now - Duration::from_secs(seconds)), now),
            expected
        );
    }
    assert_eq!(label(Some(now + Duration::from_secs(60)), now), "now");
}

#[test]
fn root_and_child_activity_remain_their_own_not_descendant_sorting_time() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.sessions.clear();
    app.state.session_file = Some("/fixture/activity-parent.jsonl".into());
    app.demo_agent_tree();
    let parent = app.agents.active.clone();
    let child = app.agents.nodes[&parent].children[0].clone();
    let old = SystemTime::now() - Duration::from_secs(7200);
    let fresh = SystemTime::now() - Duration::from_secs(120);
    app.agents.nodes.get_mut(&parent).unwrap().last_activity = Some(old);
    app.agents.nodes.get_mut(&child).unwrap().last_activity = Some(fresh);
    // Demo messages are not a useful timestamp source for this fixture.
    app.state.messages.clear();
    let rows = sidebar_tree::forest(&app);
    let row = rows
        .iter()
        .find(|row| row.target == sidebar_tree::Target::Agent(parent.clone()))
        .unwrap();
    assert_eq!(row.activity, Some(old));
    assert_eq!(row.children[0].activity, Some(fresh));
    let second = &row.children[1];
    assert!(second.activity.is_none());
    // Loaded parked history can supply a real own-row timestamp even without
    // discovery or a live event. It is not inferred from the parent's runtime.
    let second_id = app.agents.nodes[&parent].children[1].clone();
    let seconds = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 3600;
    app.agents
        .parked
        .get_mut(&second_id)
        .unwrap()
        .state
        .messages
        .last_mut()
        .unwrap()
        .timestamp = (seconds * 1000) as i64;
    let reprojected = sidebar_tree::forest(&app);
    let dated = reprojected
        .iter()
        .find(|row| matches!(&row.target, sidebar_tree::Target::Agent(id) if id == &parent))
        .unwrap();
    assert_eq!(label(dated.children[1].activity, SystemTime::now()), "1h");
    // No node row inherits the fresh activity of an unrelated/child row.
    assert_eq!(label(row.activity, SystemTime::now()), "2h");
    assert_eq!(label(second.activity, SystemTime::now()), "—");
}

#[test]
fn only_observed_agent_work_updates_activity_not_status_queries_or_repaint() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.ensure_agent();
    let id = app.agents.active.clone();
    assert!(app.agents.nodes[&id].last_activity.is_none());
    for record in [
        json!({"type":"response","command":"get_state","success":true}),
        json!({"type":"extension_ui_request","method":"setStatus"}),
    ] {
        app.agent_event(&record);
    }
    assert!(app.agents.nodes[&id].last_activity.is_none());
    app.agent_event(&json!({"type":"tool_execution_update"}));
    let stamp = app.agents.nodes[&id].last_activity.unwrap();
    assert!(stamp.elapsed().unwrap() < Duration::from_secs(2));
    app.agent_event(&json!({"type":"response","command":"get_state"}));
    assert_eq!(app.agents.nodes[&id].last_activity, Some(stamp));
}

#[test]
fn ages_are_visible_without_metadata_and_do_not_overlap_disclosure_or_overflow() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        for zoom in [0.8, 1.0, 1.4] {
            let ctx = egui::Context::default();
            let mut app = crate::app::files::tests::app(&ctx);
            app.prefs.theme = theme;
            ctx.set_zoom_factor(zoom);
            app.demo_agent_tree();
            app.projects = false;
            let parent = app.agents.active.clone();
            let child = app.agents.nodes[&parent].children[0].clone();
            app.agents.nodes.get_mut(&parent).unwrap().last_activity =
                Some(SystemTime::now() - Duration::from_secs(300));
            app.agents.nodes.get_mut(&child).unwrap().last_activity =
                Some(SystemTime::now() - Duration::from_secs(120));
            let mut time = 0.0;
            let mut draw = || {
                time += 0.1;
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(500.0, 900.0),
                        )),
                        time: Some(time),
                        ..Default::default()
                    },
                    |ctx| app.sidebar(ctx),
                )
            };
            for _ in 0..5 {
                draw();
            }
            let out = draw();
            let labels: Vec<_> = out
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape {
                        Some((
                            t.galley.job.text.as_str(),
                            egui::Rect::from_min_size(t.pos, t.galley.size()),
                        ))
                    } else {
                        None
                    }
                })
                .collect();
            for age in ["5m", "2m", "—"] {
                assert!(labels.iter().any(|(text, _)| *text == age), "missing {age}");
            }
            for (_, rect) in labels
                .iter()
                .filter(|(text, _)| ["5m", "2m"].contains(text))
            {
                assert!(ctx.screen_rect().contains_rect(*rect));
            }
            let rows = sidebar_tree::forest(&app);
            let row = rows
                .iter()
                .find(|row| matches!(&row.target,sidebar_tree::Target::Agent(id)if id==&parent))
                .unwrap();
            let toggle = ctx
                .data(|d| {
                    d.get_temp::<egui::Rect>(egui::Id::new(("sidebar-disclosure-rect", &row.key)))
                })
                .unwrap();
            let age = labels.iter().find(|(text, _)| *text == "5m").unwrap().1;
            assert!(age.left() > toggle.right());
            let parent_rect = ctx
                .data(|data| {
                    data.get_temp::<egui::Rect>(egui::Id::new(("sidebar-row-rect", &row.key)))
                })
                .unwrap();
            let child_rect = ctx
                .data(|data| {
                    data.get_temp::<egui::Rect>(egui::Id::new((
                        "sidebar-row-rect",
                        &row.children[0].key,
                    )))
                })
                .unwrap();
            assert_eq!(parent_rect.height(), ROW_HEIGHT);
            assert!((child_rect.top() - parent_rect.bottom() - 2.0).abs() < 0.1);
            assert!(!app.prefs.sidebar_view.metadata);
        }
    }
}
