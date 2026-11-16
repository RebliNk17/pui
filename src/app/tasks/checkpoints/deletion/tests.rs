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
    let (tx, _) = mpsc::channel();
    app.tasks.store = Some(tx);
    let task = app.tasks.current_mut().unwrap();
    task.add_checkpoint("first").unwrap();
    task.add_checkpoint("same").unwrap();
    task.add_checkpoint("same").unwrap();
    app
}
#[test]
fn exact_list_delete_refuses_shifted_duplicates_and_preserves_workflow() {
    let ctx = egui::Context::default();
    let mut app = fixture(&ctx);
    let (tx, rx) = mpsc::channel();
    app.tasks.store = Some(tx);
    let before = app.tasks.current().unwrap().clone();
    let pending = Pending {
        owner: before.id.clone(),
        index: 1,
        notes: before.checkpoints.clone(),
    };
    app.delete_checkpoint(&pending).unwrap();
    let StoreCommand::Save(saved) = rx.try_recv().unwrap() else {
        panic!()
    };
    assert_eq!(saved.checkpoints.len(), 2);
    assert_eq!(saved.checkpoints[0], before.checkpoints[0]);
    assert_eq!(saved.checkpoints[1], before.checkpoints[2]);
    assert_eq!(saved.phase, before.phase);
    assert!(app.delete_checkpoint(&pending).is_err());
    assert!(rx.try_recv().is_err());
    assert!(app.tasks.processes.is_empty());
    assert!(app.tasks.job.is_none());
    let pending = Pending {
        owner: saved.id.clone(),
        index: 0,
        notes: saved.checkpoints.clone(),
    };
    app.tasks.selected = None;
    assert!(app.delete_checkpoint(&pending).is_err());
}
#[test]
fn native_delete_request_and_escape_cancel_themes_zoom() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            let mut app = fixture(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let (tx, rx) = mpsc::channel();
            app.tasks.store = Some(tx);
            let task = app.tasks.current().unwrap().clone();
            let before = serde_json::to_value(&task).unwrap();
            let mut view = work_item::View::default();
            let draw = |app: &mut Pui, view: &mut work_item::View, events| {
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
                        egui::CentralPanel::default()
                            .show(ctx, |ui| app.task_checkpoints(ui, &task, view));
                    },
                )
            };
            for _ in 0..3 {
                draw(&mut app, &mut view, vec![]);
            }
            let (_, rect, enabled) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new((
                        "checkpoint-delete-open",
                        2usize,
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
                            pressed,
                            button: egui::PointerButton::Primary,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert!(view.checkpoint_delete.is_some());
            for _ in 0..3 {
                draw(&mut app, &mut view, vec![]);
            }
            draw(
                &mut app,
                &mut view,
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    repeat: false,
                    physical_key: None,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert!(view.checkpoint_delete.is_none());
            assert_eq!(
                serde_json::to_value(app.tasks.current().unwrap()).unwrap(),
                before
            );
            assert!(rx.try_recv().is_err());
        }
    }
}
#[test]
fn native_confirmation_pointer_commits_once() {
    let ctx = egui::Context::default();
    let mut app = fixture(&ctx);
    let (tx, rx) = mpsc::channel();
    app.tasks.store = Some(tx);
    let task = app.tasks.current().unwrap().clone();
    let mut view = work_item::View::default();
    view.checkpoint_delete = Some(Pending {
        owner: task.id.clone(),
        index: 0,
        notes: task.checkpoints.clone(),
    });
    let draw = |app: &mut Pui, view: &mut work_item::View, events| {
        ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| app.checkpoint_delete_dialog(ctx, &task, view),
        )
    };
    for _ in 0..4 {
        draw(&mut app, &mut view, vec![]);
    }
    let output = draw(&mut app, &mut view, vec![]);
    let pos = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == "Delete note" => {
                Some(t.pos + t.galley.size() / 2.0)
            }
            _ => None,
        })
        .unwrap();
    for pressed in [true, false] {
        draw(
            &mut app,
            &mut view,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(view.checkpoint_delete.is_none());
    let StoreCommand::Save(saved) = rx.try_recv().unwrap() else {
        panic!()
    };
    assert_eq!(saved.checkpoints, task.checkpoints[1..]);
    assert_eq!(saved.phase, task.phase);
    assert!(rx.try_recv().is_err());
    assert!(app.tasks.processes.is_empty());
}
#[test]
fn changed_snapshot_cancels_editor_and_rekeys_identical_shifted_note() {
    let ctx = egui::Context::default();
    let mut app = fixture(&ctx);
    let mut view = work_item::View::default();
    let draw = |app: &mut Pui, view: &mut work_item::View| {
        let task = app.tasks.current().unwrap().clone();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.task_checkpoints(ui, &task, view));
        });
    };
    draw(&mut app, &mut view);
    let generation = view.checkpoint_generation;
    let task = app.tasks.current().unwrap();
    view.checkpoint_delete = Some(Pending {
        owner: task.id.clone(),
        index: 1,
        notes: task.checkpoints.clone(),
    });
    app.tasks.current_mut().unwrap().checkpoints.remove(1);
    draw(&mut app, &mut view);
    assert!(view.checkpoint_delete.is_none());
    assert!(view.checkpoint_generation > generation);
    app.demo = true;
    let task = app.tasks.current().unwrap();
    let pending = Pending {
        owner: task.id.clone(),
        index: 0,
        notes: task.checkpoints.clone(),
    };
    assert!(app.delete_checkpoint(&pending).is_err());
}
