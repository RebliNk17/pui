use super::*;
use crate::app::files::tests as file_tests;
fn control(ctx: &egui::Context, name: &str) -> (egui::Id, egui::Rect, bool) {
    ctx.data(|data| data.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new(name)))
        .unwrap()
}
fn app(ctx: &egui::Context) -> Pui {
    let mut app = file_tests::app(ctx);
    app.prefs.hide_sidebar = true;
    app.workspace = super::tests::workspace();
    for file in &mut app.workspace.files {
        file.status = "??".into();
    }
    app.open_diff_review(
        ("first.rs".into(), "@@ -1 +1 @@\n-old\n+kept\n".into()).into(),
        false,
    );
    app
}
fn completed(app: &mut Pui) {
    if let Some(pending) = &mut app.changes.git_files.pending {
        let result = pending.rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let (tx, rx) = mpsc::channel();
        tx.send(result).unwrap();
        pending.rx = rx;
    }
}

#[test]
fn demo_git_files_uses_real_navigation_metadata_and_untracked_requires_no_repository_read() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_changes: true,
            demo_git_files: true,
            ..Default::default()
        },
    );
    for _ in 0..4 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    assert!(
        app.client.is_none()
            && !app.changes.review.as_ref().unwrap().3
            && control(&ctx, "git-next-file").2
    );
    file_tests::click(&ctx, &mut app, control(&ctx, "git-next-file").1.center());
    completed(&mut app);
    file_tests::frame(&ctx, &mut app, vec![]);
    assert_eq!(app.changes.review.as_ref().unwrap().0, "second.txt");
}

#[test]
fn actual_pointer_and_enter_step_git_files_without_wrap_or_staging() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    for _ in 0..4 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    assert!(!control(&ctx, "git-previous-file").2);
    assert!(control(&ctx, "git-next-file").2);
    file_tests::click(&ctx, &mut app, control(&ctx, "git-next-file").1.center());
    completed(&mut app);
    file_tests::frame(&ctx, &mut app, vec![]);
    assert_eq!(app.changes.review.as_ref().unwrap().0, "second.rs");
    let (id, _, _) = control(&ctx, "git-next-file");
    ctx.memory_mut(|memory| memory.request_focus(id));
    file_tests::frame(&ctx, &mut app, vec![]);
    file_tests::frame(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            modifiers: egui::Modifiers::NONE,
            physical_key: None,
            pressed: true,
            repeat: false,
        }],
    );
    completed(&mut app);
    file_tests::frame(&ctx, &mut app, vec![]);
    assert_eq!(app.changes.review.as_ref().unwrap().0, "third.rs");
    assert!(!control(&ctx, "git-next-file").2);
    assert!(!app.changes.expanded());
}

#[test]
fn narrow_zoom_loading_failure_and_stale_labels_fit_and_settle_without_repeated_git() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            let mut time = 0.0;
            let mut draw = |app: &mut Pui| {
                time += 0.1;
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(480.0, 900.0),
                        )),
                        time: Some(time),
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(app, ctx, &mut eframe::Frame::_new_kittest()),
                )
            };
            for _ in 0..5 {
                draw(&mut app);
            }
            let workspace = app.workspace.clone();
            let tx = super::tests::inject(&mut app.changes, &workspace, 1);
            let out = draw(&mut app);
            file_tests::text(&out, "Loading…");
            assert_eq!(
                app.changes.git_file_navigation(&ctx).status,
                "Loading Git file 2 of 3…"
            );
            assert!(!control(&ctx, "git-next-file").2);
            tx.send(super::tests::loaded(
                &workspace,
                1,
                Err("Git unavailable. Previous review was kept.".into()),
            ))
            .unwrap();
            for _ in 0..40 {
                draw(&mut app);
            }
            let out = draw(&mut app);
            file_tests::text(&out, "Git unavailable");
            for name in ["git-next-file", "git-previous-file"] {
                assert!(ctx.screen_rect().contains_rect(control(&ctx, name).1));
            }
            assert!(
                out.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(1)
            );
            assert_eq!(app.changes.review.as_ref().unwrap().0, "first.rs");
            app.changes.invalidate();
            let out = draw(&mut app);
            file_tests::text(&out, "Git file list changed");
            assert!(!control(&ctx, "git-next-file").2);
        }
    }
}

#[test]
fn completed_read_waits_for_popup_then_hidden_owner_drops_it() {
    let ctx = egui::Context::default();
    let workspace = super::tests::workspace();
    let mut changes = super::tests::changes(&ctx, &workspace);
    let tx = super::tests::inject(&mut changes, &workspace, 1);
    tx.send(super::tests::loaded(&workspace, 1, Ok("+second".into())))
        .unwrap();
    let popup = egui::Id::new("git-file-test-popup");
    egui::Popup::open_id(&ctx, popup);
    changes.poll_git_files(&ctx, &workspace);
    assert_eq!(changes.review.as_ref().unwrap().0, "first.rs");
    assert!(changes.git_files.pending.is_some());
    egui::Popup::close_id(&ctx, popup);
    changes.review_inputs(&ctx, false, &[], 0, &workspace);
    assert!(changes.git_files.pending.is_none());
    assert_eq!(changes.review.as_ref().unwrap().0, "first.rs");
}
