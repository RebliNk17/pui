use super::tests::{pointer, pos};
use super::*;
use crate::app::files::tests::{app, frame};

fn setup() -> (egui::Context, Pui, PathBuf, &'static str) {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.prefs.hide_sidebar = true;
    app.prefs.hide_inspector = true;
    let source = "prefix selected source\nsecond line suffix\n";
    let root = app.prefs.cwd.clone();
    let path = root.join("selection.rs");
    std::fs::write(&path, source).unwrap();
    app.files
        .open_target(&ctx, &root, path.to_str().unwrap())
        .unwrap();
    for _ in 0..5 {
        frame(&ctx, &mut app, vec![]);
    }
    app.state.busy = true;
    app.draft = "Keep this draft".into();
    (ctx, app, path, source)
}
fn drag(ctx: &egui::Context, app: &mut Pui, source: &str, range: Range<usize>) -> egui::Pos2 {
    let output = frame(ctx, app, vec![]);
    let start = pos(&output, source, range.start);
    let end = pos(&output, source, range.end);
    frame(ctx, app, pointer(start, egui::PointerButton::Primary, true));
    frame(ctx, app, vec![egui::Event::PointerMoved(end)]);
    frame(ctx, app, pointer(end, egui::PointerButton::Primary, false));
    for pressed in [true, false] {
        frame(
            ctx,
            app,
            pointer(start, egui::PointerButton::Secondary, pressed),
        );
    }
    for _ in 0..3 {
        frame(ctx, app, vec![]);
    }
    start
}
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn file_viewer_exact_multiline_selection_is_read_only_scoped_and_draft_only() {
    let (ctx, mut app, path, source) = setup();
    let count = app.state.messages.len();
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    drag(&ctx, &mut app, source, 7..33);
    let snapshot = ctx
        .data(|data| data.get_temp::<SourceMenu>(super::key()))
        .unwrap();
    assert_eq!(
        snapshot.selected.as_ref().unwrap().text,
        "selected source\nsecond lin"
    );
    assert!(
        snapshot
            .selected
            .as_ref()
            .unwrap()
            .label
            .contains("selection.rs:1–2")
    );
    let target = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut app,
            pointer(target, egui::PointerButton::Primary, pressed),
        );
    }
    assert!(app.draft.starts_with("Keep this draft\n\n"));
    assert!(app.draft.contains("selected source\nsecond lin"));
    assert!(!app.draft.contains("prefix"));
    assert!(!app.draft.contains("suffix"));
    assert_eq!(app.state.messages.len(), count);
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    assert!(ctx.data(|data| data.get_temp::<SourceMenu>(super::key()).is_none()));
}

#[test]
fn full_app_source_menu_shortcuts_cannot_stop_send_or_approve_and_close_releases() {
    let (ctx, mut app, _path, source) = setup();
    let permission = json!({"id":"source-selection-permission", "method":"confirm", "title":"Allow bash?", "message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."});
    assert!(prompts::approval(&permission).is_some());
    app.state.dialogs.push_back(permission);
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    drag(&ctx, &mut app, source, 7..15);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
    );
    assert_eq!(app.draft, "Keep this draft");
    assert!(app.pending_dialog.is_none());
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(app.state.busy);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(commands.try_recv().is_err());
    assert!(!egui::Popup::is_any_open(&ctx));
    app.state.dialogs.clear();
    drag(&ctx, &mut app, source, 7..15);
    app.files.close();
    frame(&ctx, &mut app, vec![]);
    assert!(ctx.data(|data| data.get_temp::<SourceMenu>(super::key()).is_none()));
    assert!(!egui::Popup::is_any_open(&ctx));
}

#[test]
fn opening_another_source_file_does_not_reuse_the_previous_selection() {
    let (ctx, mut app, _path, source) = setup();
    drag(&ctx, &mut app, source, 7..15);
    let path = app.prefs.cwd.join("other.rs");
    std::fs::write(&path, source).unwrap();
    app.files
        .open_target(&ctx, &app.prefs.cwd, path.to_str().unwrap())
        .unwrap();
    assert!(ctx.data(|data| data.get_temp::<SourceMenu>(super::key()).is_none()));
    assert!(!egui::text_selection::LabelSelectionState::load(&ctx).has_selection());
    for _ in 0..5 {
        frame(&ctx, &mut app, vec![]);
    }
    assert_eq!(app.draft, "Keep this draft");
}
