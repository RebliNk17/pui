use super::*;
use crate::app::files::tests::{app, frame, settle};
pub(super) fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
pub(super) fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
pub(super) fn fixture() -> (egui::Context, Pui) {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let path = app.prefs.cwd.join("clipboard.csv");
    fs::write(&path, "Name,Note\nBeta,=1+2\nAlpha,שלום\n").unwrap();
    app.files
        .open_target(&ctx, &app.prefs.cwd.clone(), path.to_str().unwrap())
        .unwrap();
    settle(&mut app.files);
    app.prefs.hide_sidebar = true;
    app.focus_composer = false;
    app.draft = "Kept draft".into();
    app.state.busy = true;
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    open_copy(&ctx, &mut app);
    (ctx, app)
}
pub(super) fn open_copy(ctx: &egui::Context, app: &mut Pui) {
    egui::Popup::close_all(ctx);
    frame(ctx, app, vec![]);
    let id = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("table-toolbar", 3usize))))
        .unwrap();
    let pos = ctx.read_response(id).unwrap().rect.center();
    for pressed in [true, false] {
        frame(ctx, app, pointer(pos, pressed));
    }
    frame(ctx, app, vec![]);
}
pub(super) fn control(ctx: &egui::Context, slot: usize) -> egui::Id {
    ctx.data(|data| data.get_temp::<egui::Id>(egui::Id::new(("table-export", slot))))
        .unwrap()
}
#[test]
fn table_export_pointer_phases_chords_and_question_activation_preserve_draft_run_permission() {
    for phase in [true, false] {
        let (ctx, mut app) = fixture();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let mut question = questions::demo_dialog();
        let mut value: Value =
            serde_json::from_str(question["placeholder"].as_str().unwrap()).unwrap();
        value["questions"] = json!([value["questions"][0].clone()]);
        question["placeholder"] = json!(value.to_string());
        app.state.dialogs.push_back(question);
        let pos = ctx
            .read_response(control(&ctx, 0))
            .unwrap()
            .interact_rect
            .center();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
        let mut copied = 0;
        for pressed in [true, false] {
            let mut events = pointer(pos, pressed);
            if pressed == phase {
                events.extend([
                    key(egui::Key::Enter, egui::Modifiers::COMMAND),
                    key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                    key(egui::Key::Space, egui::Modifiers::NONE),
                    egui::Event::Text(" ".into()),
                ]);
            }
            let out = frame(&ctx, &mut app, events);
            copied+=out.platform_output.commands.iter().filter(|command|matches!(command,egui::OutputCommand::CopyText(text) if text.starts_with("Name,Note\n"))).count();
        }
        assert_eq!(copied, 1);
        assert_eq!(app.draft, "Kept draft");
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(app.pending_dialog.is_none());
        assert!(app.state.busy);
        assert!(commands.try_recv().is_err());
    }
}
#[test]
fn table_export_native_enter_space_tab_reverse_escape_and_foreign_popup_are_scoped() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app) = fixture();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.state.dialogs.push_back(json!({"id":"copy-permission","method":"confirm","title":"Allow bash?","message":"controlled"}));
        let csv = control(&ctx, 0);
        let tsv = control(&ctx, 1);
        ctx.memory_mut(|memory| memory.request_focus(csv));
        let mut events = vec![key(activation, egui::Modifiers::NONE)];
        if activation == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        let out = frame(&ctx, &mut app, events);
        assert!(out.platform_output.commands.iter().any(|command|matches!(command,egui::OutputCommand::CopyText(text) if text.starts_with("Name,Note\n"))));
        open_copy(&ctx, &mut app);
        ctx.memory_mut(|memory| memory.request_focus(csv));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
        );
        assert_eq!(ctx.memory(|memory| memory.focused()), Some(tsv));
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
        assert!(app.state.busy);
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
        assert_eq!(app.draft, "Kept draft");
        egui::Popup::open_id(&ctx, egui::Id::new("foreign-export-popup"));
        ctx.memory_mut(|memory| memory.request_focus(csv));
        let out = frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert!(
            !out.platform_output
                .commands
                .iter()
                .any(|command| matches!(command, egui::OutputCommand::CopyText(_)))
        );
    }
}
#[test]
fn export_focused_modified_activation_is_never_copy_or_underlying_permission() {
    for chord in [egui::Key::Enter, egui::Key::Backspace] {
        let (ctx, mut app) = fixture();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.state.dialogs.push_back(json!({"id":"export-modified","method":"confirm","title":"Allow bash?","message":"controlled"}));
        let id = control(&ctx, 0);
        ctx.memory_mut(|memory| memory.request_focus(id));
        let out = frame(&ctx, &mut app, vec![key(chord, egui::Modifiers::COMMAND)]);
        assert!(
            !out.platform_output
                .commands
                .iter()
                .any(|command| matches!(command, egui::OutputCommand::CopyText(_)))
        );
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
        assert_eq!(app.draft, "Kept draft");
        assert!(app.state.busy);
    }
}

#[test]
fn header_only_empty_projection_hidden_source_and_foreign_modal_do_not_copy() {
    let (ctx, mut app) = fixture();
    let Some(Ok(Content::Table(preview))) = &mut app.files.tabs[0].content else {
        panic!()
    };
    let context = ctx.clone();
    crate::app::files::delimited::projection::tests::apply(
        &context,
        preview,
        "no match",
        projection::Order::Original,
    );
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    open_copy(&ctx, &mut app);
    let csv = control(&ctx, 0);
    ctx.memory_mut(|memory| memory.request_focus(csv));
    let out = frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(out.platform_output.commands.iter().any(
        |command| matches!(command,egui::OutputCommand::CopyText(text) if text=="Name,Note\n")
    ));
    let Some(Ok(Content::Table(preview))) = &mut app.files.tabs[0].content else {
        panic!()
    };
    preview.set_header(false);
    let path = app.files.tabs[0].path.clone();
    app.prefs.table_headers.set(&app.prefs.cwd, &path, false);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    open_copy(&ctx, &mut app);
    let response = ctx.read_response(control(&ctx, 0)).unwrap();
    assert!(!response.enabled());
    app.files.tabs[0].source = true;
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
            ..Default::default()
        },
        |ctx| {
            let before = ctx.input(|input| input.events.clone());
            app.files.context_inputs(ctx, true, true);
            // Source now shares the live Copy/options toolbar; its open menu
            // still owns modified activation, but disabled CSV cannot copy.
            assert!(!before.is_empty());
            assert!(ctx.input(|input| input.events.is_empty()));
            assert!(app.files.table_export_owns_keyboard(ctx));
        },
    );
    app.files.tabs[0].source = false;
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
            ..Default::default()
        },
        |ctx| {
            prompt_ui::modal(ctx, egui::Id::new("export-foreign-modal"), 220.0, |ui| {
                prompt_ui::heading(ui, "Information", "Foreign")
            });
        },
    );
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
            ..Default::default()
        },
        |ctx| {
            let before = ctx.input(|input| input.events.clone());
            app.files.context_inputs(ctx, true, true);
            assert_eq!(ctx.input(|input| input.events.clone()), before);
        },
    );
}

#[test]
fn newly_arrived_question_text_first_tab_and_disabled_controls_keep_exact_owner() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app) = fixture();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let csv = control(&ctx, 0);
        let tsv = control(&ctx, 1);
        // Buttons need not retain pointer focus on macOS. Acquire through native
        // registration/traversal instead of assuming a pointer grants focus.
        for _ in 0..120 {
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
            );
            if ctx.memory(|memory| memory.has_focus(csv)) {
                break;
            }
        }
        assert_eq!(ctx.memory(|memory| memory.focused()), Some(csv));
        app.state.dialogs.push_back(questions::demo_dialog());
        let mut events = vec![
            key(activation, egui::Modifiers::NONE),
            egui::Event::Paste("not an answer".into()),
        ];
        if activation == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        let out = frame(&ctx, &mut app, events);
        assert!(out.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text.starts_with("Name,Note\n"))));
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(app.pending_dialog.is_none());
        assert!(commands.try_recv().is_err());
        assert_eq!(app.draft, "Kept draft");
        open_copy(&ctx, &mut app);
        ctx.memory_mut(|memory| memory.request_focus(csv));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
        );
        assert_eq!(ctx.memory(|memory| memory.focused()), Some(tsv));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(app.state.busy);
    }
    let (ctx, mut preview) =
        crate::app::files::delimited::grid::tests::setup("Name,Note\nA,B\n".into(), true);
    let csv = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("table-toolbar", 3usize))))
        .unwrap();
    ctx.memory_mut(|memory| memory.request_focus(csv));
    for _ in 0..2 {
        let output = ctx.run(
            egui::RawInput {
                events: vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_enabled_ui(false, |ui| preview.show(ui, Path::new("keyboard.csv")));
                });
            },
        );
        assert!(output.platform_output.commands.is_empty());
    }
    let disabled = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("table-toolbar", 3usize))))
        .unwrap();
    assert!(!ctx.read_response(disabled).unwrap().enabled());
}

#[test]
fn table_export_narrow_themes_zoom_and_oversize_leave_clipboard_unchanged() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app) = fixture();
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            egui::Popup::close_all(&ctx);
            let mut native = eframe::Frame::_new_kittest();
            for _ in 0..5 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 780.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut native),
                );
            }
            for slot in 0..5usize {
                let id = ctx
                    .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("table-toolbar", slot))))
                    .unwrap();
                let response = ctx.read_response(id).unwrap();
                assert!(
                    response.rect.left() >= 0.0
                        && response.rect.right() <= ctx.screen_rect().right()
                );
            }
        }
    }
    let (ctx, mut app) = fixture();
    let Some(Ok(Content::Table(preview))) = &mut app.files.tabs[0].content else {
        panic!()
    };
    *preview = Preview::new(format!("{}\n", "\t".repeat(MAX_OUTPUT - 2)), b',');
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    open_copy(&ctx, &mut app);
    let tsv = control(&ctx, 1);
    ctx.memory_mut(|memory| memory.request_focus(tsv));
    let out = frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(
        !out.platform_output
            .commands
            .iter()
            .any(|command| matches!(command, egui::OutputCommand::CopyText(_)))
    );
    let Some(Ok(Content::Table(preview))) = &app.files.tabs[0].content else {
        panic!()
    };
    assert!(
        preview
            .export
            .notice
            .as_ref()
            .unwrap()
            .contains("nothing copied")
    );
    assert_eq!(app.draft, "Kept draft");
}
