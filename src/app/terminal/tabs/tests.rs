use super::*;
use crate::app::files::tests::{app, click, frame, text};
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
fn control(ctx: &egui::Context, name: &str) -> (egui::Id, egui::Rect, bool) {
    ctx.data(|data| data.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new(name)))
        .unwrap()
}
fn tab_control(ctx: &egui::Context, index: usize) -> (egui::Id, egui::Rect, u64) {
    ctx.data(|data| {
        data.get_temp::<(egui::Id, egui::Rect, u64)>(egui::Id::new(("terminal-tab", index)))
    })
    .unwrap()
}
fn setup() -> (egui::Context, Pui) {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let root = app.prefs.cwd.clone();
    app.terminals.toggle(&ctx, &root, true);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app)
}

#[test]
fn independent_tabs_survive_hide_and_project_change_with_unique_input_owners() {
    let (ctx, mut app) = setup();
    let root = app.prefs.cwd.clone();
    let first = app.terminals.projects[&root].active().unwrap().id;
    app.terminals
        .projects
        .get_mut(&root)
        .unwrap()
        .active_mut()
        .unwrap()
        .demo
        .as_mut()
        .unwrap()
        .process(b"\r\nFIRST-MARKER");
    click(&ctx, &mut app, control(&ctx, "terminal-tab-add").1.center());
    assert_eq!(app.terminals.projects[&root].tabs.len(), 2);
    let second = app.terminals.projects[&root].active().unwrap().id;
    assert_ne!(first, second);
    app.terminals
        .projects
        .get_mut(&root)
        .unwrap()
        .active_mut()
        .unwrap()
        .demo
        .as_mut()
        .unwrap()
        .process(b"\r\nSECOND-MARKER");
    click(&ctx, &mut app, tab_control(&ctx, 0).1.center());
    assert_eq!(app.terminals.projects[&root].active().unwrap().id, first);
    assert!(
        app.terminals.projects[&root]
            .active()
            .unwrap()
            .demo
            .as_ref()
            .unwrap()
            .screen()
            .contents()
            .contains("FIRST-MARKER")
    );
    app.terminals.close(&ctx);
    assert_eq!(app.terminals.total_tabs(), 2);
    app.terminals.toggle(&ctx, &root, true);
    frame(&ctx, &mut app, vec![]);
    app.prefs.cwd = root.join("src");
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.terminals.total_tabs(), 3);
    app.prefs.cwd = root.clone();
    frame(&ctx, &mut app, vec![]);
    click(&ctx, &mut app, tab_control(&ctx, 1).1.center());
    assert_eq!(app.terminals.projects[&root].active().unwrap().id, second);
    assert!(
        app.terminals.projects[&root]
            .active()
            .unwrap()
            .demo
            .as_ref()
            .unwrap()
            .screen()
            .contents()
            .contains("SECOND-MARKER")
    );
}

#[test]
fn ctrl_tab_is_early_focus_scoped_ordered_and_approval_safe() {
    let (ctx, mut app) = setup();
    let root = app.prefs.cwd.clone();
    for _ in 0..2 {
        app.terminals.start(&ctx, true);
    }
    frame(&ctx, &mut app, vec![]);
    app.state.busy = true;
    app.draft = "keep draft".into();
    app.state.dialogs.push_back(json!({"id":"terminal-tab-approval","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo safe\"}\n\nApprove this call only."}));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Tab, egui::Modifiers::CTRL),
            key(egui::Key::Tab, egui::Modifiers::CTRL),
        ],
    );
    assert_eq!(app.terminals.projects[&root].selected, 1);
    let (id, _, _) = tab_control(&ctx, 1);
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
            key(egui::Key::Escape, egui::Modifiers::NONE),
        ],
    );
    assert!(app.state.busy);
    assert_eq!(app.draft, "keep draft");
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(commands.try_recv().is_err());
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::CTRL)],
    );
    assert_eq!(app.terminals.projects[&root].selected, 1);
    app.settings_open = true;
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::CTRL)],
    );
    assert_eq!(app.terminals.projects[&root].selected, 1);
}

#[test]
fn tab_caps_and_closing_final_tab_never_implicitly_respawn() {
    let ctx = egui::Context::default();
    let mut terminal = Terminals::default();
    let root = Path::new("/demo");
    terminal.toggle(&ctx, root, true);
    for _ in 0..MAX_TABS + 3 {
        terminal.start(&ctx, true);
    }
    assert_eq!(terminal.total_tabs(), MAX_TABS);
    for index in 1..5 {
        terminal.workspace(&ctx, &PathBuf::from(format!("/demo-{index}")), true);
        for _ in 0..MAX_TABS {
            terminal.start(&ctx, true);
        }
    }
    assert_eq!(terminal.total_tabs(), MAX_TOTAL_TABS);
    terminal.root = root.into();
    let ids = terminal.projects[root]
        .tabs
        .iter()
        .map(|tab| tab.id)
        .collect::<Vec<_>>();
    for id in ids {
        terminal.close_tab(&ctx, root, id);
    }
    terminal.workspace(&ctx, root, true);
    assert!(terminal.projects[root].tabs.is_empty());
    assert_eq!(terminal.total_tabs(), MAX_TOTAL_TABS - MAX_TABS);
    terminal.start(&ctx, true);
    assert_eq!(terminal.projects[root].tabs.len(), 1);
    assert!(terminal.projects[root].tabs[0].label > MAX_TABS as u64);
}

#[test]
fn running_close_confirmation_is_bound_to_exact_tab_and_cancel_preserves_all_shells() {
    let (ctx, mut app) = setup();
    let root = app.prefs.cwd.clone();
    let (tx, rx) = mpsc::channel();
    app.terminals
        .projects
        .get_mut(&root)
        .unwrap()
        .active_mut()
        .unwrap()
        .starting = Some(rx);
    frame(&ctx, &mut app, vec![]);
    let id = app.terminals.projects[&root].active().unwrap().id;
    let close = ctx
        .data(|data| data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("terminal-tab-close")))
        .unwrap()
        .1;
    click(&ctx, &mut app, close.center());
    assert_eq!(app.terminals.confirm_stop, Some((root.clone(), id)));
    let out = frame(&ctx, &mut app, vec![]);
    let cancel = text(&out, "Keep running");
    click(&ctx, &mut app, cancel);
    assert!(app.terminals.confirm_stop.is_none());
    assert_eq!(app.terminals.total_tabs(), 1);
    drop(tx);
    frame(&ctx, &mut app, vec![]);
    assert!(
        app.terminals.projects[&root]
            .active()
            .unwrap()
            .starting
            .is_none()
    );
    app.terminals.start(&ctx, true);
    let survivor = app.terminals.projects[&root].active().unwrap().id;
    app.terminals.confirm_stop = Some((root.clone(), id));
    frame(&ctx, &mut app, vec![]);
    let out = frame(&ctx, &mut app, vec![]);
    click(&ctx, &mut app, text(&out, "Stop terminal"));
    assert_eq!(app.terminals.total_tabs(), 1);
    assert_eq!(app.terminals.projects[&root].active().unwrap().id, survivor);
}

#[test]
fn narrow_themes_zoom_left_rail_controls_fit_and_settle() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut terminal = Terminals::default();
            let root = Path::new("/demo/long/project/directory");
            terminal.toggle(&ctx, root, true);
            for _ in 1..MAX_TABS {
                terminal.start(&ctx, true);
            }
            let mut time = 0.0;
            let mut out = egui::FullOutput::default();
            for _ in 0..45 {
                time += 0.1;
                out = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(390.0, 650.0),
                        )),
                        time: Some(time),
                        ..Default::default()
                    },
                    |ctx| {
                        terminal.context_inputs(ctx, true);
                        terminal.show(ctx, root, true);
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.label("Chat remains visible");
                        });
                    },
                );
            }
            let (_, plus, enabled) = control(&ctx, "terminal-tab-add");
            assert!(!enabled);
            assert!(ctx.screen_rect().contains_rect(plus));
            let (_, close) = ctx
                .data(|data| {
                    data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("terminal-tab-close"))
                })
                .unwrap();
            assert!(ctx.screen_rect().contains_rect(close));
            let rail = ctx
                .data(|data| {
                    data.get_temp::<egui::Rect>(egui::Id::new("terminal-shell-rail-bounds"))
                })
                .unwrap();
            let output = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("terminal-output-bounds")))
                .unwrap();
            assert!(
                rail.right() < output.left(),
                "rail must stay left of PTY: {rail:?} {output:?}"
            );
            assert!(rail.width() <= 129.0 && output.width() >= 150.0);
            assert!(plus.left() >= rail.left() && plus.right() <= rail.right());
            assert!(close.left() >= rail.left() && close.right() <= rail.right());
            let first = tab_control(&ctx, 0).1;
            let second = tab_control(&ctx, 1).1;
            assert!(
                (first.left() - second.left()).abs() < 1.0 && second.top() >= first.bottom(),
                "shell rows must be vertical"
            );
            assert!(first.height() <= 28.0 && second.height() <= 28.0);
            for index in 0..MAX_TABS {
                let (_, row, id) = tab_control(&ctx, index);
                let (icon, pos, elided) = ctx
                    .data(|data| {
                        data.get_temp::<(egui::Rect, egui::Pos2, bool)>(egui::Id::new((
                            "terminal-shell-icon",
                            id,
                        )))
                    })
                    .unwrap();
                assert!(row.contains_rect(icon) && pos.x > icon.right());
                assert!(!elided, "shell label must fit icon rail");
            }
            let heading = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("terminal-heading-bounds")))
                .unwrap();
            assert!(
                ctx.screen_rect().contains_rect(heading),
                "header must not grow dock width"
            );
            assert!(
                out.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(1)
            );
        }
    }
}
