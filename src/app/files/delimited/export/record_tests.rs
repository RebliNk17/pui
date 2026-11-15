use super::*;
use crate::app::files::delimited::grid::tests::{frame, key, setup};
fn record(preview: &Preview, record: usize, delimiter: u8) -> Result<String, &'static str> {
    serialize_records(
        &preview.source,
        preview.index.as_ref().unwrap(),
        std::iter::once(Some(record)),
        delimiter,
    )
}
#[test]
fn exact_original_record_serializes_unicode_multiline_ragged_empty_and_formula_fields_only() {
    let source =
        "Name,Note,Formula\r\nAlpha,\"a\"\"b\n世界\",=1+2\r\nshort\r\n\"\",\"tab\tvalue\",\r\n";
    let preview = Preview::new(source.into(), b',');
    for delimiter in [b',', b'\t'] {
        for original in 0..4 {
            let copied = record(&preview, original, delimiter).unwrap();
            let reparsed = parse(&copied, delimiter).unwrap();
            assert_eq!(reparsed.records.len(), 1);
            let before = preview.index.as_ref().unwrap();
            let a = &before.cells[before.records[original].clone()];
            let b = &reparsed.cells[reparsed.records[0].clone()];
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                assert_eq!(a.value(source), b.value(&copied));
            }
            if original == 1 {
                assert!(copied.contains("=1+2") && !copied.starts_with("Name"));
            }
        }
    }
    assert_eq!(record(&preview, 2, b',').unwrap(), "short\n");
    assert_eq!(
        record(&Preview::new("\n".into(), b','), 0, b'\t').unwrap(),
        "\"\"\n"
    );
    assert!(record(&preview, 4, b',').is_err());
    assert!(
        serialize_records(
            source,
            preview.index.as_ref().unwrap(),
            [Some(0), None].into_iter(),
            b','
        )
        .is_err(),
        "never skip an unavailable projected record"
    );
    assert!(record(&preview, 0, b';').is_err());
    assert_eq!(preview.source.as_str(), source);
}
#[test]
fn active_original_record_validates_applied_position_and_not_unapplied_query_or_header_toggle() {
    let (ctx, mut preview) = setup("Name,Note\nBeta,two\nAlpha,one\n".into(), true);
    projection::tests::apply(&ctx, &mut preview, "", projection::Order::Ascending);
    let index = preview.index.as_ref().unwrap();
    let rows = preview.projection.rows(index.records.len(), preview.header);
    let position = rows.position(1).unwrap();
    preview
        .grid
        .select(&ctx, Path::new("table.csv"), 1, 0, position);
    assert_eq!(preview.grid.current_record(rows), Some(1));
    assert_eq!(record(&preview, 1, b',').unwrap(), "Beta,two\n");
    preview.projection.demo();
    let rows = preview.projection.rows(index.records.len(), preview.header);
    assert_eq!(
        preview.grid.current_record(rows),
        Some(1),
        "unapplied draft does not change active original identity"
    );
    preview.grid.active.0 = 2;
    assert!(
        preview.grid.current_record(rows).is_none(),
        "stale active/position refused rather than copying a hidden record"
    );
}
#[test]
fn actual_header_toggle_and_applied_filter_reconcile_record_before_menu_copy() {
    let (ctx, mut preview) = setup("Name,Note\nBeta,two\nAlpha,one\n".into(), true);
    let size = Vec2::new(620.0, 760.0);
    preview
        .grid
        .select(&ctx, Path::new("keyboard.csv"), 2, 0, 2);
    projection::tests::apply(&ctx, &mut preview, "Alpha", projection::Order::Descending);
    assert_eq!(preview.grid.active.0, 2);
    assert_eq!(
        preview
            .grid
            .current_record(preview.projection.rows(3, true)),
        Some(2)
    );
    preview.set_header(false);
    for _ in 0..3 {
        frame(&ctx, &mut preview, size, vec![]);
    }
    assert_eq!(preview.grid.active.0, 2);
    assert_eq!(
        preview
            .grid
            .current_record(preview.projection.rows(3, false)),
        Some(2)
    );
    assert_eq!(record(&preview, 2, b'\t').unwrap(), "Alpha\tone\n");
}

#[test]
fn actual_copy_record_is_original_not_first_filtered_row_and_retains_no_clipboard_body() {
    let (ctx, mut preview) = setup("Name,Note\nBeta,=1+2\nAlpha,שלום\n".into(), true);
    let size = Vec2::new(620.0, 760.0);
    projection::tests::apply(&ctx, &mut preview, "", projection::Order::Ascending);
    let index = preview.index.as_ref().unwrap();
    let position = preview
        .projection
        .rows(index.records.len(), true)
        .position(1)
        .unwrap();
    preview
        .grid
        .select(&ctx, Path::new("keyboard.csv"), 1, 0, position);
    for _ in 0..3 {
        frame(&ctx, &mut preview, size, vec![]);
    }
    for (slot, expected) in [(2, "Beta,=1+2\n"), (3, "Beta\t=1+2\n")] {
        super::tests::open_copy(&ctx, &mut preview, size);
        let id = ctx
            .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("table-export", slot))))
            .unwrap();
        assert!(
            ctx.read_response(id).unwrap().enabled(),
            "active {:?}, current {:?}, rows {:?}",
            preview.grid.active,
            preview.grid.current_record(
                preview
                    .projection
                    .rows(preview.index.as_ref().unwrap().records.len(), true)
            ),
            (0..3)
                .map(|position| preview.projection.rows(3, true).record(position))
                .collect::<Vec<_>>()
        );
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
        assert!(
            preview
                .export
                .notice
                .as_ref()
                .unwrap()
                .contains("original record 2")
        );
        assert!(preview.export.notice.as_ref().unwrap().capacity() < 160);
        assert!(
            frame(&ctx, &mut preview, size, vec![])
                .platform_output
                .commands
                .is_empty()
        );
    }
}
#[test]
fn focused_old_record_cannot_adopt_changed_active_record_on_next_render() {
    let (ctx, mut preview) = setup("Name,Note\nBeta,two\nAlpha,one\n".into(), true);
    let size = Vec2::new(620.0, 760.0);
    super::tests::open_copy(&ctx, &mut preview, size);
    let old = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("table-export", 2))))
        .unwrap();
    preview
        .grid
        .select(&ctx, Path::new("keyboard.csv"), 1, 0, 1);
    ctx.memory_mut(|memory| memory.request_focus(old));
    let output = frame(
        &ctx,
        &mut preview,
        size,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(output.platform_output.commands.is_empty());
    let new = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("table-export", 2))))
        .unwrap();
    assert_ne!(old, new);
}
#[test]
fn one_record_expansion_limit_preflights_before_partial_clipboard_and_large_other_records_do_not_count()
 {
    let preview = Preview::new(format!("ok\n{}\n", "\t".repeat(MAX_OUTPUT - 5)), b',');
    assert_eq!(record(&preview, 0, b'\t').unwrap(), "ok\n");
    let preview = Preview::new(format!("{}\n", "\t".repeat(MAX_OUTPUT - 2)), b',');
    assert!(record(&preview, 0, b'\t').is_err());
    let preview = Preview::new(format!("{}\n", "x".repeat(MAX_OUTPUT - 1)), b',');
    let start = std::time::Instant::now();
    let copied = record(&preview, 0, b',').unwrap();
    assert_eq!(copied.len(), MAX_OUTPUT);
    eprintln!(
        "512KiBrecordserialize {:?}; exactoutput{}B/capacity{}B; Export{}B statusoutsidepayloadpool",
        start.elapsed(),
        copied.len(),
        copied.capacity(),
        std::mem::size_of::<Export>()
    );
}
