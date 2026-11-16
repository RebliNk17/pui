use super::super::super::tests::{fixture, key};
use super::*;
use crate::app::files::tests::frame;
#[test]
fn pointer_parent_choice_themes_zoom_stale_target_and_load_identity() {
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
            let (id, rect) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect)>(popup_id().with(("commit-base", 1usize)))
                })
                .unwrap();
            for pressed in [true, false] {
                frame(
                    &ctx,
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
            assert_eq!(app.changes.branches.history.detail.comparison.base, 1);
            let expected = app
                .changes
                .branches
                .history
                .detail
                .result
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .parents[1]
                .clone();
            assert_eq!(expected, "c".repeat(40));
            app.changes.branches.history.detail.comparison.key = Some(id);
            app.changes
                .branches
                .history
                .detail
                .result
                .as_mut()
                .unwrap()
                .as_mut()
                .unwrap()
                .parents[1] = "d".repeat(40);
            app.changes.branches.history.detail.comparison.base = 0;
            frame(&ctx, &mut app, vec![]);
            assert_eq!(app.changes.branches.history.detail.comparison.base, 0);
            assert!(!app.changes.branches.history.detail.patch.pending());
            assert_eq!(app.draft, "Kept draft");
        }
    }
}
#[test]
fn explicit_parent_change_cancels_loaded_results_without_git_send_or_approval() {
    let (ctx, mut app) = fixture();
    app.changes
        .branches
        .history
        .open_detail(&ctx, &"a".repeat(40), true);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let id = ctx
        .data(|d| d.get_temp::<(egui::Id, egui::Rect)>(popup_id().with(("commit-base", 1usize))))
        .unwrap()
        .0;
    let cancel = Arc::new(AtomicBool::new(false));
    let (_tx, rx) = mpsc::channel();
    app.changes
        .branches
        .history
        .detail
        .patch
        .set_pending_for_test(rx, cancel.clone());
    ctx.memory_mut(|m| m.request_focus(id));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.dialogs.push_back(questions::demo_dialog());
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Space, egui::Modifiers::NONE),
            egui::Event::Text(" ".into()),
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
        ],
    );
    assert_eq!(app.changes.branches.history.detail.comparison.base, 1);
    assert!(cancel.load(Ordering::Acquire));
    assert!(!app.changes.branches.history.detail.patch.pending());
    assert_eq!(app.draft, "Kept draft");
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(commands.try_recv().is_err());
}
