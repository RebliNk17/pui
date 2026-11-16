use super::*;
#[test]
fn explicit_real_test_retains_diagnostic_without_resume_delivery_or_schedule_change() {
    let ctx = egui::Context::default();
    let mut app = super::super::tests::app(&ctx);
    let id = super::super::tests::create(&mut app, "monitor");
    assert!(app.request_monitor_test(&id).is_err());
    app.change_job(&id, "pause").unwrap();
    app.request_monitor_test(&id).unwrap();
    let (id, original) = app.automation.test_confirm.take().unwrap();
    let before = app.automation.jobs[0].clone();
    app.start_monitor_test(id, original).unwrap();
    let check = app
        .automation
        .precheck
        .as_ref()
        .unwrap()
        .run
        .rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let (tx, rx) = mpsc::channel();
    tx.send(check).unwrap();
    app.automation.precheck.as_mut().unwrap().run.rx = rx;
    app.poll_monitor_test();
    let job = &app.automation.jobs[0];
    assert!(!job.enabled);
    assert_eq!(job.next_run, before.next_run);
    assert_eq!(job.last_run, before.last_run);
    assert_eq!(job.deliveries, 0);
    assert!(job.pending.is_none());
    assert!(job.evidence.is_some());
    assert!(app.automation.precheck.is_none());
    assert!(app.automation.running.is_empty());
}
#[test]
fn native_confirmation_escape_themes_zoom_runs_nothing() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            let mut app = super::super::tests::app(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let id = super::super::tests::create(&mut app, "monitor");
            app.change_job(&id, "pause").unwrap();
            app.automation.open = true;
            app.request_monitor_test(&id).unwrap();
            let before = serde_json::to_value(&app.automation.jobs).unwrap();
            for n in 0..4 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 620.0),
                        )),
                        events: if n == 3 {
                            vec![egui::Event::Key {
                                key: egui::Key::Escape,
                                pressed: true,
                                repeat: false,
                                physical_key: None,
                                modifiers: egui::Modifiers::NONE,
                            }]
                        } else {
                            vec![]
                        },
                        ..Default::default()
                    },
                    |ctx| app.monitor_test_dialog(ctx),
                );
            }
            assert!(app.automation.test_confirm.is_none());
            assert!(app.automation.precheck.is_none());
            assert_eq!(before, serde_json::to_value(&app.automation.jobs).unwrap());
        }
    }
}
