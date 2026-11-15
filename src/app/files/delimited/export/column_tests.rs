use super::ui_tests::{control, fixture, key, open_copy, pointer};
use super::*;
use crate::app::files::tests::frame;
#[test]
fn column_exact_unicode_quotes_ragged_empty_and_formulas_in_applied_order() {
    let (ctx, mut preview) = grid::tests::setup(
        "Name,Note\r\nBeta,=1+2\r\nAlpha,\"世界\n\"\"quote\"\"\"\r\nshort\r\n".into(),
        true,
    );
    projection::tests::apply(&ctx, &mut preview, "", projection::Order::Ascending);
    let index = preview.index.as_ref().unwrap();
    let rows = preview.projection.rows(index.records.len(), true);
    assert_eq!(
        column::serialize(&preview.source, index, rows, 1, b',').unwrap(),
        "Note\n\"世界\n\"\"quote\"\"\"\n=1+2\n\"\"\n"
    );
    assert!(column::serialize(&preview.source, index, rows, 2, b',').is_err());
    assert!(column::serialize(&preview.source, index, rows, 1, b';').is_err());
    projection::tests::apply(&ctx, &mut preview, "Beta", projection::Order::Original);
    let index = preview.index.as_ref().unwrap();
    let rows = preview.projection.rows(index.records.len(), true);
    assert_eq!(
        column::serialize(&preview.source, index, rows, 1, b'\t').unwrap(),
        "Note\n=1+2\n"
    );
}
#[test]
fn column_pointer_and_native_keyboard_copy_once_with_question_and_chords() {
    for slot in [4, 5] {
        let (ctx, mut app) = fixture();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.state.dialogs.push_back(questions::demo_dialog());
        let pos = ctx
            .read_response(control(&ctx, slot))
            .unwrap()
            .rect
            .center();
        let mut copies = 0;
        for pressed in [true, false] {
            let mut events = pointer(pos, pressed);
            events.extend([
                key(egui::Key::Enter, egui::Modifiers::COMMAND),
                key(egui::Key::Backspace, egui::Modifiers::COMMAND),
            ]);
            let output = frame(&ctx, &mut app, events);
            copies+=output.platform_output.commands.iter().filter(|command|matches!(command,egui::OutputCommand::CopyText(text) if text=="Name\nBeta\nAlpha\n")).count();
        }
        assert_eq!(copies, 1);
        assert!(commands.try_recv().is_err());
        assert_eq!(app.draft, "Kept draft");
        assert_eq!(app.state.dialogs.len(), 1);
        open_copy(&ctx, &mut app);
        let before = control(&ctx, 3);
        ctx.memory_mut(|memory| memory.request_focus(before));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
        );
        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            Some(control(&ctx, 4))
        );
        let output = frame(
            &ctx,
            &mut app,
            vec![
                key(egui::Key::Space, egui::Modifiers::NONE),
                egui::Event::Text(" ".into()),
            ],
        );
        assert!(output.platform_output.commands.iter().any(|command|matches!(command,egui::OutputCommand::CopyText(text) if text=="Name\nBeta\nAlpha\n")));
    }
}
#[test]
fn stale_column_focus_and_source_view_cannot_copy() {
    let (ctx, mut app) = fixture();
    let old = control(&ctx, 4);
    let Some(Ok(Content::Table(preview))) = &mut app.files.tabs[0].content else {
        panic!()
    };
    preview.grid.active.1 = 1;
    ctx.memory_mut(|memory| memory.request_focus(old));
    let output = frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(output.platform_output.commands.is_empty());
    assert_ne!(control(&ctx, 4), old);
    app.files.tabs[0].source = true;
    frame(&ctx, &mut app, vec![]);
    open_copy(&ctx, &mut app);
    assert!(!ctx.read_response(control(&ctx, 4)).unwrap().enabled());
    assert!(!ctx.read_response(control(&ctx, 5)).unwrap().enabled());
}
#[test]
fn column_headerless_empty_projection_modal_and_reverse_escape_refuse_or_cancel() {
    let (ctx, mut app) = fixture();
    egui::Popup::close_all(&ctx);
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
    assert!(!ctx.read_response(control(&ctx, 4)).unwrap().enabled());
    let Some(Ok(Content::Table(preview))) = &mut app.files.tabs[0].content else {
        panic!()
    };
    projection::tests::apply(&ctx, preview, "Alpha", projection::Order::Original);
    let index = preview.index.as_ref().unwrap();
    assert_eq!(
        column::serialize(
            &preview.source,
            index,
            preview.projection.rows(index.records.len(), false),
            1,
            b'\t'
        )
        .unwrap(),
        "שלום\n"
    );
    open_copy(&ctx, &mut app);
    let id = control(&ctx, 4);
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
    open_copy(&ctx, &mut app);
    let id = control(&ctx, 4);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        prompt_ui::modal(ctx, egui::Id::new("column-foreign"), 220.0, |ui| {
            prompt_ui::heading(ui, "Information", "Foreign")
        });
    });
    ctx.memory_mut(|memory| memory.request_focus(id));
    let output = frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(output.platform_output.commands.is_empty());
    assert_eq!(app.draft, "Kept draft");
}
#[test]
fn oversized_column_ui_leaves_clipboard_unchanged() {
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
    let id = control(&ctx, 5);
    ctx.memory_mut(|memory| memory.request_focus(id));
    let output = frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(output.platform_output.commands.is_empty());
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
}
#[test]
fn column_output_bound_is_exact_and_preflights_escaping() {
    let preview = Preview::new(format!("{}\n", "x".repeat(MAX_OUTPUT - 1)), b',');
    let index = preview.index.as_ref().unwrap();
    let start = std::time::Instant::now();
    let output = column::serialize(
        &preview.source,
        index,
        preview.projection.rows(1, false),
        0,
        b',',
    )
    .unwrap();
    assert_eq!(output.len(), MAX_OUTPUT);
    assert_eq!(output.capacity(), MAX_OUTPUT);
    eprintln!(
        "512KiBcolumnserialize {:?}; Export{}B",
        start.elapsed(),
        std::mem::size_of::<Export>()
    );
    let preview = Preview::new(format!("{}\n", "\t".repeat(MAX_OUTPUT - 2)), b',');
    assert!(
        column::serialize(
            &preview.source,
            preview.index.as_ref().unwrap(),
            preview.projection.rows(1, false),
            0,
            b'\t'
        )
        .is_err()
    );
}
