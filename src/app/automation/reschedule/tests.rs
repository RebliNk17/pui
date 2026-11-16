use super::*;
#[test]
fn preview_is_exact_utc_three_occurrences_and_invalid_is_whole_refusal() {
    let after = chrono::DateTime::parse_from_rfc3339("2026-01-01T08:00:00Z")
        .unwrap()
        .timestamp();
    let times = preview("0 9 * * *", after).unwrap();
    assert_eq!(times.len(), 3);
    assert_eq!(times[0], after + 3600);
    assert_eq!(times[1] - times[0], 86400);
    for invalid in ["bad", "0 0 31 2 *", "* * * * * command"] {
        assert!(preview(invalid, after).is_err());
    }
}
#[test]
fn paused_reschedule_changes_only_schedule_keeps_delivery_and_requires_exact_snapshot() {
    let ctx = egui::Context::default();
    let mut app = super::super::tests::app(&ctx);
    let id = super::super::tests::create(&mut app, "cron");
    assert!(app.begin_reschedule(&id).is_err());
    app.change_job(&id, "pause").unwrap();
    app.begin_reschedule(&id).unwrap();
    let mut editor = app.automation.reschedule.take().unwrap();
    editor.draft = "0 9 * * 1-5".into();
    app.save_reschedule(&editor).unwrap();
    let job = &app.automation.jobs[0];
    assert!(!job.enabled);
    assert!(!job.dispatching);
    assert!(job.pending.is_none());
    assert_eq!(job.deliveries, 0);
    assert!(matches!(&job.trigger,Trigger::Cron{schedule}if schedule=="0 9 * * 1-5"));
    assert!(app.save_reschedule(&editor).is_err());
    assert!(app.automation.running.is_empty());
}
#[test]
fn modal_cancel_themes_zoom_does_not_reschedule_or_deliver() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            let mut app = super::super::tests::app(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let id = super::super::tests::create(&mut app, "cron");
            app.change_job(&id, "pause").unwrap();
            app.automation.open = true;
            app.begin_reschedule(&id).unwrap();
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
                    |ctx| app.reschedule_dialog(ctx),
                );
            }
            assert!(app.automation.reschedule.is_none());
            assert_eq!(before, serde_json::to_value(&app.automation.jobs).unwrap());
            assert!(app.automation.running.is_empty());
        }
    }
}
#[test]
fn native_save_pointer_changes_paused_schedule_without_prompt() {
    let ctx = egui::Context::default();
    let mut app = super::super::tests::app(&ctx);
    let id = super::super::tests::create(&mut app, "cron");
    app.change_job(&id, "pause").unwrap();
    app.automation.open = true;
    app.begin_reschedule(&id).unwrap();
    app.automation.reschedule.as_mut().unwrap().draft = "0 12 * * *".into();
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
            |ctx| app.reschedule_dialog(ctx),
        )
    };
    for _ in 0..4 {
        draw(&mut app, vec![]);
    }
    let (_, rect, enabled) = ctx
        .data(|d| d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new("cron-save-schedule")))
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
    assert!(app.automation.reschedule.is_none());
    assert!(!app.automation.jobs[0].enabled);
    assert!(
        matches!(&app.automation.jobs[0].trigger,Trigger::Cron{schedule}if schedule=="0 12 * * *")
    );
    assert_eq!(app.automation.jobs[0].deliveries, 0);
}
#[test]
fn monitor_timing_bounds_and_paused_save_preserve_command_and_delivery() {
    assert_eq!(monitor_timing("10", "300").unwrap(), (10, 300));
    for (i, t) in [
        ("9", "1"),
        ("86401", "1"),
        ("10", "0"),
        ("10", "301"),
        ("exit 0", "2"),
    ] {
        assert!(monitor_timing(i, t).is_err());
    }
    let ctx = egui::Context::default();
    let mut app = super::super::tests::app(&ctx);
    let id = super::super::tests::create(&mut app, "monitor");
    assert!(app.begin_reschedule(&id).is_err());
    app.change_job(&id, "pause").unwrap();
    app.begin_reschedule(&id).unwrap();
    let mut editor = app.automation.reschedule.take().unwrap();
    editor.monitor = Some(("120".into(), "15".into()));
    let message = app.automation.jobs[0].message.clone();
    app.save_reschedule(&editor).unwrap();
    let job = &app.automation.jobs[0];
    assert!(
        matches!(&job.trigger,Trigger::Monitor{command,interval_seconds:120,timeout_seconds:15}if command=="exit 0")
    );
    assert!(!job.enabled);
    assert_eq!(job.message, message);
    assert_eq!(job.deliveries, 0);
    assert!(app.automation.running.is_empty());
    assert!(app.save_reschedule(&editor).is_err());
}
#[test]
fn monitor_modal_pointer_save_and_invalid_cancel_in_themes_zoom() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            let mut app = super::super::tests::app(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let id = super::super::tests::create(&mut app, "monitor");
            app.change_job(&id, "pause").unwrap();
            app.automation.open = true;
            app.begin_reschedule(&id).unwrap();
            app.automation.reschedule.as_mut().unwrap().monitor = Some(("60".into(), "20".into()));
            let draw = |app: &mut Pui, events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 620.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| app.reschedule_dialog(ctx),
                )
            };
            for _ in 0..4 {
                draw(&mut app, vec![]);
            }
            let (_, rect, enabled) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new("cron-save-schedule"))
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
            assert!(app.automation.reschedule.is_none());
            assert!(matches!(
                app.automation.jobs[0].trigger,
                Trigger::Monitor {
                    interval_seconds: 60,
                    timeout_seconds: 20,
                    ..
                }
            ));
            assert!(!app.automation.jobs[0].enabled);
            assert!(app.automation.running.is_empty());
            app.begin_reschedule(&id).unwrap();
            app.automation.reschedule.as_mut().unwrap().monitor = Some(("0".into(), "20".into()));
            draw(&mut app, vec![]);
            assert!(
                !ctx.data(|d| d
                    .get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new("cron-save-schedule")))
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
            assert!(app.automation.reschedule.is_none());
            assert_eq!(app.automation.jobs[0].deliveries, 0);
        }
    }
}
#[test]
fn content_edit_preserves_trigger_creator_and_next_run_while_paused() {
    let ctx = egui::Context::default();
    let mut app = super::super::tests::app(&ctx);
    let id = super::super::tests::create(&mut app, "monitor");
    app.change_job(&id, "pause").unwrap();
    app.begin_job_content(&id).unwrap();
    let mut editor = app.automation.reschedule.take().unwrap();
    let before = app.automation.jobs[0].clone();
    editor.content = Some((
        "New title".into(),
        "Literal <script> 世界\nfollow up".into(),
    ));
    app.save_reschedule(&editor).unwrap();
    let after = &app.automation.jobs[0];
    assert_eq!(
        serde_json::to_value(&after.trigger).unwrap(),
        serde_json::to_value(&before.trigger).unwrap()
    );
    assert_eq!(after.creator.id, before.creator.id);
    assert_eq!(after.next_run, before.next_run);
    assert_eq!(after.name, "New title");
    assert_eq!(after.message, "Literal <script> 世界\nfollow up");
    assert!(!after.enabled);
    assert_eq!(after.deliveries, 0);
    assert!(app.automation.running.is_empty());
    assert!(app.save_reschedule(&editor).is_err());
    for (name, message) in [
        ("", "valid"),
        ("valid", ""),
        ("bad\nname", "valid"),
        ("valid", "bad\0message"),
    ] {
        assert!(validate_content(name, message).is_err());
    }
    assert!(validate_content(&"世".repeat(107), "valid").is_err());
}
#[test]
fn prompt_modal_pointer_save_and_escape_themes_zoom_never_delivers() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            let mut app = super::super::tests::app(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let id = super::super::tests::create(&mut app, "cron");
            app.change_job(&id, "pause").unwrap();
            app.automation.open = true;
            app.begin_job_content(&id).unwrap();
            app.automation.reschedule.as_mut().unwrap().content =
                Some(("Edited job".into(), "Edited follow-up".into()));
            let draw = |app: &mut Pui, events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 620.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| app.reschedule_dialog(ctx),
                )
            };
            for _ in 0..4 {
                draw(&mut app, vec![]);
            }
            let (_, rect, enabled) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new("cron-save-schedule"))
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
            assert!(app.automation.reschedule.is_none());
            assert_eq!(app.automation.jobs[0].message, "Edited follow-up");
            assert!(!app.automation.jobs[0].enabled);
            app.begin_job_content(&id).unwrap();
            app.automation.reschedule.as_mut().unwrap().content =
                Some(("".into(), "bad unsaved".into()));
            draw(&mut app, vec![]);
            assert!(
                !ctx.data(|d| d
                    .get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new("cron-save-schedule")))
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
            assert!(app.automation.reschedule.is_none());
            assert_eq!(app.automation.jobs[0].message, "Edited follow-up");
            assert_eq!(app.automation.jobs[0].deliveries, 0);
        }
    }
}
#[test]
fn demo_refuses_schedule_editor_without_mutating_jobs() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo_automation();
    let before = serde_json::to_value(&app.automation.jobs).unwrap();
    let id = app.automation.jobs[0].id.clone();
    assert!(app.begin_reschedule(&id).is_err());
    assert_eq!(before, serde_json::to_value(&app.automation.jobs).unwrap());
    assert!(app.automation.reschedule.is_none());
}
