use super::*;
#[test]
fn linked_checkpoint_legacy_roundtrip_validation_and_missing_run_refusal() {
    let mut task = Task::new(
        "/fixture".into(),
        "provider".into(),
        "model".into(),
        "off".into(),
    );
    assert!(task.add_linked_checkpoint("note", "missing").is_err());
    assert!(task.checkpoints.is_empty());
    task.runs.push(Run {
        id: "run-one".into(),
        role: Role::Checks,
        attempt: 0,
        status: "failed".into(),
        session: None,
        summary: String::new(),
        cost: 0.0,
        permission_cost: 0.0,
        selection: None,
        routing_for: None,
    });
    let phase = task.phase;
    task.add_linked_checkpoint("user note", "run-one").unwrap();
    let note = task.checkpoints[0].clone();
    assert_eq!(note.run.as_deref(), Some("run-one"));
    task.edit_checkpoint(0, &note, "edited").unwrap();
    assert_eq!(task.checkpoints[0].run, note.run);
    let mut value = serde_json::to_value(&task).unwrap();
    let restored: Task = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(restored.checkpoints[0].run, note.run);
    value["checkpoints"][0]
        .as_object_mut()
        .unwrap()
        .remove("run");
    let legacy: Task = serde_json::from_value(value).unwrap();
    assert!(legacy.checkpoints[0].run.is_none());
    task.runs.clear();
    task.validate_checkpoints().unwrap();
    assert!(task.add_linked_checkpoint("new", "run-one").is_err());
    assert_eq!(task.phase, phase);
    task.checkpoints[0].run = Some("x".repeat(129));
    assert!(task.validate_checkpoints().is_err());
}
#[test]
fn native_link_note_and_inspect_roundtrip_themes_zoom_no_execution() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            let mut app = Pui::new(
                &eframe::CreationContext::_new_kittest(ctx.clone()),
                Options {
                    demo: true,
                    ..Default::default()
                },
            );
            app.demo_task();
            app.demo = false;
            app.tasks.live = None;
            let (tx, rx) = mpsc::channel();
            app.tasks.store = Some(tx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut task = app.tasks.current().unwrap().clone();
            let run = task.runs.first().unwrap().clone();
            let mut view = work_item::View::default();
            view.tab = work_item::Tab::Activity;
            let draw = |app: &mut Pui, view: &mut work_item::View, task: &Task, events| {
                ctx.run(
                    egui::RawInput {
                        events,
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 620.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            if view.tab == work_item::Tab::Activity {
                                app.checkpoint_link_action(ui, task, view, &run);
                            } else {
                                app.task_checkpoints(ui, task, view);
                            }
                        });
                    },
                )
            };
            for _ in 0..3 {
                draw(&mut app, &mut view, &task, vec![]);
            }
            let (_, rect, enabled) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new(
                        "checkpoint-link-action",
                    ))
                })
                .unwrap();
            assert!(enabled);
            for pressed in [true, false] {
                draw(
                    &mut app,
                    &mut view,
                    &task,
                    vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::PointerButton {
                            pos: rect.center(),
                            pressed,
                            button: egui::PointerButton::Primary,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert_eq!(view.tab, work_item::Tab::Checkpoints);
            assert_eq!(view.checkpoint_run.as_deref(), Some(run.id.as_str()));
            assert!(rx.try_recv().is_err());
            app.save_checkpoint_link(&task.id, "linked note", view.checkpoint_run.as_deref())
                .unwrap();
            let StoreCommand::Save(saved) = rx.try_recv().unwrap() else {
                panic!()
            };
            task = *saved;
            view.checkpoint_run = None;
            for _ in 0..3 {
                draw(&mut app, &mut view, &task, vec![]);
            }
            let index = task.checkpoints.len() - 1;
            let (_, rect, enabled) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new((
                        "checkpoint-inspect-run",
                        index,
                    )))
                })
                .unwrap();
            assert!(enabled);
            for pressed in [true, false] {
                draw(
                    &mut app,
                    &mut view,
                    &task,
                    vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::PointerButton {
                            pos: rect.center(),
                            pressed,
                            button: egui::PointerButton::Primary,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert_eq!(view.tab, work_item::Tab::Activity);
            assert_eq!(app.tasks.selected_run.as_deref(), Some(run.id.as_str()));
            assert!(app.tasks.processes.is_empty());
            assert!(app.tasks.job.is_none());
            assert!(rx.try_recv().is_err());
        }
    }
}
