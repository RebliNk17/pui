use super::*;
use crate::app::files::tests::{app, click, frame};
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
fn fixture(ctx: &egui::Context) -> Pui {
    let mut app = app(ctx);
    let path = app.prefs.cwd.join("input.csv");
    fs::write(&path, "Name,Note\nBeta,=1+2\nAlpha,body\n").unwrap();
    app.files
        .open_target(ctx, &app.prefs.cwd.clone(), path.to_str().unwrap())
        .unwrap();
    for _ in 0..8 {
        frame(ctx, &mut app, vec![]);
    }
    app.state.busy = true;
    app.draft = "Kept".into();
    app
}
#[test]
fn cell_pointer_and_first_focus_escape_never_answer_or_stop() {
    for press in [true, false] {
        for code in [egui::Key::Enter, egui::Key::Backspace] {
            let ctx = egui::Context::default();
            let mut app = fixture(&ctx);
            app.state.dialogs.push_back(json!({"id":"table-input","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo safe\"}\n\nApprove this call only."}));
            let (client, _, commands) = rpc::Client::stub_with_records();
            app.client = Some(client);
            for _ in 0..5 {
                frame(&ctx, &mut app, vec![]);
            }
            let cell = ctx
                .data(|d| {
                    d.get_temp::<egui::Rect>(egui::Id::new((
                        "delimited-cell",
                        2usize,
                        1usize,
                        false,
                    )))
                })
                .unwrap();
            let view = ctx
                .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
                .unwrap();
            let pos = cell.intersect(view).center();
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("composer")));
            for pressed in [true, false] {
                let mut events = vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ];
                if pressed == press {
                    events.push(key(code, egui::Modifiers::COMMAND));
                }
                frame(&ctx, &mut app, events);
            }
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
            );
            assert_eq!(app.state.dialogs.len(), 1);
            assert!(commands.try_recv().is_err());
            assert_eq!(app.draft, "Kept");
            assert!(app.state.busy);
        }
    }
}
#[test]
fn cell_pointer_plain_keys_cannot_answer_new_single_question() {
    for code in [egui::Key::Enter, egui::Key::Space, egui::Key::Num1] {
        let ctx = egui::Context::default();
        let mut app = fixture(&ctx);
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let cell = ctx
            .data(|d| {
                d.get_temp::<egui::Rect>(egui::Id::new(("delimited-cell", 2usize, 1usize, false)))
            })
            .unwrap();
        let view = ctx
            .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
            .unwrap();
        let pos = cell.intersect(view).center();
        let mut dialog = crate::app::questions::demo_dialog();
        let mut payload: Value =
            serde_json::from_str(dialog["placeholder"].as_str().unwrap()).unwrap();
        payload["questions"].as_array_mut().unwrap().truncate(1);
        dialog["placeholder"] = Value::String(payload.to_string());
        app.state.dialogs.push_back(dialog);
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("composer")));
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                key(code, egui::Modifiers::NONE),
                egui::Event::Text(" ".into()),
            ],
        );
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
        assert_eq!(app.draft, "Kept");
    }
}
#[test]
fn native_reverse_toolbar_and_grid_escape_keep_busy_and_permission() {
    let ctx = egui::Context::default();
    let mut app = fixture(&ctx);
    let output = frame(&ctx, &mut app, vec![]);
    let pos = super::tests::point(&output, "Filter");
    click(&ctx, &mut app, pos);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    app.state.dialogs.push_back(json!({"id":"reverse-table","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo safe\"}\n\nApprove this call only."}));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let grid = ctx
        .data(|d| d.get_temp::<egui::Id>(egui::Id::new("delimited-grid-focus")))
        .unwrap();
    for _ in 0..120 {
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
        );
        if ctx.memory(|m| m.has_focus(grid)) {
            break;
        }
    }
    assert!(ctx.memory(|m| m.has_focus(grid)));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
    );
    assert!(!ctx.memory(|m| m.has_focus(grid)));
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
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(commands.try_recv().is_err());
    assert!(app.state.busy);
    assert_eq!(app.draft, "Kept");
}
