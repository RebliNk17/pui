use super::*;
use crate::app::files::delimited::grid::tests::{frame, key, setup};
pub(super) fn open_copy(ctx: &egui::Context, preview: &mut Preview, size: Vec2) {
    egui::Popup::close_all(ctx);
    frame(ctx, preview, size, vec![]);
    let id = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("table-toolbar", 3usize))))
        .unwrap();
    let pos = ctx.read_response(id).unwrap().rect.center();
    for pressed in [true, false] {
        frame(
            ctx,
            preview,
            size,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    for _ in 0..3 {
        frame(ctx, preview, size, vec![]);
    }
}
fn view(preview: &Preview, delimiter: u8) -> Result<String, &'static str> {
    serialize(
        &preview.source,
        preview.index.as_ref().unwrap(),
        preview.projection.rows(
            preview.index.as_ref().unwrap().records.len(),
            preview.header,
        ),
        delimiter,
    )
}
#[test]
fn csv_tsv_escaping_roundtrip_unicode_quotes_newlines_empty_ragged_and_literal_formulas() {
    let source =
        "Name,Note,Formula\r\nAlpha,\"a\"\"b\n世界\",=1+2\r\nshort\r\n\"\",\"tab\tvalue\",\r\n";
    for delimiter in [b',', b'\t'] {
        let preview = Preview::new(source.into(), b',');
        let copied = view(&preview, delimiter).unwrap();
        let reparsed = parse(&copied, delimiter).unwrap();
        let original = preview.index.as_ref().unwrap();
        assert_eq!(original.records.len(), reparsed.records.len());
        for record in 0..original.records.len() {
            let a = &original.cells[original.records[record].clone()];
            let b = &reparsed.cells[reparsed.records[record].clone()];
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                assert_eq!(a.value(source), b.value(&copied));
            }
        }
        assert!(copied.contains("=1+2"));
        assert!(copied.contains("\"\""));
        assert_eq!(preview.source.as_str(), source);
    }
}
#[test]
fn applied_sort_filter_header_and_headerless_export_keep_exact_current_records() {
    let (ctx, mut preview) = setup("Name,Note\nBeta,two\nAlpha,one\n".into(), true);
    crate::app::files::delimited::projection::tests::apply(
        &ctx,
        &mut preview,
        "Alpha",
        projection::Order::Descending,
    );
    assert_eq!(view(&preview, b',').unwrap(), "Name,Note\nAlpha,one\n");
    preview.set_header(false);
    assert_eq!(view(&preview, b'\t').unwrap(), "Alpha\tone\n");
    crate::app::files::delimited::projection::tests::apply(
        &ctx,
        &mut preview,
        "absent",
        projection::Order::Original,
    );
    assert_eq!(view(&preview, b',').unwrap(), "");
}
#[test]
fn output_limit_is_complete_or_rejected_not_truncated_and_empty_record_is_preserved() {
    // CSV source can keep tabs unquoted; TSV must quote the whole field.
    // The source is valid/in-limit while the serialized view exceeds its cap.
    let source = format!("{}\n", "\t".repeat(MAX_OUTPUT - 2));
    let preview = Preview::new(source, b',');
    assert!(view(&preview, b'\t').is_err());
    let preview = Preview::new("\n".into(), b',');
    assert_eq!(view(&preview, b',').unwrap(), "\"\"\n");
    let preview = Preview::new(format!("{}\n", "x".repeat(MAX_OUTPUT - 1)), b',');
    assert_eq!(view(&preview, b',').unwrap().len(), MAX_OUTPUT);
    assert!(
        serialize(
            "",
            &parse("", b',').unwrap(),
            preview.projection.rows(0, false),
            b';'
        )
        .is_err()
    );
}
#[test]
fn maximum_cell_view_serialization_and_retained_status_are_bounded() {
    let source = (0..MAX_RECORDS)
        .map(|record| format!("{record},x,x,x,x,x,x,x,x,x\n"))
        .collect::<String>();
    let preview = Preview::new(source, b',');
    let start = std::time::Instant::now();
    let output = view(&preview, b',').unwrap();
    let elapsed = start.elapsed();
    assert_eq!(output.lines().count(), MAX_RECORDS);
    assert!(output.len() <= MAX_OUTPUT);
    eprintln!(
        "100,000cellclipboardserialize {elapsed:?}; output{}B/capacity{}B; retainedexportallowance512B",
        output.len(),
        output.capacity()
    );
    let (ctx, mut preview) = setup("Name,Note\nA,body\n".into(), true);
    let size = Vec2::new(620.0, 760.0);
    let mut delay = Duration::ZERO;
    frame(&ctx, &mut preview, size, vec![egui::Event::PointerGone]);
    for _ in 0..100 {
        let out = frame(&ctx, &mut preview, size, vec![]);
        delay = out.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        assert!(out.platform_output.commands.is_empty());
    }
    assert!(delay > Duration::from_secs(1), "settled delay {delay:?}");
    assert!(std::mem::size_of::<Export>() + 160 < 512);
    assert!(preview.export.notice.is_none());
}

#[test]
fn actual_copy_uses_applied_projection_not_unapplied_query_and_old_controls_cannot_retarget() {
    let (ctx, mut preview) = setup("Name,Note\nBeta,two\nAlpha,one\n".into(), true);
    let size = Vec2::new(620.0, 760.0);
    projection::tests::apply(&ctx, &mut preview, "Alpha", projection::Order::Descending);
    preview.projection.demo(); // changes only the unapplied query/sort settings
    for _ in 0..3 {
        frame(&ctx, &mut preview, size, vec![]);
    }
    open_copy(&ctx, &mut preview, size);
    let csv = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("table-export", 0))))
        .unwrap();
    ctx.memory_mut(|memory| memory.request_focus(csv));
    let out = frame(
        &ctx,
        &mut preview,
        size,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(out.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == "Name,Note\nAlpha,one\n")));
    assert!(preview.export.notice.as_ref().unwrap().capacity() < 160);
    let mut replacement = Preview::new("Different,owner\nPrivate,content\n".into(), b',');
    ctx.memory_mut(|memory| memory.request_focus(csv));
    let out = frame(
        &ctx,
        &mut replacement,
        size,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(out.platform_output.commands.is_empty());
    assert!(replacement.export.notice.is_none());
    preview.export.hidden();
    assert!(preview.export.controls.iter().all(Option::is_none));
    assert!(!preview.export.owns_keyboard(&ctx));
}

#[test]
fn actual_clipboard_buttons_output_current_view_and_no_idle_serialization() {
    let (ctx, mut preview) = setup("Name,Note\nBeta,=1+2\nAlpha,שלום\n".into(), true);
    let size = Vec2::new(620.0, 760.0);
    for _ in 0..3 {
        frame(&ctx, &mut preview, size, vec![]);
    }
    for (slot, expected) in [
        (0, "Name,Note\nBeta,=1+2\nAlpha,שלום\n"),
        (1, "Name\tNote\nBeta\t=1+2\nAlpha\tשלום\n"),
    ] {
        open_copy(&ctx, &mut preview, size);
        let id = ctx
            .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("table-export", slot))))
            .unwrap();
        ctx.memory_mut(|memory| memory.request_focus(id));
        let output = frame(
            &ctx,
            &mut preview,
            size,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert!(output.platform_output.commands.iter().any(
            |command| matches!(command,egui::OutputCommand::CopyText(text) if text==expected)
        ));
        let output = frame(&ctx, &mut preview, size, vec![]);
        assert!(output.platform_output.commands.is_empty());
    }
}
