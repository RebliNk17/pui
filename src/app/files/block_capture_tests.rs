//! Real viewer wiring: owner binding, load identity, refresh/close and deferred capture.
use super::*;
use tests::{app, click, frame, settle};

fn key() -> egui::Id {
    egui::Id::new("file-rendered-block-capture")
}
fn control(ctx: &egui::Context, name: &str) -> egui::Pos2 {
    ctx.data(|data| data.get_temp::<egui::Rect>(key().with(name)))
        .unwrap()
        .center()
}
fn block(ctx: &egui::Context, index: usize) -> egui::Pos2 {
    ctx.data(|data| data.get_temp::<egui::Rect>(key().with(("block", index))))
        .unwrap()
        .center()
}
fn draw(ctx: &egui::Context, app: &mut Pui) {
    for _ in 0..4 {
        frame(ctx, app, vec![]);
    }
}
fn start(ctx: &egui::Context, app: &mut Pui) {
    draw(ctx, app);
    click(ctx, app, control(ctx, "launch"));
    draw(ctx, app);
    click(ctx, app, block(ctx, 0));
    click(ctx, app, block(ctx, 2));
    click(ctx, app, control(ctx, "action"));
    draw(ctx, app);
}

#[test]
fn real_markdown_file_modal_captures_once_and_rejects_refreshed_or_closed_document() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.prefs.hide_sidebar = true;
    app.prefs.hide_inspector = true;
    let root = app.prefs.cwd.clone();
    let path = root.join("multi-block.md");
    let source = "First **שלום**.\n\nOmitted middle.\n\n```text\n世界\n\nlast\n```";
    fs::write(&path, source).unwrap();
    app.files
        .open_target(&ctx, &root, path.to_str().unwrap())
        .unwrap();
    settle(&mut app.files);
    app.draft = "Original".into();
    app.context_chips.clear_quotes();
    let messages = app.state.messages.len();
    let (client, _events, commands) = crate::rpc::Client::stub_with_commands();
    app.client = Some(client);
    let permission = json!({"id":"file-block-permission", "method":"confirm", "title":"Allow bash?", "message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."});
    assert!(prompts::approval(&permission).is_some());
    app.state.dialogs.push_back(permission.clone());
    app.state.busy = true;
    start(&ctx, &mut app);
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }],
    );
    frame(&ctx, &mut app, vec![egui::Event::Text("y".into())]);
    assert!(app.pending_dialog.is_none());
    assert_eq!(app.state.dialogs.front(), Some(&permission));
    assert_eq!(app.outgoing_text(), "Original");
    click(&ctx, &mut app, control(&ctx, "action"));
    draw(&ctx, &mut app);
    assert!(
        app.outgoing_text()
            .contains("[Rendered block 1 · lines 1–1 within this block]\nFirst שלום.")
    );
    assert!(
        app.outgoing_text()
            .contains("[Rendered block 3 · lines 1–3 within this block]\n世界\n\nlast\n")
    );
    assert!(!app.outgoing_text().contains("Omitted middle"));
    assert_eq!(app.outgoing_text().matches("[Context:").count(), 1);
    assert_eq!(app.state.messages.len(), messages);
    assert!(
        commands.try_recv().is_err(),
        "no provider or permission command"
    );
    let captured = app.outgoing_text().clone();
    draw(&ctx, &mut app);
    assert_eq!(app.outgoing_text(), captured);

    start(&ctx, &mut app);
    // Refresh allocates a fresh load identity even when path/text are unchanged.
    let old = app.files.tabs[0]._budget.document();
    app.files.refresh(&ctx);
    settle(&mut app.files);
    assert_ne!(old, app.files.tabs[0]._budget.document());
    draw(&ctx, &mut app);
    click(&ctx, &mut app, control(&ctx, "action"));
    assert_eq!(app.outgoing_text(), captured);

    start(&ctx, &mut app);
    // Replacing the loaded document text in the same tab cannot reuse review.
    app.files.tabs[0].content = Some(Ok(Content::Text("Replacement body".into())));
    draw(&ctx, &mut app);
    click(&ctx, &mut app, control(&ctx, "action"));
    assert_eq!(app.outgoing_text(), captured);
    click(&ctx, &mut app, control(&ctx, "cancel"));
    draw(&ctx, &mut app);
    app.files.refresh(&ctx);
    settle(&mut app.files);
    start(&ctx, &mut app);
    app.files.close_tab(0);
    draw(&ctx, &mut app);
    assert_eq!(app.outgoing_text(), captured);
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    assert_eq!(app.state.dialogs.front(), Some(&permission));
    assert!(app.pending_dialog.is_none());
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
}
