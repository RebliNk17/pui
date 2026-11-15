use super::super::tests::{fixture, key};
use super::*;
use crate::app::files::tests::frame;
fn loaded() -> (egui::Context, Pui) {
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
    (ctx, app)
}
fn parent(ctx: &egui::Context) -> (egui::Id, egui::Rect) {
    ctx.data(|data| data.get_temp(popup_id().with(("inspect-parent", 0usize))))
        .unwrap()
}
#[test]
fn pointer_parent_inspection_uses_exact_loaded_oid_retains_list_filter_and_no_send() {
    let (ctx, mut app) = loaded();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    // Full OIDs may wrap; reveal the real parent control before pointer activation.
    for _ in 0..4 {
        ctx.read_response(parent(&ctx).0)
            .unwrap()
            .scroll_to_me(Some(egui::Align::Center));
        frame(&ctx, &mut app, vec![]);
    }
    let response = ctx.read_response(parent(&ctx).0).unwrap();
    let pos = response.interact_rect.center();
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
                key(egui::Key::Enter, egui::Modifiers::COMMAND),
                key(egui::Key::Backspace, egui::Modifiers::COMMAND),
            ],
        );
    }
    let history = &mut app.changes.branches.history;
    assert_eq!(
        history.detail.selected.as_deref(),
        Some("b".repeat(40).as_str())
    );
    assert_eq!(history.snapshot.as_ref().unwrap().head, "a".repeat(40));
    assert_eq!(history.detail.requests, 0);
    history.close_detail(&ctx);
    assert!(!history.has_detail());
    assert_eq!(app.draft, "Kept draft");
    assert!(commands.try_recv().is_err());
    assert!(app.state.busy);
}
#[test]
fn native_tab_parent_enter_space_new_question_and_escape_are_owned() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app) = loaded();
        let id = parent(&ctx).0;
        for _ in 0..12 {
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
            );
            if ctx.memory(|memory| memory.has_focus(id)) {
                break;
            }
        }
        assert!(ctx.memory(|memory| memory.has_focus(id)));
        app.state.dialogs.push_back(questions::demo_dialog());
        let mut events = vec![key(activation, egui::Modifiers::NONE)];
        if activation == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        frame(&ctx, &mut app, events);
        assert_eq!(
            app.changes.branches.history.detail.selected.as_deref(),
            Some("b".repeat(40).as_str())
        );
        assert_eq!(app.state.dialogs.len(), 1);
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(!app.changes.branches.open);
        assert_eq!(app.draft, "Kept draft");
    }
}
#[test]
fn old_parent_control_cannot_adopt_changed_source_target_or_missing_snapshot_member() {
    for case in 0..3 {
        let (ctx, mut app) = loaded();
        let old = parent(&ctx).0;
        let history = &mut app.changes.branches.history;
        match case {
            0 => {
                history.detail.selected = Some("b".repeat(40));
                history
                    .detail
                    .result
                    .as_mut()
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .oid = "b".repeat(40);
            }
            1 => {
                history
                    .detail
                    .result
                    .as_mut()
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .parents[0] = "c".repeat(40)
            }
            _ => history
                .snapshot
                .as_mut()
                .unwrap()
                .commits
                .pop()
                .map(|_| ())
                .unwrap(),
        }
        ctx.memory_mut(|memory| memory.request_focus(old));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert_eq!(
            app.changes.branches.history.detail.selected.as_deref(),
            Some(
                if case == 0 {
                    "b".repeat(40)
                } else {
                    "a".repeat(40)
                }
                .as_str()
            )
        );
        assert!(app.changes.branches.history.detail.pending.is_none());
    }
}
