use super::ui_tests::{control, fixture, key, open_copy, pointer};
use super::*;
use crate::app::files::tests::frame;
#[test]
fn real_pointer_record_actions_own_press_release_chords_and_new_question_without_sending() {
    for phase in [true, false] {
        let (ctx, mut app) = fixture();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.state.dialogs.push_back(questions::demo_dialog());
        let pos = ctx.read_response(control(&ctx, 2)).unwrap().rect.center();
        let mut copied = 0;
        for pressed in [true, false] {
            let mut events = pointer(pos, pressed);
            if phase == pressed {
                events.extend([
                    key(egui::Key::Enter, egui::Modifiers::COMMAND),
                    key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                    key(egui::Key::Space, egui::Modifiers::NONE),
                    egui::Event::Text(" ".into()),
                    egui::Event::Paste("no leak".into()),
                ]);
            }
            let output = frame(&ctx, &mut app, events);
            copied+=output.platform_output.commands.iter().filter(|command|matches!(command,egui::OutputCommand::CopyText(text) if text=="Name,Note\n")).count();
        }
        assert_eq!(copied, 1);
        assert_eq!(app.draft, "Kept draft");
        assert!(app.state.busy);
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
    }
}
#[test]
fn first_native_tab_to_record_then_space_enter_and_reverse_escape_keep_exact_owner() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app) = fixture();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let record = control(&ctx, 2);
        let tsv = control(&ctx, 3);
        let preceding = control(&ctx, 1);
        ctx.memory_mut(|memory| memory.request_focus(preceding));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
        );
        assert_eq!(ctx.memory(|memory| memory.focused()), Some(record));
        app.state.dialogs.push_back(questions::demo_dialog());
        let mut events = vec![key(activation, egui::Modifiers::NONE)];
        if activation == egui::Key::Space {
            events.push(egui::Event::Text(" ".into()));
        }
        let output = frame(&ctx, &mut app, events);
        assert!(output.platform_output.commands.iter().any(
            |command| matches!(command,egui::OutputCommand::CopyText(text) if text=="Name,Note\n")
        ));
        open_copy(&ctx, &mut app);
        ctx.memory_mut(|memory| memory.request_focus(tsv));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)],
        );
        frame(
            &ctx,
            &mut app,
            vec![
                key(egui::Key::Escape, egui::Modifiers::NONE),
                egui::Event::Text(" ".into()),
                egui::Event::Paste("ignored".into()),
            ],
        );
        assert!(!egui::Popup::is_any_open(&ctx));
        assert_eq!(app.draft, "Kept draft");
        assert!(app.state.busy);
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
    }
}
#[test]
fn source_empty_invalid_projection_replacement_and_foreign_modal_refuse_record_export() {
    let (ctx, mut app) = fixture();
    let old = control(&ctx, 2);
    let Some(Ok(Content::Table(preview))) = &mut app.files.tabs[0].content else {
        panic!()
    };
    preview.set_header(false);
    projection::tests::apply(&ctx, preview, "no-match", projection::Order::Original);
    let path = app.files.tabs[0].path.clone();
    app.prefs.table_headers.set(&app.prefs.cwd, &path, false);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    open_copy(&ctx, &mut app);
    assert!(!ctx.read_response(control(&ctx, 2)).unwrap().enabled());
    assert!(!ctx.read_response(control(&ctx, 3)).unwrap().enabled());
    app.files.tabs[0].source = true;
    frame(&ctx, &mut app, vec![]);
    open_copy(&ctx, &mut app);
    assert!(!ctx.read_response(control(&ctx, 2)).unwrap().enabled());
    app.files.tabs[0].content = Some(Ok(Content::Table(Preview::new(
        "Other,private\n".into(),
        b',',
    ))));
    egui::Popup::close_all(&ctx);
    ctx.memory_mut(|memory| memory.request_focus(old));
    let output = frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(
        !output
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command, egui::OutputCommand::CopyText(_)))
    );
    app.files.tabs[0].source = false;
    open_copy(&ctx, &mut app);
    let current = control(&ctx, 2);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        prompt_ui::modal(
            ctx,
            egui::Id::new("record-copy-foreign-modal"),
            220.0,
            |ui| prompt_ui::heading(ui, "Information", "Foreign"),
        );
    });
    ctx.memory_mut(|memory| memory.request_focus(current));
    let output = frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(
        !output
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command, egui::OutputCommand::CopyText(_)))
    );
    assert_eq!(app.draft, "Kept draft");
}
#[test]
fn oversized_record_action_preserves_clipboard_and_does_not_allocate_retained_output() {
    let (ctx, mut app) = fixture();
    egui::Popup::close_all(&ctx);
    app.files.tabs[0].content = Some(Ok(Content::Table(Preview::new(
        format!("{}\n", "\t".repeat(MAX_OUTPUT - 2)),
        b',',
    ))));
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    open_copy(&ctx, &mut app);
    let id = control(&ctx, 3);
    ctx.memory_mut(|memory| memory.request_focus(id));
    let output = frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(
        !output
            .platform_output
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
    assert!(preview.export.notice.as_ref().unwrap().capacity() < 160);
    assert_eq!(app.draft, "Kept draft");
    assert!(app.state.busy);
}

#[test]
fn record_menu_narrow_light_dark_zoom_remains_contextual_and_adds_no_toolbar_height() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app) = fixture();
            app.prefs.theme = mode;
            ctx.set_zoom_factor(zoom);
            egui::Popup::close_all(&ctx);
            let draw = |app: &mut Pui, events| {
                let mut native = eframe::Frame::_new_kittest();
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 780.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(app, ctx, &mut native),
                )
            };
            for _ in 0..4 {
                draw(&mut app, vec![]);
            }
            let trigger = ctx
                .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("table-toolbar", 3usize))))
                .unwrap();
            let pos = ctx.read_response(trigger).unwrap().rect.center();
            for pressed in [true, false] {
                draw(&mut app, pointer(pos, pressed));
            }
            for _ in 0..3 {
                draw(&mut app, vec![]);
            }
            for slot in [2, 3] {
                let response = ctx.read_response(control(&ctx, slot)).unwrap();
                assert!(response.enabled());
                assert!(response.rect.width() <= 260.0);
                assert!(ctx.screen_rect().contains_rect(response.rect));
            }
            draw(
                &mut app,
                vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
            );
            let output = draw(&mut app, vec![]);
            assert!(!output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="Copy record as CSV" || text.galley.job.text=="Copy record as TSV")));
            let height = ctx
                .data(|data| data.get_temp::<f32>(egui::Id::new("table-toolbar-height")))
                .unwrap();
            assert!(
                height <= 80.0,
                "narrow toolbar can wrap at high zoom: {height}"
            );
        }
    }
}
