use super::super::super::tests::{fixture, key};
use super::*;
use crate::app::files::tests::frame;
#[test]
fn loaded_file_enter_targets_exact_path_and_keeps_question_draft_rpc_unchanged() {
    let (ctx, mut app) = fixture();
    app.changes
        .branches
        .history
        .open_detail(&ctx, &"a".repeat(40), true);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let load = ctx
        .data(|d| d.get_temp::<(egui::Id, egui::Rect)>(popup_id().with("commit-files")))
        .unwrap()
        .0;
    ctx.memory_mut(|m| m.request_focus(load));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    let mut release = key(egui::Key::Enter, egui::Modifiers::NONE);
    if let egui::Event::Key { pressed, .. } = &mut release {
        *pressed = false;
    }
    frame(&ctx, &mut app, vec![release]);
    let row = ctx
        .data(|d| {
            d.get_temp::<(egui::Id, egui::Rect)>(
                popup_id().with(("commit-file-patch", "src/example.rs")),
            )
        })
        .unwrap()
        .0;
    for _ in 0..4 {
        ctx.read_response(row)
            .unwrap()
            .scroll_to_me(Some(egui::Align::Center));
        frame(&ctx, &mut app, vec![]);
    }
    let row = ctx
        .data(|d| {
            d.get_temp::<(egui::Id, egui::Rect)>(
                popup_id().with(("commit-file-patch", "src/example.rs")),
            )
        })
        .unwrap()
        .0;
    assert!(
        app.changes
            .branches
            .history
            .detail
            .files
            .rows
            .contains(&row)
    );
    ctx.memory_mut(|m| m.request_focus(row));
    frame(&ctx, &mut app, vec![]);
    assert!(
        ctx.memory(|m| m.has_focus(row)),
        "row focus {:?}",
        ctx.read_response(row)
    );
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.dialogs.push_back(questions::demo_dialog());
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Enter, egui::Modifiers::NONE),
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
        ],
    );
    assert_eq!(
        app.changes.branches.history.detail.patch.path.as_deref(),
        Some("src/example.rs")
    );
    assert!(app.changes.branches.history.detail.patch.result.is_some());
    assert_eq!(app.state.dialogs.len(), 1);
    assert_eq!(app.draft, "Kept draft");
    assert!(commands.try_recv().is_err());
}
#[test]
fn file_row_pointer_themes_zoom_and_changed_path_rejects_staged_old_control() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app) = fixture();
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            app.changes
                .branches
                .history
                .open_detail(&ctx, &"a".repeat(40), true);
            app.changes.branches.history.detail.files.result =
                Some(Ok(vec![workspace::history::files::Entry {
                    status: 'M',
                    path: "src/example.rs".into(),
                }]));
            for _ in 0..4 {
                frame(&ctx, &mut app, vec![]);
            }
            let row = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect)>(
                        popup_id().with(("commit-file-patch", "src/example.rs")),
                    )
                })
                .unwrap()
                .0;
            for _ in 0..4 {
                ctx.read_response(row)
                    .unwrap()
                    .scroll_to_me(Some(egui::Align::Center));
                frame(&ctx, &mut app, vec![]);
            }
            let pos = ctx.read_response(row).unwrap().interact_rect.center();
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert_eq!(
                app.changes.branches.history.detail.patch.path.as_deref(),
                Some("src/example.rs")
            );
            app.changes.branches.history.detail.patch = Default::default();
            app.changes.branches.history.detail.files.row_key = Some(row);
            app.changes
                .branches
                .history
                .detail
                .files
                .result
                .as_mut()
                .unwrap()
                .as_mut()
                .unwrap()[0]
                .path = "other.rs".into();
            frame(&ctx, &mut app, vec![]);
            assert!(app.changes.branches.history.detail.patch.path.is_none());
        }
    }
}
#[test]
fn pointer_patch_load_in_themes_zoom_and_stale_control_refusal() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app) = fixture();
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            app.changes
                .branches
                .history
                .open_detail(&ctx, &"a".repeat(40), true);
            for _ in 0..4 {
                frame(&ctx, &mut app, vec![]);
            }
            let id = ctx
                .data(|d| d.get_temp::<(egui::Id, egui::Rect)>(popup_id().with("commit-patch")))
                .unwrap()
                .0;
            for _ in 0..4 {
                ctx.read_response(id)
                    .unwrap()
                    .scroll_to_me(Some(egui::Align::Center));
                frame(&ctx, &mut app, vec![]);
            }
            let pos = ctx.read_response(id).unwrap().interact_rect.center();
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert!(app.changes.branches.history.detail.patch.result.is_some());
            app.changes
                .branches
                .history
                .open_detail(&ctx, &"b".repeat(40), true);
            ctx.memory_mut(|m| m.request_focus(id));
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
            );
            assert!(app.changes.branches.history.detail.patch.result.is_none());
            assert_eq!(app.draft, "Kept draft");
        }
    }
}
#[test]
fn explicit_patch_native_activation_preserves_draft_question_and_cancels_on_owner_change() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app) = fixture();
        app.changes
            .branches
            .history
            .open_detail(&ctx, &"a".repeat(40), true);
        for _ in 0..4 {
            frame(&ctx, &mut app, vec![]);
        }
        let id = ctx
            .data(|d| d.get_temp::<(egui::Id, egui::Rect)>(popup_id().with("commit-patch")))
            .unwrap()
            .0;
        ctx.memory_mut(|m| m.request_focus(id));
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.state.dialogs.push_back(questions::demo_dialog());
        frame(
            &ctx,
            &mut app,
            vec![
                key(activation, egui::Modifiers::NONE),
                egui::Event::Text(" ".into()),
                key(egui::Key::Enter, egui::Modifiers::COMMAND),
                key(egui::Key::Backspace, egui::Modifiers::COMMAND),
            ],
        );
        assert!(
            app.changes
                .branches
                .history
                .detail
                .patch
                .result
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .contains("diff --git")
        );
        assert!(commands.try_recv().is_err());
        assert_eq!(app.draft, "Kept draft");
        assert_eq!(app.state.dialogs.len(), 1);
        let cancel = Arc::new(AtomicBool::new(false));
        let (_tx, rx) = mpsc::channel();
        app.changes.branches.history.detail.patch.pending =
            Some(workspace::read_only::Request::new(rx, cancel.clone()));
        app.changes.branches.history.poll(None);
        assert!(cancel.load(Ordering::Acquire));
        assert!(app.changes.branches.history.detail.patch.result.is_none());
    }
}
