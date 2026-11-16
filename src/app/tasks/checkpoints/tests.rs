use super::*;
#[test]
fn explicit_save_targets_selected_task_only_queues_storage_and_never_runs_work() {
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
    let task = app.tasks.current().unwrap().clone();
    let (tx, rx) = mpsc::channel();
    app.tasks.store = Some(tx);
    app.tasks.live = None;
    let phase = task.phase;
    let mut view = work_item::View::default();
    view.checkpoint_draft = "explicit handoff".into();
    let draw = |app: &mut Pui, view: &mut work_item::View, events| {
        ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.task_checkpoints(ui, &task, view));
            },
        )
    };
    for _ in 0..3 {
        draw(&mut app, &mut view, vec![]);
    }
    let (_, rect, enabled) = ctx
        .data(|d| d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new("save-task-checkpoint")))
        .unwrap();
    assert!(enabled);
    for pressed in [true, false] {
        draw(
            &mut app,
            &mut view,
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
    assert!(view.checkpoint_draft.is_empty());
    let StoreCommand::Save(saved) = rx.try_recv().unwrap() else {
        panic!()
    };
    assert_eq!(saved.id, task.id);
    assert_eq!(saved.phase, phase);
    assert_eq!(saved.checkpoints.last().unwrap().text, "explicit handoff");
    assert!(app.tasks.processes.is_empty());
    assert!(app.tasks.job.is_none());
    app.tasks.live = Some("other-task".into());
    assert!(app.save_checkpoint(&task.id, "wrong owner").is_err());
    assert!(rx.try_recv().is_err());
}
#[test]
fn checkpoint_ui_themes_zoom_demo_disabled_and_cancel_keeps_workflow_unchanged() {
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
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            app.demo_task();
            let task = app.tasks.current().unwrap().clone();
            let before = serde_json::to_value(&task).unwrap();
            let mut view = work_item::View::default();
            view.checkpoint_draft = "unsaved note".into();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(420.0, 620.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| app.task_checkpoints(ui, &task, &mut view));
                },
            );
            let (_, _, enabled) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new(
                        "save-task-checkpoint",
                    ))
                })
                .unwrap();
            assert!(!enabled);
            assert!(app.save_checkpoint(&task.id, "no implicit write").is_err());
            assert_eq!(
                serde_json::to_value(app.tasks.current().unwrap()).unwrap(),
                before
            );
            assert_eq!(view.checkpoint_draft, "unsaved note");
            assert!(app.tasks.processes.is_empty());
        }
    }
}
