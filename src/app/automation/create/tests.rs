use super::*;
#[test]
fn creates_paused_exact_owner_without_message_or_replay_and_stale_owner_refuses() {
    let ctx = egui::Context::default();
    let mut app = super::super::tests::app(&ctx);
    app.begin_cron_form().unwrap();
    let mut form = app.automation.create.take().unwrap();
    form.name = "Explicit job".into();
    form.message = "Review next steps".into();
    app.save_cron_form(&form).unwrap();
    let job = &app.automation.jobs[0];
    assert!(!job.enabled);
    assert!(!job.dispatching);
    assert!(job.pending.is_none());
    assert_eq!(job.deliveries, 0);
    assert_eq!(job.creator.id, form.owner.active);
    assert!(app.automation.running.is_empty());
    form.owner.record.cwd = PathBuf::from("/other-root");
    assert!(app.save_cron_form(&form).is_err());
    assert_eq!(app.automation.jobs.len(), 1);
}
#[test]
fn monitor_creation_requires_ack_and_stays_paused_without_running_shell() {
    let ctx = egui::Context::default();
    let mut app = super::super::tests::app(&ctx);
    app.begin_monitor_form().unwrap();
    let mut form = app.automation.create.take().unwrap();
    form.name = "Wait explicitly".into();
    form.message = "Review result".into();
    let monitor = form.monitor.as_mut().unwrap();
    monitor.command = "exit 0".into();
    assert!(app.save_cron_form(&form).is_err());
    form.monitor.as_mut().unwrap().acknowledged = true;
    app.save_cron_form(&form).unwrap();
    let job = &app.automation.jobs[0];
    assert!(
        matches!(&job.trigger,Trigger::Monitor{command,interval_seconds:60,timeout_seconds:30}if command=="exit 0")
    );
    assert!(!job.enabled);
    assert_eq!(job.deliveries, 0);
    assert!(app.automation.running.is_empty());
    assert!(job.pending.is_none());
}
#[test]
fn monitor_native_form_create_and_cancel_themes_zoom() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            let mut app = super::super::tests::app(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            app.automation.open = true;
            app.begin_monitor_form().unwrap();
            let form = app.automation.create.as_mut().unwrap();
            form.name = "Native monitor".into();
            form.message = "Review result".into();
            let monitor = form.monitor.as_mut().unwrap();
            monitor.command = "exit 0".into();
            monitor.acknowledged = true;
            let draw = |app: &mut Pui, events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(620.0, 900.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| app.cron_form(ctx),
                )
            };
            for _ in 0..4 {
                draw(&mut app, vec![]);
            }
            let (id, rect, enabled) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new("create-cron-save"))
                })
                .unwrap();
            assert!(enabled);
            assert!(
                ctx.read_response(id).unwrap().interact_rect.is_positive(),
                "{mode:?}/{zoom} {rect:?}"
            );
            for pressed in [true, false] {
                draw(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::PointerButton {
                            pos: rect.center(),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert!(app.automation.create.is_none());
            assert!(!app.automation.jobs[0].enabled);
            assert!(app.automation.running.is_empty());
            app.begin_monitor_form().unwrap();
            draw(&mut app, vec![]);
            draw(
                &mut app,
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    repeat: false,
                    physical_key: None,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert!(app.automation.create.is_none());
            assert_eq!(app.automation.jobs.len(), 1);
        }
    }
}
#[test]
fn native_pointer_create_paused_and_escape_cancel_themes_zoom() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            let mut app = super::super::tests::app(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            app.automation.open = true;
            app.begin_cron_form().unwrap();
            let form = app.automation.create.as_mut().unwrap();
            form.name = "Native recurring".into();
            form.message = "Only after Resume".into();
            let draw = |app: &mut Pui, events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(620.0, 900.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| app.cron_form(ctx),
                )
            };
            for _ in 0..4 {
                draw(&mut app, vec![]);
            }
            let (_, rect, enabled) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new("create-cron-save"))
                })
                .unwrap();
            assert!(enabled);
            for pressed in [true, false] {
                draw(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::PointerButton {
                            pos: rect.center(),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert!(app.automation.create.is_none());
            assert_eq!(app.automation.jobs.len(), 1);
            assert!(!app.automation.jobs[0].enabled);
            app.begin_cron_form().unwrap();
            draw(&mut app, vec![]);
            assert!(
                !ctx.data(|d| d
                    .get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new("create-cron-save")))
                    .unwrap()
                    .2
            );
            draw(
                &mut app,
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    repeat: false,
                    physical_key: None,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert!(app.automation.create.is_none());
            assert_eq!(app.automation.jobs.len(), 1);
        }
    }
}
