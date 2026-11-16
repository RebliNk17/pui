use super::super::super::tests::{fixture, key};
use super::*;
use crate::app::files::tests::frame;
#[test]
fn pointer_load_narrow_themes_zoom_and_owner_change_drop_file_request() {
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
                .data(|d| d.get_temp::<(egui::Id, egui::Rect)>(popup_id().with("commit-files")))
                .unwrap()
                .0;
            for _ in 0..20 {
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
            assert!(
                app.changes.branches.history.detail.files.result.is_some(),
                "{mode:?} {zoom} {:?}",
                ctx.read_response(id)
            );
            let cancel = Arc::new(AtomicBool::new(false));
            let (_tx, rx) = mpsc::channel();
            app.changes.branches.history.detail.files.pending =
                Some(workspace::read_only::Request::new(rx, cancel.clone()));
            app.changes.branches.history.poll(None);
            assert!(cancel.load(Ordering::Acquire));
            assert!(app.changes.branches.history.detail.files.result.is_none());
            assert_eq!(app.draft, "Kept draft");
        }
    }
}
#[test]
fn explicit_file_load_space_keeps_draft_question_no_rpc_and_back_drops_data() {
    let (ctx, mut app) = fixture();
    assert!(
        app.changes
            .branches
            .history
            .open_detail(&ctx, &"a".repeat(40), true)
    );
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(app.changes.branches.history.detail.files.result.is_none());
    let id = ctx
        .data(|d| d.get_temp::<(egui::Id, egui::Rect)>(popup_id().with("commit-files")))
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
            key(egui::Key::Space, egui::Modifiers::NONE),
            egui::Event::Text(" ".into()),
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
        ],
    );
    assert_eq!(
        app.changes
            .branches
            .history
            .detail
            .files
            .result
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(app.draft, "Kept draft");
    assert!(commands.try_recv().is_err());
    assert_eq!(app.state.dialogs.len(), 1);
    app.changes.branches.history.close_detail(&ctx);
    assert!(app.changes.branches.history.detail.files.result.is_none());
}
