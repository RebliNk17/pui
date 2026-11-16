use super::*;
fn fixture(ctx: &egui::Context) -> Pui {
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
    let (tx, _rx) = mpsc::channel();
    app.tasks.store = Some(tx);
    app.tasks
        .current_mut()
        .unwrap()
        .add_checkpoint("original")
        .unwrap();
    app
}
#[test]
fn edit_preserves_metadata_refuses_stale_and_targets_selected_store_without_work() {
    let ctx = egui::Context::default();
    let mut app = fixture(&ctx);
    let (tx, rx) = mpsc::channel();
    app.tasks.store = Some(tx);
    let task = app.tasks.current().unwrap().clone();
    let original = task.checkpoints[0].clone();
    app.save_checkpoint_edit(&task.id, 0, &original, "  revised 世界  ")
        .unwrap();
    let StoreCommand::Save(saved) = rx.try_recv().unwrap() else {
        panic!()
    };
    assert_eq!(saved.checkpoints[0].text, "revised 世界");
    assert_eq!(saved.checkpoints[0].created, original.created);
    assert_eq!(saved.checkpoints[0].phase, original.phase);
    assert_eq!(saved.phase, task.phase);
    assert_eq!(saved.checkpoints.len(), task.checkpoints.len());
    assert!(
        app.save_checkpoint_edit(&task.id, 0, &original, "stale overwrite")
            .is_err()
    );
    assert!(rx.try_recv().is_err());
    assert!(app.tasks.processes.is_empty());
    assert!(app.tasks.job.is_none());
    app.tasks.selected = None;
    let changed = saved.checkpoints[0].clone();
    assert!(
        app.save_checkpoint_edit(&task.id, 0, &changed, "wrong owner")
            .is_err()
    );
}
#[test]
fn domain_invalid_edit_preserves_whole_task_and_serialized_restore_has_only_committed_text() {
    let ctx = egui::Context::default();
    let mut app = fixture(&ctx);
    let task = app.tasks.current_mut().unwrap();
    let original = task.checkpoints[0].clone();
    let before = serde_json::to_value(&task).unwrap();
    for text in ["".into(), "x".repeat(2049), "bad\0value".into()] {
        assert!(task.edit_checkpoint(0, &original, &text).is_err());
        assert_eq!(serde_json::to_value(&task).unwrap(), before);
    }
    assert!(
        task.edit_checkpoint(32, &original, "invalid index")
            .is_err()
    );
    task.edit_checkpoint(0, &original, "committed").unwrap();
    let restored: Task = serde_json::from_slice(&serde_json::to_vec(&task).unwrap()).unwrap();
    assert_eq!(restored.checkpoints[0].text, "committed");
    restored.validate_checkpoints().unwrap();
}
#[test]
fn native_pointer_edit_save_light_dark_narrow_zoom() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            let mut app = fixture(&ctx);
            let (tx, rx) = mpsc::channel();
            app.tasks.store = Some(tx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            app.tasks.open = true;
            let task = app.tasks.current().unwrap().clone();
            let mut view = work_item::View::default();
            view.tab = work_item::Tab::Checkpoints;
            let draw = |app: &mut Pui, view: &mut work_item::View, events| {
                let current = app.tasks.current().unwrap().clone();
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 620.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .show(ctx, |ui| app.task_checkpoints(ui, &current, view));
                    },
                )
            };
            for _ in 0..3 {
                draw(&mut app, &mut view, vec![]);
            }
            let (_, rect, enabled) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new((
                        "checkpoint-edit-open",
                        0usize,
                    )))
                })
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
            assert!(view.checkpoint_edit.is_some());
            view.checkpoint_edit.as_mut().unwrap().text = "updated".into();
            for _ in 0..2 {
                draw(&mut app, &mut view, vec![]);
            }
            let (_, rect, enabled) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new(
                        "checkpoint-edit-save",
                    ))
                })
                .unwrap();
            assert!(enabled);
            assert!(rect.right() <= 420.0);
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
            assert!(view.checkpoint_edit.is_none());
            let StoreCommand::Save(saved) = rx.try_recv().unwrap() else {
                panic!()
            };
            assert_eq!(saved.checkpoints[0].text, "updated");
            assert_eq!(saved.phase, task.phase);
            assert!(app.tasks.processes.is_empty());
            let original = saved.checkpoints[0].clone();
            view.checkpoint_edit = Some(Editor {
                index: 0,
                original: original.clone(),
                text: "cancel me".into(),
            });
            draw(&mut app, &mut view, vec![]);
            let (_, _, ids) = ctx
                .data(|d| {
                    d.get_temp::<(String, u64, Vec<egui::Id>)>(egui::Id::new(
                        "checkpoint-edit-controls",
                    ))
                })
                .unwrap();
            ctx.memory_mut(|m| m.request_focus(ids[0]));
            draw(&mut app, &mut view, vec![]);
            app.tasks.views.insert(task.id.clone(), view);
            let _ = ctx.run(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                },
                |ctx| app.checkpoint_inputs(ctx),
            );
            assert!(app.tasks.views[&task.id].checkpoint_edit.is_none());
            assert_eq!(app.tasks.current().unwrap().checkpoints[0], original);
            assert!(rx.try_recv().is_err());
            app.demo = true;
            assert!(
                app.save_checkpoint_edit(&task.id, 0, &original, "disabled")
                    .is_err()
            );
        }
    }
}
