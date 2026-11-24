use super::*;
use crate::app::files::tests::{app, click, frame, text};

fn setup() -> (egui::Context, Pui) {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let root = app.prefs.cwd.clone();
    app.terminals.toggle(&ctx, &root, true);
    app.state.busy = true;
    app.draft = "Keep me".into();
    app.context_chips.clear_quotes();
    app.prefs.hide_inspector = true;
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app)
}
fn open_context(ctx: &egui::Context, app: &mut Pui) {
    let target = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("terminal-context-button")))
        .unwrap()
        .center();
    click(ctx, app, target);
    for _ in 0..3 {
        frame(ctx, app, vec![]);
    }
}

#[test]
fn native_terminal_toolbar_capture_keeps_chat_shell_and_selection_and_releases_snapshot() {
    let (ctx, mut app) = setup();
    let root = app.prefs.cwd.clone();
    let project = app
        .terminals
        .projects
        .get_mut(&root)
        .unwrap()
        .active_mut()
        .unwrap();
    project
        .demo
        .as_mut()
        .unwrap()
        .process(b"\x1b[2J\x1b[HSELECTED literal ; echo never-run\r\nsecond");
    project.selection = Some(((0, 0), (0, 8)));
    frame(&ctx, &mut app, vec![]);
    let messages = app.state.messages.len();
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    open_context(&ctx, &mut app);
    let output = frame(&ctx, &mut app, vec![]);
    text(&output, "Add terminal selection to chat");
    app.terminals
        .projects
        .get_mut(&root)
        .unwrap()
        .active_mut()
        .unwrap()
        .demo
        .as_mut()
        .unwrap()
        .process(b"\x1b[HCHANGED!");
    let target = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("terminal-context-add")))
        .unwrap()
        .center();
    click(&ctx, &mut app, target);
    assert!(
        app.outgoing_text()
            .starts_with("Keep me\n\n[Context: Terminal selection")
    );
    assert!(app.outgoing_text().contains("SELECTED"));
    assert!(!app.outgoing_text().contains("CHANGED"));
    assert!(app.state.busy && app.terminals.is_open());
    assert_eq!(app.state.messages.len(), messages);
    assert_eq!(
        app.terminals.projects[&root].active().unwrap().selection,
        Some(((0, 0), (0, 8)))
    );
    assert!(
        app.terminals.projects[&root]
            .active()
            .unwrap()
            .demo
            .is_some()
    );
    assert!(commands.try_recv().is_err());
    assert!(ctx.data(|data| data.get_temp::<Capture>(key()).is_none()));
    assert!(app.focus_composer || ctx.memory(|memory| memory.has_focus(egui::Id::new("composer"))));
}

#[test]
fn snapshot_cancellation_hide_and_project_switch_release_context_without_retargeting() {
    let (ctx, mut app) = setup();
    open_context(&ctx, &mut app);
    let captured = ctx.data(|data| data.get_temp::<Capture>(key())).unwrap();
    let owner = captured.owner;
    assert!(egui::Popup::is_id_open(&ctx, owner));
    app.terminals.close(&ctx);
    assert!(ctx.data(|data| data.get_temp::<Capture>(key()).is_none()));
    assert!(!egui::Popup::is_id_open(&ctx, owner));
    let root = app.prefs.cwd.clone();
    app.terminals.toggle(&ctx, &root, true);
    frame(&ctx, &mut app, vec![]);
    open_context(&ctx, &mut app);
    app.prefs.cwd = root.join("src");
    frame(&ctx, &mut app, vec![]);
    assert!(ctx.data(|data| data.get_temp::<Capture>(key()).is_none()));
    assert_eq!(app.outgoing_text(), "Keep me");
    assert!(app.state.busy);
    assert_eq!(app.terminals.projects.len(), 2);
}

#[test]
fn capture_menu_keyboard_activation_does_not_answer_pending_permission() {
    let (ctx, mut app) = setup();
    let permission = json!({"id":"terminal-context-permission", "method":"confirm", "title":"Allow bash?", "message":"Read status\n{\"command\":\"echo example\"}\n\nApprove this call only."});
    assert!(prompts::approval(&permission).is_some());
    app.state.dialogs.push_back(permission);
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    open_context(&ctx, &mut app);
    let key = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
    );
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.pending_dialog.is_none());
    assert_eq!(app.outgoing_text(), "Keep me");
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(app.outgoing_text().contains("Terminal visible output"));
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.pending_dialog.is_none());
    assert!(commands.try_recv().is_err());
    assert!(app.state.busy);
}

#[test]
fn early_menu_escape_never_stops_busy_pi_after_terminal_loses_focus() {
    let (ctx, mut app) = setup();
    let (client, _events, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    open_context(&ctx, &mut app);
    assert!(!app.terminals.focused(&ctx));
    let escape = egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    frame(&ctx, &mut app, vec![escape]);
    assert!(app.state.busy);
    assert_eq!(app.outgoing_text(), "Keep me");
    assert!(!egui::Popup::is_any_open(&ctx));
    assert!(commands.try_recv().is_err());
    assert!(ctx.data(|data| data.get_temp::<Capture>(key()).is_none()));
}

#[test]
fn terminal_capture_menu_fits_zoom_themes_and_settles_without_repaint() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            let mut terminal = Terminals::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let root = Path::new("/demo");
            terminal.toggle(&ctx, root, true);
            let mut time = 0.0;
            let mut draw = |events| {
                time += 0.016;
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(390.0, 600.0),
                        )),
                        events,
                        time: Some(time),
                        ..Default::default()
                    },
                    |ctx| {
                        terminal.show(ctx, root, true);
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.label("Chat remains visible");
                        });
                    },
                )
            };
            for _ in 0..3 {
                draw(vec![]);
            }
            let target = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("terminal-context-button")))
                .unwrap()
                .center();
            for pressed in [true, false] {
                draw(vec![
                    egui::Event::PointerMoved(target),
                    egui::Event::PointerButton {
                        pos: target,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
            }
            for _ in 0..100 {
                draw(vec![]);
            }
            let saved = ctx
                .data(|data| data.get_temp::<Capture>(key()))
                .unwrap_or_else(|| panic!("menu disappeared at zoom {zoom}"));
            let popup = ctx.memory(|memory| memory.area_rect(saved.owner)).unwrap();
            assert!(
                ctx.screen_rect().contains_rect(popup),
                "{popup:?} at {zoom}"
            );
            let output = draw(vec![]);
            assert!(
                output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                    > Duration::from_secs(1)
            );
        }
    }
}
