use super::ui_tests::{click, key, pointer, select, setup};
use super::*;
use crate::app::files::tests::frame;
fn approval(app: &mut Pui) -> std::sync::mpsc::Receiver<serde_json::Value> {
    app.state.dialogs.push_back(json!({"id":"local-review-owned","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."}));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    commands
}
#[test]
fn first_native_tab_shift_tab_and_immediate_escape_cancel_editor_without_stop_deny_or_send() {
    for shift in [None, Some(false), Some(true)] {
        let (ctx, mut app) = setup();
        let commands = approval(&mut app);
        select(&ctx, &mut app, "chosen");
        click(&ctx, &mut app, "review-comment-add");
        assert!(app.changes.review_comments.editor.is_some());
        if let Some(shift) = shift {
            frame(
                &ctx,
                &mut app,
                vec![key(
                    egui::Key::Tab,
                    egui::Modifiers {
                        shift,
                        ..egui::Modifiers::NONE
                    },
                )],
            );
        }
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(
            app.changes.review_comments.editor.is_none(),
            "shift={shift:?}"
        );
        assert!(app.changes.review.is_some());
        assert_eq!(app.draft, "Kept draft");
        assert!(app.state.busy);
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(app.pending_dialog.is_none());
        assert!(commands.try_recv().is_err());
    }
}
#[test]
fn empty_saved_list_clear_all_shift_tab_then_escape_cancels_only_editor() {
    let (ctx, mut app) = setup();
    let commands = approval(&mut app);
    select(&ctx, &mut app, "chosen");
    click(&ctx, &mut app, "review-comment-add");
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("nonempty unsaved note".into())],
    );
    assert!(app.changes.review_comments.items.is_empty());
    let next = app.changes.review_comments.after_editor.unwrap();
    let clear = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("review-comment-clear")))
        .unwrap();
    assert!(clear.contains(ctx.read_response(next).unwrap().rect.center()));
    ctx.memory_mut(|memory| memory.request_focus(next));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)],
    );
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(app.changes.review_comments.editor.is_none());
    assert!(!app.changes.review_comments.clear_requested);
    assert!(app.changes.review.is_some());
    assert_eq!(app.draft, "Kept draft");
    assert!(app.state.busy);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.pending_dialog.is_none());
    assert!(commands.try_recv().is_err());
}
#[test]
fn external_resolve_shift_tab_then_immediate_escape_cancels_editor_not_run() {
    let (ctx, mut app) = setup();
    let commands = approval(&mut app);
    select(&ctx, &mut app, "chosen");
    click(&ctx, &mut app, "review-comment-add");
    frame(&ctx, &mut app, vec![egui::Event::Text("saved note".into())]);
    click(&ctx, &mut app, "review-comment-save");
    frame(&ctx, &mut app, vec![]);
    select(&ctx, &mut app, "chosen");
    click(&ctx, &mut app, "review-comment-add");
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text("unsaved note".into())],
    );
    let resolve = app.changes.review_comments.after_editor.unwrap();
    assert!(
        ctx.read_response(resolve)
            .is_some_and(|response| response.enabled())
    );
    ctx.memory_mut(|memory| memory.request_focus(resolve));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)],
    );
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(app.changes.review_comments.editor.is_none());
    assert_eq!(app.changes.review_comments.items.len(), 1);
    assert!(app.changes.review.is_some());
    assert!(app.state.busy);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.pending_dialog.is_none());
    assert_eq!(app.draft, "Kept draft");
    assert!(commands.try_recv().is_err());
}
#[test]
fn comment_pointer_press_release_from_composer_focus_claims_only_dangerous_chords() {
    for chord in [egui::Key::Enter, egui::Key::Backspace] {
        for chord_on_press in [true, false] {
            let (ctx, mut app) = setup();
            let commands = approval(&mut app);
            select(&ctx, &mut app, "chosen");
            // Keyboard focus is intentionally composer; exact label selection stays.
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
            let rect = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("review-comment-add")))
                .unwrap();
            for pressed in [true, false] {
                let mut events = pointer(rect.center(), pressed);
                if pressed == chord_on_press {
                    events.push(key(chord, egui::Modifiers::COMMAND));
                }
                frame(&ctx, &mut app, events);
            }
            assert!(
                app.changes.review_comments.editor.is_some(),
                "{chord:?} on press {chord_on_press}"
            );
            assert_eq!(app.draft, "Kept draft");
            assert!(app.state.busy);
            assert_eq!(app.state.dialogs.len(), 1);
            assert!(app.pending_dialog.is_none());
            assert!(commands.try_recv().is_err());
        }
    }
}
