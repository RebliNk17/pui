use super::ui_tests::{click, key, select, setup};
use super::*;
use crate::app::files::tests::frame;
#[test]
fn local_resolve_activation_cannot_answer_new_question() {
    for keycode in [egui::Key::Enter, egui::Key::Space, egui::Key::Num1] {
        let (ctx, mut app) = setup();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        select(&ctx, &mut app, "chosen");
        click(&ctx, &mut app, "review-comment-add");
        frame(&ctx, &mut app, vec![egui::Event::Text("note".into())]);
        click(&ctx, &mut app, "review-comment-save");
        frame(&ctx, &mut app, vec![]);
        let resolve = app.changes.review_comments.after_editor.unwrap();
        ctx.memory_mut(|memory| memory.request_focus(resolve));
        let mut dialog = questions::demo_dialog();
        let mut payload: Value =
            serde_json::from_str(dialog["placeholder"].as_str().unwrap()).unwrap();
        let first = payload["questions"][0].clone();
        payload["questions"] = json!([first]);
        dialog["placeholder"] = json!(payload.to_string());
        app.state.dialogs.push_back(dialog);
        let mut events = vec![key(keycode, egui::Modifiers::NONE)];
        if keycode == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        frame(&ctx, &mut app, events);
        if keycode != egui::Key::Num1 {
            assert!(app.changes.review_comments.items[0].resolved);
        }
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(app.pending_dialog.is_none());
        assert!(commands.try_recv().is_err());
        assert_eq!(app.draft, "Kept draft");
    }
}
#[test]
fn initial_question_cannot_reuse_comment_typing_numbers_or_enter_then_recovers_after_cancel() {
    let (ctx, mut app) = setup();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    select(&ctx, &mut app, "chosen");
    click(&ctx, &mut app, "review-comment-add");
    app.state.dialogs.push_back(questions::demo_dialog());
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::Text("note 1".into()),
            key(egui::Key::Num1, egui::Modifiers::NONE),
            key(egui::Key::Enter, egui::Modifiers::NONE),
        ],
    );
    assert!(
        app.changes
            .review_comments
            .editor
            .as_ref()
            .unwrap()
            .text
            .contains("note 1")
    );
    assert!(app.changes.local_comment_editor_owns_keyboard(&ctx));
    assert!(app.pending_dialog.is_none());
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(commands.try_recv().is_err());
    // Entering custom text belongs only to the local editor even if a new
    // free-text question is initialized in that same render pass.
    app.state.dialogs.clear();
    let mut dialog = questions::demo_dialog();
    dialog["id"] = json!("free-text-local-review");
    let message: Value = serde_json::from_str(dialog["placeholder"].as_str().unwrap()).unwrap();
    let mut free = message;
    free["questions"] = json!([{"id":"free","question":"Explain?","options":[]}]);
    dialog["placeholder"] = json!(free.to_string());
    app.state.dialogs.push_back(dialog);
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Text(" comment only".into())],
    );
    assert!(
        app.changes
            .review_comments
            .editor
            .as_ref()
            .unwrap()
            .text
            .contains("comment only")
    );
    assert!(app.changes.local_comment_editor_owns_keyboard(&ctx));
    assert!(commands.try_recv().is_err());
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(app.changes.review_comments.editor.is_none());
    assert!(app.pending_dialog.is_none());
    assert!(commands.try_recv().is_err());
    assert_eq!(app.draft, "Kept draft");
}
