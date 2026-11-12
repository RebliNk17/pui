use super::*;
use crate::app::files::tests as file_tests;

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        pressed: true,
        repeat: false,
        physical_key: None,
    }
}
fn app() -> Pui {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    let path = app.prefs.cwd.join("grid.csv");
    fs::write(&path, "Name,Note\nAlpha,one\nBeta,שלום世界\n").unwrap();
    app.files
        .open_target(&ctx, &app.prefs.cwd.clone(), path.to_str().unwrap())
        .unwrap();
    let tab = app.files.tabs.last_mut().unwrap();
    tab.content = Some(
        tab.rx
            .take()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap(),
    );
    for _ in 0..4 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    let out = file_tests::frame(&ctx, &mut app, vec![]);
    let pos = file_tests::text(&out, "Navigate cells");
    file_tests::click(&ctx, &mut app, pos);
    app
}
fn active(app: &Pui) -> (usize, usize) {
    let Some(Ok(Content::Table(preview))) = &app.files.tabs[0].content else {
        panic!();
    };
    preview.grid.active
}
#[test]
fn demo_grid_keeps_initial_active_cell_visible_and_keyboard_owned() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_files: true,
            demo_table_grid: true,
            ..Default::default()
        },
    );
    for _ in 0..100 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    let Some(Ok(Content::Table(preview))) = &app.files.tabs[0].content else {
        panic!();
    };
    let id = preview.grid.id(&app.files.tabs[0].path);
    assert_eq!(ctx.memory(|m| m.focused()), Some(id));
    assert_eq!(preview.grid.active, (7, 1));
    let rect = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-active-rect")))
        .unwrap();
    assert!(
        rect.is_positive(),
        "{rect:?}, offset {:?}, viewport {:?}",
        preview.grid.offset,
        preview.grid.viewport
    );
}
#[test]
fn full_app_arrows_copy_and_escape_preserve_draft_busy_messages_and_rpc() {
    let mut app = app();
    let ctx = app.ctx.clone();
    app.state.busy = true;
    app.draft = "Unsent message".into();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let count = app.state.messages.len();
    file_tests::frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::End, egui::Modifiers::COMMAND)],
    );
    assert_eq!(active(&app), (2, 1));
    let out = file_tests::frame(&ctx, &mut app, vec![egui::Event::Copy]);
    assert!(out.platform_output.commands.iter().any(
        |command| matches!(command, egui::OutputCommand::CopyText(text) if text == "שלום世界")
    ));
    file_tests::frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Escape, egui::Modifiers::NONE),
        ],
    );
    assert!(app.state.busy);
    assert_eq!(app.draft, "Unsent message");
    assert_eq!(app.state.messages.len(), count);
    assert!(commands.try_recv().is_err());
    assert_eq!(
        fs::read_to_string(app.prefs.cwd.join("grid.csv")).unwrap(),
        "Name,Note\nAlpha,one\nBeta,שלום世界\n"
    );
}
#[test]
fn arriving_inline_approval_cannot_reinterpret_focused_grid_shortcuts() {
    let mut app = app();
    let ctx = app.ctx.clone();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let permission = json!({"id":"approve-grid", "method":"confirm", "title":"Allow bash?", "message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."});
    assert!(prompts::approval(&permission).is_some());
    app.state.dialogs.push_back(permission);
    for key in [egui::Key::Enter, egui::Key::Backspace, egui::Key::Escape] {
        file_tests::frame(
            &ctx,
            &mut app,
            vec![super::ui_tests::key(
                key,
                if key == egui::Key::Escape {
                    egui::Modifiers::NONE
                } else {
                    egui::Modifiers::COMMAND
                },
            )],
        );
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
    }
    assert_eq!(active(&app), (0, 0));
}
#[test]
fn invisible_source_other_file_and_reloaded_preview_do_not_steal_arrow_or_copy() {
    for mode in 0..5 {
        let mut app = app();
        let ctx = app.ctx.clone();
        match mode {
            0 => app.files.close(),
            1 => app.files.tabs[0].source = true,
            2 => {
                let path = app.prefs.cwd.join("README.md");
                app.files.select(&ctx, path);
            }
            3 => {
                app.terminals
                    .restore_saved_layout(&ctx, &app.prefs.cwd, (true, true));
            }
            _ => {
                let Some(Ok(Content::Table(preview))) = &mut app.files.tabs[0].content else {
                    panic!();
                };
                *preview = Preview::new("changed,content\n".into(), b',');
            }
        }
        let out = file_tests::frame(
            &ctx,
            &mut app,
            vec![
                key(egui::Key::ArrowDown, egui::Modifiers::NONE),
                egui::Event::Copy,
            ],
        );
        assert_eq!(active(&app), (0, 0));
        assert!(
            out.platform_output
                .commands
                .iter()
                .all(|command| !matches!(command, egui::OutputCommand::CopyText(_)))
        );
    }
}
#[test]
fn cell_context_escape_closes_before_global_stop_or_approval_shortcuts() {
    let mut app = app();
    let ctx = app.ctx.clone();
    app.state.busy = true;
    app.draft = "Keep".into();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let pos = ctx
        .data(|data| {
            data.get_temp::<egui::Rect>(egui::Id::new(("delimited-cell", 2_usize, 1_usize, false)))
        })
        .unwrap()
        .center();
    for pressed in [true, false] {
        file_tests::frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(egui::Popup::is_any_open(&ctx));
    file_tests::frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
    );
    assert!(egui::Popup::is_any_open(&ctx));
    file_tests::frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!egui::Popup::is_any_open(&ctx));
    assert!(app.state.busy);
    assert_eq!(app.draft, "Keep");
    assert!(commands.try_recv().is_err());
}
#[test]
fn hidden_or_source_owner_drops_its_cell_menu_without_capturing() {
    for source in [false, true] {
        let mut app = app();
        let ctx = app.ctx.clone();
        let pos = ctx
            .data(|data| {
                data.get_temp::<egui::Rect>(egui::Id::new((
                    "delimited-cell",
                    2_usize,
                    1_usize,
                    false,
                )))
            })
            .unwrap()
            .center();
        for pressed in [true, false] {
            file_tests::frame(
                &ctx,
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(egui::Popup::is_any_open(&ctx));
        if source {
            app.files.tabs[0].source = true;
        } else {
            app.files.close();
        }
        file_tests::frame(&ctx, &mut app, vec![]);
        assert!(!egui::Popup::is_any_open(&ctx));
        assert!(app.draft.is_empty());
    }
}
#[test]
fn pointer_cell_focus_tracks_original_record_and_does_not_silently_quote() {
    let mut app = app();
    let ctx = app.ctx.clone();
    let pos = ctx
        .data(|data| {
            data.get_temp::<egui::Rect>(egui::Id::new(("delimited-cell", 2_usize, 2_usize, false)))
        })
        .unwrap()
        .left_center()
        + egui::vec2(20.0, 0.0);
    file_tests::click(&ctx, &mut app, pos);
    assert_eq!(active(&app), (1, 1));
    let count = app.state.messages.len();
    app.collect_context_capture();
    assert!(app.draft.is_empty());
    assert_eq!(app.state.messages.len(), count);
    file_tests::frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
    );
    assert_eq!(active(&app), (2, 1));
}
