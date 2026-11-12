use super::*;

fn draw(
    ctx: &egui::Context,
    time: f64,
    stamp: Option<SystemTime>,
    visible: bool,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(300.0, 200.0),
            )),
            time: Some(time),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let y = if visible { 10.0 } else { 500.0 };
                paint(
                    ui,
                    egui::Rect::from_min_size(egui::pos2(10.0, y), egui::vec2(250.0, ROW_HEIGHT)),
                    stamp,
                );
            });
        },
    )
}

#[test]
fn only_visible_known_time_schedules_coarse_deadline_unknown_and_offscreen_stay_idle() {
    for (stamp, visible) in [
        (None, true),
        (Some(SystemTime::now() - Duration::from_secs(120)), false),
        (Some(SystemTime::now() - Duration::from_secs(120)), true),
    ] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        for n in 0..10 {
            draw(&ctx, n as f64, stamp, visible);
        }
        let out = draw(&ctx, 11.0, stamp, visible);
        let delay = out.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        assert!(
            delay > Duration::from_secs(1),
            "Must not run an animation: {delay:?}"
        );
        if stamp.is_some() && visible {
            assert!(delay <= Duration::from_secs(60));
        } else {
            assert!(delay > Duration::from_secs(3600));
        }
    }
}

#[test]
fn sidebar_demo_uses_real_compact_tree_ages_without_live_provider_or_preferences() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_agents: true,
            demo_sidebar_density: true,
            ..Default::default()
        },
    );
    for _ in 0..4 {
        crate::app::files::tests::frame(&ctx, &mut app, vec![]);
    }
    let out = crate::app::files::tests::frame(&ctx, &mut app, vec![]);
    assert!(app.client.is_none() && !app.model_picker_open && !app.prefs.sidebar_view.metadata);
    for age in ["5m", "2m", "3m"] {
        assert!(out.shapes.iter().any(
            |shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == age)
        ));
    }
}

#[test]
fn saved_root_has_visible_age_with_metadata_off_without_changing_permission_or_draft() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    app.sessions.truncate(1);
    app.sessions[0].modified = SystemTime::now() - Duration::from_secs(3600);
    app.projects = false;
    app.prefs.hide_sidebar = false;
    app.draft = "Keep draft".into();
    app.state.busy = true;
    app.state.dialogs.push_back(json!({"id":"age-permission","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."}));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let messages = app.state.messages.len();
    for _ in 0..4 {
        crate::app::files::tests::frame(&ctx, &mut app, vec![]);
    }
    let out = crate::app::files::tests::frame(&ctx, &mut app, vec![]);
    assert!(
        out.shapes
            .iter()
            .any(|s| matches!(&s.shape,egui::Shape::Text(t)if t.galley.job.text=="1h"))
    );
    assert_eq!(app.draft, "Keep draft");
    assert_eq!(app.state.messages.len(), messages);
    assert!(app.state.busy && app.state.dialogs.len() == 1 && app.pending_dialog.is_none());
    assert!(commands.try_recv().is_err());
}
