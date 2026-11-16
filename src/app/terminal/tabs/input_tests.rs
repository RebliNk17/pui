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
fn app_with_tabs(ctx: &egui::Context) -> Pui {
    let mut app = app(ctx);
    let root = app.prefs.cwd.clone();
    app.terminals.toggle(ctx, &root, true);
    app.terminals.start(ctx, true);
    app.terminals.start(ctx, true);
    for _ in 0..3 {
        frame(ctx, &mut app, vec![]);
    }
    app.state.busy = true;
    app.draft = "keep".into();
    app.state.dialogs.push_back(json!({"id":"tabs-input-approval","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo safe\"}\n\nApprove this call only."}));
    app
}

#[test]
fn consecutive_frame_switches_keep_exact_input_focus_and_escape_never_answers_approval() {
    let ctx = egui::Context::default();
    let mut app = app_with_tabs(&ctx);
    let root = app.prefs.cwd.clone();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    for selected in [0, 1, 2, 0] {
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::CTRL)],
        );
        assert_eq!(app.terminals.projects[&root].selected, selected);
        let id = app.terminals.projects[&root].active().unwrap().input_id();
        assert!(
            ctx.memory(|memory| memory.has_focus(id)),
            "focus escaped selected {selected}"
        );
    }
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Escape, egui::Modifiers::NONE),
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
        ],
    );
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.state.busy && commands.try_recv().is_err());
    assert_eq!(app.draft, "keep");
}

#[test]
fn capture_escape_returns_to_owned_header_before_next_frame_approval_keys() {
    let ctx = egui::Context::default();
    let mut app = app_with_tabs(&ctx);
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let rect = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("terminal-context-button")))
        .unwrap();
    click(&ctx, &mut app, rect.center());
    frame(&ctx, &mut app, vec![]);
    assert!(egui::Popup::is_any_open(&ctx));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!egui::Popup::is_any_open(&ctx));
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
            key(egui::Key::Escape, egui::Modifiers::NONE),
        ],
    );
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.state.busy && commands.try_recv().is_err());
    assert_eq!(app.draft, "keep");
}

#[test]
fn restored_control_shortcuts_are_owned_even_without_any_project_or_pty() {
    for index in 0..3 {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let root = app.prefs.cwd.clone();
        app.terminals
            .restore_saved_layout(&ctx, &root, (true, true));
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        assert!(app.terminals.projects.is_empty());
        let id = app.terminals.controls[index];
        ctx.memory_mut(|memory| {
            memory.request_focus_with_filter(
                id,
                egui::EventFilter {
                    escape: true,
                    ..Default::default()
                },
            )
        });
        app.state.busy = true;
        app.draft = "keep".into();
        app.state.dialogs.push_back(json!({"id":"restore-tab-approval","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo safe\"}\n\nApprove this call only."}));
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        frame(
            &ctx,
            &mut app,
            vec![
                key(egui::Key::Enter, egui::Modifiers::COMMAND),
                key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                key(egui::Key::Escape, egui::Modifiers::NONE),
            ],
        );
        assert!(app.terminals.projects.is_empty());
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(app.state.busy && commands.try_recv().is_err());
        assert_eq!(app.draft, "keep");
    }
}

#[test]
fn activation_space_counterpart_is_consumed_but_unrelated_text_and_paste_remain() {
    let ctx = egui::Context::default();
    let out = ctx.run(
        egui::RawInput {
            events: vec![
                key(egui::Key::Space, egui::Modifiers::NONE),
                egui::Event::Text(" ".into()),
                egui::Event::Text("other".into()),
                egui::Event::Paste("paste".into()),
            ],
            ..Default::default()
        },
        |ctx| {
            consume_activation(ctx);
            let events = ctx.input(|input| input.events.clone());
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event,egui::Event::Text(text)if text==" "))
            );
            assert!(
                events
                    .iter()
                    .any(|event| matches!(event,egui::Event::Text(text)if text=="other"))
            );
            assert!(
                events
                    .iter()
                    .any(|event| matches!(event,egui::Event::Paste(text)if text=="paste"))
            );
        },
    );
    let _ = out;
    let mut app = app_with_tabs(&ctx);
    let (id, _, _) = ctx
        .data(|data| {
            data.get_temp::<(egui::Id, egui::Rect, u64)>(egui::Id::new(("terminal-tab", 0)))
        })
        .unwrap();
    ctx.memory_mut(|memory| {
        memory.request_focus_with_filter(
            id,
            egui::EventFilter {
                escape: true,
                ..Default::default()
            },
        )
    });
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Space, egui::Modifiers::NONE),
            egui::Event::Text(" ".into()),
        ],
    );
    assert_eq!(app.terminals.projects[&app.prefs.cwd].selected, 0);
}
