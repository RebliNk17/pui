use super::ui_tests::{control, fixture, key, open_copy, pointer};
use super::*;
use crate::app::files::tests::frame;
pub(super) fn selected() -> (egui::Context, Pui) {
    let (ctx, mut app) = fixture();
    egui::Popup::close_all(&ctx);
    let path = app.files.tabs[0].path.clone();
    let Some(Ok(Content::Table(preview))) = &mut app.files.tabs[0].content else {
        panic!()
    };
    preview.grid.focus(&ctx, &path);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
    );
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::ArrowDown, egui::Modifiers::SHIFT),
            key(egui::Key::ArrowRight, egui::Modifiers::SHIFT),
        ],
    );
    open_copy(&ctx, &mut app);
    (ctx, app)
}
#[test]
fn selection_csv_tsv_exact_escaping_ragged_and_bounds() {
    let (ctx, preview) = grid::tests::setup(
        "a,b\n\"comma,\",\"quote\"\"世界\nline\"\nshort\n".into(),
        false,
    );
    let index = preview.index.as_ref().unwrap();
    let rows = preview.projection.rows(index.records.len(), false);
    for delimiter in [b',', b'\t'] {
        let output =
            rectangle::serialize_as(&preview.source, index, rows, (1, 0), (2, 1), delimiter)
                .unwrap();
        let parsed = parse(&output, delimiter).unwrap();
        assert_eq!(parsed.records.len(), 2);
        assert_eq!(parsed.value(&output, 0, 0), "comma,");
        assert_eq!(parsed.value(&output, 0, 1), "quote\"世界\nline");
        assert_eq!(parsed.value(&output, 1, 1), "");
    }
    assert!(rectangle::serialize_as(&preview.source, index, rows, (0, 0), (1, 1), b';').is_err());
    let _ = ctx;
}
#[test]
fn explicit_selection_menu_pointer_and_native_tab_copy_only_rectangle_without_send() {
    for slot in [6, 7] {
        let (ctx, mut app) = selected();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        let id = control(&ctx, slot);
        assert!(ctx.read_response(id).unwrap().enabled());
        app.state.dialogs.push_back(questions::demo_dialog());
        let pos = ctx.read_response(id).unwrap().interact_rect.center();
        let mut copied = 0;
        for pressed in [true, false] {
            let mut events = pointer(pos, pressed);
            events.extend([
                key(egui::Key::Enter, egui::Modifiers::COMMAND),
                key(egui::Key::Backspace, egui::Modifiers::COMMAND),
            ]);
            let output = frame(&ctx, &mut app, events);
            copied+=output.platform_output.commands.iter().filter(|command|matches!(command,egui::OutputCommand::CopyText(text) if text==if slot==6{"Beta,=1+2\nAlpha,שלום\n"}else{"Beta\t=1+2\nAlpha\tשלום\n"})).count();
        }
        assert_eq!(copied, 1);
        assert_eq!(app.draft, "Kept draft");
        assert!(commands.try_recv().is_err());
        assert_eq!(app.state.dialogs.len(), 1);
        open_copy(&ctx, &mut app);
        let before = control(&ctx, 5);
        ctx.memory_mut(|memory| memory.request_focus(before));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
        );
        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            Some(control(&ctx, 6))
        );
    }
}
#[test]
fn selection_enter_space_escape_foreign_modal_and_maximum_output_are_bounded() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app) = selected();
        let id = control(&ctx, 6);
        ctx.memory_mut(|memory| memory.request_focus(id));
        let output = frame(&ctx, &mut app, vec![key(activation, egui::Modifiers::NONE)]);
        assert!(output.platform_output.commands.iter().any(|command|matches!(command,egui::OutputCommand::CopyText(text) if text=="Beta,=1+2\nAlpha,שלום\n")));
        open_copy(&ctx, &mut app);
        let id = control(&ctx, 7);
        ctx.memory_mut(|memory| memory.request_focus(id));
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
        assert!(!egui::Popup::is_any_open(&ctx));
        assert_eq!(app.draft, "Kept draft");
        open_copy(&ctx, &mut app);
        let id = control(&ctx, 6);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            prompt_ui::modal(ctx, egui::Id::new("foreign-selection-copy"), 220.0, |ui| {
                prompt_ui::heading(ui, "Information", "Foreign")
            });
        });
        ctx.memory_mut(|memory| memory.request_focus(id));
        assert!(
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Enter, egui::Modifiers::NONE)]
            )
            .platform_output
            .commands
            .is_empty()
        );
    }
    let preview = Preview::new(format!("{}\n", "x".repeat(MAX_OUTPUT - 1)), b',');
    let index = preview.index.as_ref().unwrap();
    let start = std::time::Instant::now();
    let output = rectangle::serialize_as(
        &preview.source,
        index,
        preview.projection.rows(1, false),
        (0, 0),
        (0, 0),
        b',',
    )
    .unwrap();
    assert_eq!(output.len(), MAX_OUTPUT);
    assert_eq!(output.capacity(), MAX_OUTPUT);
    eprintln!(
        "512KiBselectionCSV {:?}; Export{}B plus8controls",
        start.elapsed(),
        std::mem::size_of::<Export>()
    );
    let preview = Preview::new(format!("{}\n", ",".repeat(MAX_OUTPUT - 2)), b'\t');
    assert!(
        rectangle::serialize_as(
            &preview.source,
            preview.index.as_ref().unwrap(),
            preview.projection.rows(1, false),
            (0, 0),
            (0, 0),
            b','
        )
        .is_err()
    );
}
#[test]
fn absent_source_and_changed_rectangle_disable_or_refuse_old_selection_action() {
    let (ctx, _app) = fixture();
    assert!(!ctx.read_response(control(&ctx, 6)).unwrap().enabled());
    let (ctx, mut app) = selected();
    let old = control(&ctx, 6);
    let path = app.files.tabs[0].path.clone();
    let Some(Ok(Content::Table(preview))) = &mut app.files.tabs[0].content else {
        panic!()
    };
    preview.grid.select(&ctx, &path, 1, 0, 1);
    ctx.memory_mut(|memory| memory.request_focus(old));
    let output = frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(output.platform_output.commands.is_empty());
    assert_ne!(old, control(&ctx, 6));
    app.files.tabs[0].source = true;
    frame(&ctx, &mut app, vec![]);
    open_copy(&ctx, &mut app);
    assert!(!ctx.read_response(control(&ctx, 6)).unwrap().enabled());
    assert!(!ctx.read_response(control(&ctx, 7)).unwrap().enabled());
    let _ = &mut app;
}
