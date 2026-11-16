use super::*;
use crate::app::files::tests::{app, click, frame, settle};
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
fn fixture() -> (egui::Context, Pui) {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let path = app.prefs.cwd.join("modern.csv");
    fs::write(&path, "Name,Note\nAlpha,one\nBeta,two\n").unwrap();
    app.files
        .open_target(&ctx, &app.prefs.cwd.clone(), path.to_str().unwrap())
        .unwrap();
    settle(&mut app.files);
    app.prefs.hide_sidebar = true;
    app.focus_composer = false;
    app.state.busy = true;
    app.draft = "Unsent".into();
    for _ in 0..6 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app)
}
fn control(ctx: &egui::Context, slot: usize) -> egui::Response {
    let id = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("table-toolbar", slot))))
        .unwrap();
    ctx.read_response(id).unwrap()
}
#[test]
fn collapsed_header_is_one_row_data_first_and_file_explorer_is_explicit() {
    let (ctx, mut app) = fixture();
    let out = frame(&ctx, &mut app, vec![]);
    let height = ctx
        .data(|data| data.get_temp::<f32>(egui::Id::new("table-toolbar-height")))
        .unwrap();
    assert!(
        height <= 44.0,
        "single toolbar row including margins: {height}"
    );
    for forbidden in [
        "Navigate cells",
        "First record is header",
        "Copy CSV",
        "Copy TSV",
        "Arrows move",
        "Read-only table ·",
    ] {
        assert!(!out.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text.contains(forbidden))),"persistent chrome leaked {forbidden}");
    }
    assert!(!app.files.tree_open);
    let rect = ctx
        .data(|data| {
            data.get_temp::<egui::Rect>(egui::Id::new(("browser-control", "Toggle file explorer")))
        })
        .unwrap();
    click(&ctx, &mut app, rect.center());
    assert!(app.files.tree_open);
    let out = frame(&ctx, &mut app, vec![]);
    assert!(out.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text.contains("Search files"))));
    let rect = ctx
        .data(|data| {
            data.get_temp::<egui::Rect>(egui::Id::new(("browser-control", "Toggle file explorer")))
        })
        .unwrap();
    click(&ctx, &mut app, rect.center());
    assert!(!app.files.tree_open);
    assert_eq!(app.draft, "Unsent");
}
#[test]
fn copy_options_popup_open_active_dismiss_pointer_chords_and_foreign_owner_are_scoped() {
    for slot in [3usize, 4] {
        for phase in [true, false] {
            let (ctx, mut app) = fixture();
            let (client, _, commands) = rpc::Client::stub_with_records();
            app.client = Some(client);
            app.state.dialogs.push_back(questions::demo_dialog());
            let count = app.state.messages.len();
            let response = control(&ctx, slot);
            let pos = response.rect.center();
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
            for pressed in [true, false] {
                let mut events = vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        pressed,
                        button: egui::PointerButton::Primary,
                        modifiers: egui::Modifiers::NONE,
                    },
                ];
                if pressed == phase {
                    events.extend([
                        key(egui::Key::Enter, egui::Modifiers::COMMAND),
                        key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                        key(egui::Key::Space, egui::Modifiers::NONE),
                        egui::Event::Text(" ".into()),
                    ]);
                }
                frame(&ctx, &mut app, events);
            }
            assert!(egui::Popup::is_id_open(
                &ctx,
                response.id.with("table-menu")
            ));
            frame(
                &ctx,
                &mut app,
                vec![
                    key(egui::Key::Enter, egui::Modifiers::COMMAND),
                    key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                ],
            );
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
            );
            assert!(!egui::Popup::is_any_open(&ctx));
            assert_eq!(app.state.dialogs.len(), 1);
            assert_eq!(app.draft, "Unsent");
            assert_eq!(app.state.messages.len(), count);
            assert!(app.state.busy);
            assert!(commands.try_recv().is_err());
        }
    }
}
#[test]
fn foreign_popup_and_modal_have_first_refusal_over_focused_table_trigger() {
    for modal in [false, true] {
        let (ctx, mut app) = fixture();
        let id = control(&ctx, 3).id;
        ctx.memory_mut(|memory| memory.request_focus(id));
        if modal {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                prompt_ui::modal(ctx, egui::Id::new("foreign-table-modal"), 200.0, |ui| {
                    prompt_ui::heading(ui, "Information", "Foreign owner")
                });
            });
        } else {
            egui::Popup::open_id(&ctx, egui::Id::new("foreign-table-popup"));
        }
        let out = frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert!(!egui::Popup::is_id_open(&ctx, id.with("table-menu")));
        assert!(out.platform_output.commands.is_empty());
        assert_eq!(app.draft, "Unsent");
        assert!(app.state.busy);
    }
}

#[test]
fn sort_column_dropdown_is_named_bounded_explicit_and_dismissal_never_stops_run() {
    let (ctx, mut app) = fixture();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let pos = control(&ctx, 2).rect.center();
    click(&ctx, &mut app, pos);
    frame(&ctx, &mut app, vec![]);
    // Use the actual rendered column label, not guessed row coordinates.
    let out = frame(&ctx, &mut app, vec![]);
    let pos = crate::app::files::tests::text(&out, "Name");
    click(&ctx, &mut app, pos);
    assert!(egui::Popup::is_any_open(&ctx));
    app.state.dialogs.push_back(questions::demo_dialog());
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
        ],
    );
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Escape, egui::Modifiers::NONE),
            egui::Event::Text("must not answer".into()),
            egui::Event::Paste("or paste".into()),
        ],
    );
    assert!(!egui::Popup::is_any_open(&ctx));
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
    assert_eq!(app.draft, "Unsent");
}

#[test]
fn source_tab_hides_grid_menu_and_hidden_tab_closes_own_popup_without_replay() {
    let (ctx, mut app) = fixture();
    let copy = control(&ctx, 3);
    click(&ctx, &mut app, copy.rect.center());
    assert!(egui::Popup::is_any_open(&ctx));
    app.files.close();
    frame(&ctx, &mut app, vec![]);
    assert!(!egui::Popup::is_any_open(&ctx));
    app.files.open = true;
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    click(&ctx, &mut app, control(&ctx, 1).rect.center());
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(app.files.tabs[0].source);
    let out = frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::ArrowDown, egui::Modifiers::NONE),
            egui::Event::Copy,
        ],
    );
    assert!(out.platform_output.commands.is_empty());
    assert!(!control(&ctx, 2).enabled());
}
