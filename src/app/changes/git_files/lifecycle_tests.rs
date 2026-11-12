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

fn pending_open(app: &mut Pui) -> mpsc::Sender<Loaded> {
    let (tx, rx) = mpsc::channel();
    app.diff_rx = Some(rx);
    tx
}

#[test]
fn actual_files_and_browser_toolbar_navigation_cancels_late_open_permanently() {
    for button in ["files-toolbar-button", "browser-toolbar-button"] {
        for ready_before_navigation in [false, true] {
            let ctx = egui::Context::default();
            let mut app = file_tests::app(&ctx);
            app.prefs.hide_sidebar = true;
            app.workspace = super::tests::workspace();
            app.draft = "Keep this draft".into();
            file_tests::frame(&ctx, &mut app, vec![]);
            file_tests::frame(&ctx, &mut app, vec![]);
            let pos = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(button)))
                .unwrap()
                .center();
            let tx = pending_open(&mut app);
            let result = super::tests::loaded(&app.workspace, 1, Ok("+late body".into()));
            if ready_before_navigation {
                tx.send(result).unwrap();
            }
            file_tests::click(&ctx, &mut app, pos);
            assert!(app.files.is_open() && !app.inspector_panel);
            assert!(app.diff_rx.is_none());
            if !ready_before_navigation {
                assert!(
                    tx.send(super::tests::loaded(&app.workspace, 1, Ok("+late".into())))
                        .is_err()
                );
            }
            assert_eq!(app.draft, "Keep this draft");
            app.files.close();
            app.focus_composer = true;
            file_tests::frame(&ctx, &mut app, vec![]);
            file_tests::frame(&ctx, &mut app, vec![]);
            assert!(!app.inspector_panel && app.diff_rx.is_none());
            assert_eq!(app.draft, "Keep this draft");
            assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new("composer"))));
        }
    }
}

#[test]
fn slash_and_history_file_destinations_drop_inflight_open_instead_of_deferring() {
    for command in ["/files", "/browser"] {
        let ctx = egui::Context::default();
        let mut app = file_tests::app(&ctx);
        app.workspace = super::tests::workspace();
        app.remember_navigation();
        let tx = pending_open(&mut app);
        app.draft = command.into();
        assert!(app.submit_command());
        app.draft = "Keep".into();
        file_tests::frame(&ctx, &mut app, vec![]);
        assert!(app.files.is_open() && app.diff_rx.is_none());
        assert!(
            tx.send(super::tests::loaded(&app.workspace, 1, Ok("+late".into())))
                .is_err()
        );
        app.navigate_history(false);
        file_tests::frame(&ctx, &mut app, vec![]);
        assert!(!app.files.is_open() && !app.inspector_panel);
        let tx = pending_open(&mut app);
        app.navigate_history(true);
        assert!(app.files.is_open());
        file_tests::frame(&ctx, &mut app, vec![]);
        assert!(app.diff_rx.is_none());
        assert!(
            tx.send(super::tests::loaded(
                &app.workspace,
                1,
                Ok("+late history".into())
            ))
            .is_err()
        );
        app.files.close();
        file_tests::frame(&ctx, &mut app, vec![]);
        assert!(!app.inspector_panel && app.draft == "Keep");
    }
}

#[test]
fn initial_async_completion_preserves_composer_draft_and_focus() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    app.workspace = super::tests::workspace();
    app.draft = "Draft in progress".into();
    for _ in 0..3 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
    let tx = pending_open(&mut app);
    tx.send(super::tests::loaded(&app.workspace, 1, Ok("+ready".into())))
        .unwrap();
    file_tests::frame(&ctx, &mut app, vec![]);
    assert_eq!(app.changes.review.as_ref().unwrap().0, "second.rs");
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new("composer"))));
    assert_eq!(app.draft, "Draft in progress");
}

#[test]
fn navigation_open_then_close_before_another_frame_still_cancels_the_request() {
    for command in ["/files", "/browser"] {
        let ctx = egui::Context::default();
        let mut app = file_tests::app(&ctx);
        app.workspace = super::tests::workspace();
        let tx = pending_open(&mut app);
        for _ in 0..2 {
            app.draft = command.into();
            assert!(app.submit_command());
        }
        assert!(!app.files.is_open() && app.diff_rx.is_none());
        assert!(
            tx.send(super::tests::loaded(&app.workspace, 1, Ok("+late".into())))
                .is_err()
        );
        file_tests::frame(&ctx, &mut app, vec![]);
        assert!(!app.inspector_panel);
    }
}

#[test]
fn initial_read_error_and_disconnected_worker_remain_notices_not_patch_documents() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    app.workspace = super::tests::workspace();
    let tx = pending_open(&mut app);
    tx.send(super::tests::loaded(
        &app.workspace,
        1,
        Err("fixture read failed".into()),
    ))
    .unwrap();
    file_tests::frame(&ctx, &mut app, vec![]);
    assert!(
        app.state
            .notice
            .as_ref()
            .unwrap()
            .contains("fixture read failed")
    );
    assert!(app.diff_rx.is_none() && app.changes.review.is_none());
    let tx = pending_open(&mut app);
    drop(tx);
    file_tests::frame(&ctx, &mut app, vec![]);
    assert!(
        app.state
            .notice
            .as_ref()
            .unwrap()
            .contains("without a result")
    );
    assert!(app.diff_rx.is_none() && app.changes.review.is_none());
}
