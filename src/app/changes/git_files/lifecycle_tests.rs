use super::*;
use crate::app::files::tests as file_tests;

#[test]
fn explicit_replacement_and_refreshed_workspace_drop_initial_open_result() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    app.workspace = super::tests::workspace();
    let workspace = app.workspace.clone();
    let (tx, rx) = mpsc::channel();
    app.diff_rx = Some(rx);
    tx.send(super::tests::loaded(&workspace, 1, Ok("+old read".into())))
        .unwrap();
    app.open_diff_review(
        ("recorded.rs".into(), "+explicit replacement".into()).into(),
        true,
    );
    assert!(app.diff_rx.is_none());
    file_tests::frame(&ctx, &mut app, vec![]);
    assert_eq!(app.changes.review.as_ref().unwrap().0, "recorded.rs");
    let (tx, rx) = mpsc::channel();
    app.diff_rx = Some(rx);
    tx.send(super::tests::loaded(&workspace, 1, Ok("+stale".into())))
        .unwrap();
    let (tx, rx) = mpsc::channel();
    app.workspace_rx = Some(rx);
    tx.send(workspace.clone()).unwrap();
    file_tests::frame(&ctx, &mut app, vec![]);
    assert!(app.diff_rx.is_none());
    assert_eq!(app.changes.review.as_ref().unwrap().0, "recorded.rs");
}

#[test]
fn information_modal_defers_completed_body_and_initial_open_does_not_close_settings() {
    let ctx = egui::Context::default();
    let workspace = super::tests::workspace();
    let mut changes = super::tests::changes(&ctx, &workspace);
    for _ in 0..3 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            changes.poll_git_files(ctx, &workspace);
            egui::CentralPanel::default().show(ctx, |ui| {
                diff_viewer::contents(ui, changes.review.as_mut().unwrap());
            });
            prompt_ui::modal(ctx, egui::Id::new("git-review-info"), 320.0, |ui| {
                prompt_ui::heading(ui, "Info", "");
            });
        });
    }
    let tx = super::tests::inject(&mut changes, &workspace, 1);
    tx.send(super::tests::loaded(&workspace, 1, Ok("+second".into())))
        .unwrap();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        changes.poll_git_files(ctx, &workspace);
        prompt_ui::modal(ctx, egui::Id::new("git-review-info"), 320.0, |ui| {
            prompt_ui::heading(ui, "Info", "");
        });
    });
    assert_eq!(changes.review.as_ref().unwrap().0, "first.rs");
    assert!(changes.git_files.pending.is_some());
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    app.workspace = workspace.clone();
    app.settings_open = true;
    let (tx, rx) = mpsc::channel();
    app.diff_rx = Some(rx);
    tx.send(super::tests::loaded(
        &workspace,
        1,
        Ok("+pending explicit open".into()),
    ))
    .unwrap();
    file_tests::frame(&ctx, &mut app, vec![]);
    assert!(app.settings_open && app.diff_rx.is_some());
    assert!(app.changes.review.is_none());
    app.settings_open = false;
    file_tests::frame(&ctx, &mut app, vec![]);
    assert_eq!(app.changes.review.as_ref().unwrap().0, "second.rs");
}
