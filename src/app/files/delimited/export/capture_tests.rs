use super::ui_tests::{control, key, pointer};
use super::*;
use crate::app::files::tests::frame;
#[test]
fn explicit_rectangle_to_draft_is_exact_once_bounded_and_never_sent() {
    let (ctx, mut app) = super::selection_tests::selected();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let id = control(&ctx, 8);
    let pos = ctx.read_response(id).unwrap().interact_rect.center();
    for pressed in [true, false] {
        let mut events = pointer(pos, pressed);
        events.extend([
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
        ]);
        frame(&ctx, &mut app, events);
    }
    for _ in 0..2 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(app.outgoing_text().starts_with("Kept draft"));
    assert!(app.outgoing_text().contains("Beta\t=1+2\nAlpha\tשלום\n"));
    assert_eq!(app.outgoing_text().matches("Beta").count(), 1);
    assert!(!app.outgoing_text().contains("Name\tNote"));
    assert!(
        app.outgoing_text()
            .contains("applied view rows 2–3, columns 1–2")
    );
    assert!(commands.try_recv().is_err());
    assert!(app.state.busy);
}
#[test]
fn rectangle_capture_native_keyboard_new_question_and_removed_range_keep_scope() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (ctx, mut app) = super::selection_tests::selected();
        let id = control(&ctx, 8);
        ctx.memory_mut(|memory| memory.request_focus(id));
        app.state.dialogs.push_back(questions::demo_dialog());
        let output = frame(&ctx, &mut app, vec![key(activation, egui::Modifiers::NONE)]);
        assert!(output.platform_output.commands.is_empty());
        frame(&ctx, &mut app, vec![]);
        assert!(app.outgoing_text().contains("Beta"));
        assert_eq!(app.state.dialogs.len(), 1);
        let draft = app.outgoing_text().clone();
        let path = app.files.tabs[0].path.clone();
        let Some(Ok(Content::Table(preview))) = &mut app.files.tabs[0].content else {
            panic!()
        };
        preview.grid.select(&ctx, &path, 1, 0, 1);
        ctx.memory_mut(|memory| memory.request_focus(id));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert_eq!(app.outgoing_text(), draft);
    }
}
#[test]
fn capture_total_draft_limit_rejects_whole_and_owner_change_before_collection_refuses() {
    let (ctx, mut app) = super::selection_tests::selected();
    app.draft = "x".repeat(256 * 1024);
    app.context_chips.clear_quotes();
    let before = app.outgoing_text().clone();
    let id = control(&ctx, 8);
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(app.outgoing_text(), before);
    assert!(app.state.error.as_ref().unwrap().contains("too large"));
    app.draft = "Kept".into();
    app.context_chips.clear_quotes();
    let owner = app.review_comment_owner();
    context_capture::bind_file_owner(&ctx, &app.prefs.cwd, &owner);
    assert!(context_capture::queue_file_selection(
        &ctx,
        "table · applied rows 1–2",
        "a\tb\n"
    ));
    app.state.session_file = Some("/different/session.jsonl".into());
    app.collect_context_capture();
    assert_eq!(app.outgoing_text(), "Kept");
    assert!(
        app.state
            .error
            .as_ref()
            .unwrap()
            .contains("session changed")
    );
}
#[test]
fn capture_stale_pass_owner_and_oversized_ui_never_queue_partial_values() {
    let (ctx, mut app) = super::selection_tests::selected();
    frame(&ctx, &mut app, vec![]);
    assert!(!context_capture::queue_file_selection(
        &ctx,
        "stale",
        "must not add"
    ));
    egui::Popup::close_all(&ctx);
    app.files.tabs[0].content = Some(Ok(Content::Table(Preview::new(
        format!("{}\n", "x".repeat(25 * 1024)),
        b',',
    ))));
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let path = app.files.tabs[0].path.clone();
    let Some(Ok(Content::Table(preview))) = &mut app.files.tabs[0].content else {
        panic!()
    };
    preview.grid.focus(&ctx, &path);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::A, egui::Modifiers::COMMAND)],
    );
    super::ui_tests::open_copy(&ctx, &mut app);
    let id = control(&ctx, 8);
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(app.outgoing_text(), "Kept draft");
    let Some(Ok(Content::Table(preview))) = &app.files.tabs[0].content else {
        panic!()
    };
    assert!(
        preview
            .export
            .notice
            .as_ref()
            .unwrap()
            .contains("nothing added")
    );
}
#[test]
fn capture_headerless_filtered_ragged_provenance_and_foreign_modal_are_truthful() {
    let (ctx, mut app) = super::selection_tests::selected();
    egui::Popup::close_all(&ctx);
    let path = app.files.tabs[0].path.clone();
    app.files.tabs[0].content = Some(Ok(Content::Table(Preview::new(
        "Name,Note\nshort\nBeta,=1+2\n".into(),
        b',',
    ))));
    app.prefs.table_headers.set(&app.prefs.cwd, &path, false);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let Some(Ok(Content::Table(preview))) = &mut app.files.tabs[0].content else {
        panic!()
    };
    projection::tests::apply(&ctx, preview, "short", projection::Order::Descending);
    preview.grid.focus(&ctx, &path);
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::A, egui::Modifiers::COMMAND)],
    );
    super::ui_tests::open_copy(&ctx, &mut app);
    let id = control(&ctx, 8);
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(app.outgoing_text().contains("short\t\"\"\n"));
    assert!(
        app.outgoing_text()
            .contains("applied view rows 1–1, columns 1–2")
    );
    assert!(!app.outgoing_text().contains("Beta"));
    let draft = app.outgoing_text().clone();
    super::ui_tests::open_copy(&ctx, &mut app);
    let id = control(&ctx, 8);
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
    super::ui_tests::open_copy(&ctx, &mut app);
    let id = control(&ctx, 8);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        prompt_ui::modal(ctx, egui::Id::new("foreign-range-capture"), 220.0, |ui| {
            prompt_ui::heading(ui, "Information", "Foreign")
        });
    });
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(app.outgoing_text(), draft);
}
#[test]
fn capture_exact_preflight_refuses_oversize_without_excerpt_fallback() {
    let preview = Preview::new(format!("{}\n", "x".repeat(24 * 1024)), b',');
    let index = preview.index.as_ref().unwrap();
    assert!(
        rectangle::serialize_limited(
            &preview.source,
            index,
            preview.projection.rows(1, false),
            (0, 0),
            (0, 0),
            b'\t',
            24 * 1024 - 1
        )
        .is_err()
    );
    let start = std::time::Instant::now();
    let preview = Preview::new(format!("{}\n", "x".repeat(24 * 1024 - 2)), b',');
    let output = rectangle::serialize_limited(
        &preview.source,
        preview.index.as_ref().unwrap(),
        preview.projection.rows(1, false),
        (0, 0),
        (0, 0),
        b'\t',
        24 * 1024 - 1,
    )
    .unwrap();
    assert_eq!(output.len(), 24 * 1024 - 1);
    eprintln!("24KiBselectioncapturepreflight {:?}", start.elapsed());
}
