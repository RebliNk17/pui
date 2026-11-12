use super::*;

fn render(app: &mut Pui, task: &Task, preview: &mut Preview) {
    let ctx = app.ctx.clone();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            app.task_changes_preview(ui, task, preview);
        });
    });
}
fn app() -> Pui {
    Pui::new(
        &eframe::CreationContext::_new_kittest(egui::Context::default()),
        Options {
            demo: true,
            ..Default::default()
        },
    )
}

#[test]
fn viewing_changes_does_not_start_preview_or_authorize_work() {
    let mut app = app();
    app.demo_task();
    let task = app.tasks.current().unwrap().clone();
    let before = serde_json::to_value(&task).unwrap();
    let mut preview = Preview::default();
    for _ in 0..3 {
        render(&mut app, &task, &mut preview);
    }
    assert!(preview.pending.is_none());
    assert!(preview.result.is_none());
    assert_eq!(serde_json::to_value(app.tasks.current()).unwrap(), before);
    assert!(app.tasks.job.is_none() && app.tasks.processes.is_empty());
    assert!(app.state.dialogs.is_empty());
}

#[test]
fn changed_record_discards_pending_preview_and_retained_result() {
    let mut app = app();
    app.demo_task();
    let mut task = app.tasks.current().unwrap().clone();
    let record = serde_json::to_string(&task).unwrap();
    let (sender, reply) = mpsc::channel();
    let mut preview = Preview {
        pending: Some(Pending {
            task: task.id.clone(),
            record: record.clone(),
            reply,
        }),
        record,
        result: Some(Err("Old preview".into())),
    };
    sender.send(Err("Obsolete result".into())).unwrap();
    task.note.push_str(" updated authoritative workflow");
    render(&mut app, &task, &mut preview);
    assert!(preview.pending.is_none() && preview.result.is_none());
    assert_eq!(preview.record, serde_json::to_string(&task).unwrap());
}

#[test]
fn mismatched_task_reply_is_ignored_without_starting_another_job() {
    let mut app = app();
    app.demo_task();
    let task = app.tasks.current().unwrap().clone();
    let record = serde_json::to_string(&task).unwrap();
    let (sender, reply) = mpsc::channel();
    let mut preview = Preview {
        pending: Some(Pending {
            task: "different-task".into(),
            record: record.clone(),
            reply,
        }),
        record,
        result: None,
    };
    sender.send(Err("Different workflow".into())).unwrap();
    render(&mut app, &task, &mut preview);
    assert!(preview.pending.is_none() && preview.result.is_none());
    assert!(app.tasks.job.is_none() && app.tasks.processes.is_empty());
}
