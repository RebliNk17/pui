use super::*;
use crate::app::files::delimited::grid::tests::{frame, key, setup};

const SIZE: Vec2 = Vec2::new(620.0, 600.0);
fn build(source: &str, delimiter: u8, query: &str, order: Order, column: usize) -> Map {
    Map::build(
        source,
        &parse(source, delimiter).unwrap(),
        query,
        Sort { column, order },
    )
}
fn ids(map: &Map) -> Vec<u32> {
    map.records.to_vec()
}
pub(in crate::app::files::delimited) fn apply(
    ctx: &egui::Context,
    preview: &mut Preview,
    query: &str,
    order: Order,
) {
    preview.projection.draft = query.into();
    preview.projection.sort.order = order;
    assert!(preview.projection.start(
        ctx,
        preview.source.clone(),
        preview.index.as_ref().unwrap().clone()
    ));
    finish(ctx, preview);
}
fn finish(ctx: &egui::Context, preview: &mut Preview) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while preview.projection.pending.is_some() {
        frame(ctx, preview, SIZE, vec![]);
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
}
pub(super) fn point(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing label {label}"))
}
fn click(ctx: &egui::Context, preview: &mut Preview, pos: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            ctx,
            preview,
            SIZE,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}
#[test]
fn stable_literal_text_sort_never_infers_numbers_or_evaluates_formulas() {
    let source = "value,note\n2,equal\n10,numeric-looking\n2,equal later\n=1+2,literal\n02,zero\n";
    assert_eq!(
        ids(&build(source, b',', "", Order::Ascending, 0)),
        [5, 2, 1, 3, 4, 0]
    );
    assert_eq!(
        ids(&build(source, b',', "", Order::Descending, 0)),
        [0, 4, 1, 3, 2, 5]
    );
    assert_eq!(
        ids(&build(source, b',', "", Order::Original, 0)),
        [0, 1, 2, 3, 4, 5]
    );
    assert_eq!(
        ids(&build("a,b\nx\ny,\nz,1\n", b',', "", Order::Ascending, 1)),
        [1, 2, 3, 0]
    );
}
#[test]
fn filter_is_case_sensitive_literal_any_complete_decoded_cell_csv_and_tsv() {
    let source = "Name,Note\nAlpha,\"a\"\"b\n世界\"\nBeta,=SUM(A1:A2)\nalpha,[a-z].*\n";
    assert_eq!(
        ids(&build(source, b',', "a\"b\n世界", Order::Original, 0)),
        [1]
    );
    assert_eq!(ids(&build(source, b',', "Alpha", Order::Original, 0)), [1]);
    assert_eq!(ids(&build(source, b',', "alpha", Order::Original, 0)), [3]);
    assert_eq!(
        ids(&build(source, b',', "[a-z].*", Order::Original, 0)),
        [3]
    );
    assert_eq!(
        ids(&build(source, b',', "SUM(A1:A2)", Order::Original, 0)),
        [2]
    );
    assert_eq!(
        ids(&build(
            "Name\tNote\r\nA\t\"with\ttab\"\r\n",
            b'\t',
            "with\ttab",
            Order::Original,
            0
        )),
        [1]
    );
    let long = format!("name,note\nx,{}needle\n", "z".repeat(5000));
    assert_eq!(ids(&build(&long, b',', "needle", Order::Original, 0)), [1]);
}
#[test]
fn row_maps_roundtrip_original_identity_and_pin_header_without_rescan() {
    for query in ["", "no match", "header", "a"] {
        for order in [Order::Original, Order::Ascending, Order::Descending] {
            let map = build("header\nb\na\na\n", b',', query, order, 0);
            for header in [false, true] {
                let rows = Rows {
                    map: Some(&map),
                    records: 4,
                    header,
                };
                for position in 0..rows.len() {
                    let original = rows.record(position).unwrap();
                    assert_eq!(rows.position(original), Some(position));
                    assert!(original < 4);
                    if header && position == 0 {
                        assert_eq!(original, 0);
                    }
                }
                assert_eq!(rows.record(rows.len()), None);
                if header {
                    assert_eq!(rows.position(0), Some(0));
                }
            }
        }
    }
}
#[test]
fn projected_keyboard_copy_uses_visible_order_and_source_record_identity() {
    let (ctx, mut preview) = setup(
        "name,note\nBeta,=1+2\nAlpha,שלום\nAlpha,last\n".into(),
        true,
    );
    let source_ptr = preview.source.as_ptr();
    let cells_ptr = preview.index.as_ref().unwrap().cells.as_ptr();
    apply(&ctx, &mut preview, "", Order::Ascending);
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![
            key(egui::Key::ArrowDown, egui::Modifiers::NONE),
            key(egui::Key::ArrowRight, egui::Modifiers::NONE),
        ],
    );
    assert_eq!(preview.grid.active, (2, 1));
    let copied = frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![
            egui::Event::Copy,
            key(egui::Key::ArrowDown, egui::Modifiers::NONE),
        ],
    );
    assert_eq!(preview.grid.active, (3, 1));
    assert!(
        copied.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(text) if text == "שלום")
        )
    );
    assert_eq!(preview.source.as_ptr(), source_ptr);
    assert_eq!(preview.index.as_ref().unwrap().cells.as_ptr(), cells_ptr);
    let original = preview.grid.active;
    preview.set_header(false);
    frame(&ctx, &mut preview, SIZE, vec![]);
    assert_eq!(preview.grid.active, original);
    apply(&ctx, &mut preview, "Alpha", Order::Descending);
    assert_eq!(preview.grid.active, original);
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![key(egui::Key::Home, egui::Modifiers::COMMAND)],
    );
    assert_eq!(preview.grid.active, (2, 0));
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![key(egui::Key::End, egui::Modifiers::COMMAND)],
    );
    assert_eq!(preview.grid.active, (3, 1));
    preview.projection.reset(&ctx, &mut preview.grid, 4, false);
    assert_eq!(preview.grid.active, (3, 1));
    assert!(preview.projection.map.is_none());
}
#[test]
fn completed_delivery_releases_permit_before_worker_return_without_retry() {
    let ctx = egui::Context::default();
    let mut preview = Preview::new("name\na\nb\n".into(), b',');
    let (entered, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    preview.projection.after_delivery = Some(Box::new(move || {
        entered.send(()).unwrap();
        blocked.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    assert!(preview.projection.start(
        &ctx,
        preview.source.clone(),
        preview.index.as_ref().unwrap().clone()
    ));
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    // Worker is deliberately still alive. Its delivered result must be actionable.
    preview.projection.poll(&ctx, &mut preview.grid, 3, true);
    assert!(preview.projection.pending.is_none());
    let busy = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Arc<AtomicBool>>(egui::Id::new("table-projection-worker"))
            .clone()
    });
    let released = !busy.load(Ordering::Acquire);
    preview.projection.draft = "b".into();
    let started = preview.projection.start(
        &ctx,
        preview.source.clone(),
        preview.index.as_ref().unwrap().clone(),
    );
    release.send(()).unwrap();
    assert!(released, "completion arrived before permit release");
    assert!(started, "first post-completion Apply must not need a retry");
    finish(&ctx, &mut preview);
    assert_eq!(ids(preview.projection.map.as_ref().unwrap()), [2]);
}
#[test]
fn removed_selection_is_explicit_and_no_match_never_copies_hidden_record() {
    let (ctx, mut preview) = setup("name,note\nBeta,hidden\nAlpha,visible\n".into(), false);
    preview.grid.active = (1, 1);
    apply(&ctx, &mut preview, "Alpha", Order::Original);
    assert_eq!(preview.grid.active, (2, 1));
    assert!(
        preview
            .projection
            .notice
            .unwrap()
            .contains("selected record is filtered out")
    );
    apply(&ctx, &mut preview, "absent", Order::Original);
    let output = frame(&ctx, &mut preview, SIZE, vec![egui::Event::Copy]);
    assert_eq!(preview.projection.rows(3, false).len(), 0);
    assert!(
        output
            .platform_output
            .commands
            .iter()
            .all(|command| !matches!(command, egui::OutputCommand::CopyText(_)))
    );
    preview.projection.reset(&ctx, &mut preview.grid, 3, false);
    assert_eq!(preview.projection.rows(3, false).record(1), Some(1));
}
#[test]
fn explicit_apply_only_typing_and_sort_controls_do_not_scan_and_unicode_is_bounded() {
    let (ctx, mut preview) = setup("name,note\nBeta,hello\nAlpha,שלום\n".into(), true);
    let output = frame(&ctx, &mut preview, SIZE, vec![]);
    click(&ctx, &mut preview, point(&output, "Filter"));
    for _ in 0..3 {
        frame(&ctx, &mut preview, SIZE, vec![]);
    }
    // Find the explicitly salted edit via the remembered toolbar list by
    // focusing each candidate until its TextEdit state is present.
    let id = *preview
        .projection
        .controls
        .iter()
        .find(|id| egui::TextEdit::load_state(&ctx, **id).is_some())
        .unwrap();
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![egui::Event::Text("界".repeat(200))],
    );
    assert_eq!(preview.projection.draft.chars().count(), QUERY_CHARS);
    assert!(preview.projection.draft.capacity() <= QUERY_CHARS * 4);
    for _ in 0..8 {
        frame(&ctx, &mut preview, SIZE, vec![]);
    }
    assert_eq!(preview.projection.started, 0);
    assert!(preview.projection.map.is_none());
    preview.projection.draft = "שלום".into();
    let output = frame(&ctx, &mut preview, SIZE, vec![]);
    click(&ctx, &mut preview, point(&output, "Apply"));
    finish(&ctx, &mut preview);
    assert_eq!(preview.projection.started, 1);
    assert_eq!(ids(preview.projection.map.as_ref().unwrap()), [2]);
}
#[test]
fn reset_cancels_pending_and_window_permit_prevents_parallel_projection_workers() {
    let ctx = egui::Context::default();
    let mut a = Preview::new("name\nb\na\n".into(), b',');
    let mut b = Preview::new("name\nz\ny\n".into(), b',');
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    a.projection.before_scan = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    }));
    assert!(
        a.projection
            .start(&ctx, a.source.clone(), a.index.as_ref().unwrap().clone())
    );
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        !b.projection
            .start(&ctx, b.source.clone(), b.index.as_ref().unwrap().clone())
    );
    assert!(b.projection.notice.unwrap().contains("Another table"));
    a.projection.reset(&ctx, &mut a.grid, 3, true);
    assert!(a.projection.pending.is_none());
    assert!(
        !b.projection
            .start(&ctx, b.source.clone(), b.index.as_ref().unwrap().clone())
    );
    release_tx.send(()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if b.projection
            .start(&ctx, b.source.clone(), b.index.as_ref().unwrap().clone())
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    finish(&ctx, &mut b);
    assert!(a.projection.map.is_none());
}
#[test]
fn closed_preview_keeps_shared_payload_lease_until_worker_drops_original_bodies() {
    let ctx = egui::Context::default();
    let lease = budget::Lease::new(&ctx);
    let mut preview = Preview::new("name\nb\na\n".into(), b',');
    let reserved = preview.retained_bytes();
    lease.reserve(reserved).unwrap();
    preview.attach_projection_budget(&lease);
    let source = Arc::downgrade(&preview.source);
    let weak_lease = Arc::downgrade(&lease);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    preview.projection.before_scan = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    }));
    assert!(preview.projection.start(
        &ctx,
        preview.source.clone(),
        preview.index.as_ref().unwrap().clone()
    ));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(preview);
    drop(lease);
    assert!(source.upgrade().is_some());
    assert!(weak_lease.upgrade().is_some());
    let other = budget::Lease::new(&ctx);
    assert!(other.reserve(64 * 1024 * 1024).is_err());
    let busy = ctx.data_mut(|data| {
        data.get_temp::<Arc<AtomicBool>>(egui::Id::new("table-projection-worker"))
            .unwrap()
    });
    release_tx.send(()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    // A Weak becoming un-upgradeable only proves the last strong owner began
    // dropping; Lease::drop may not have subtracted its pool charge yet. The
    // worker releases this permit after dropping input, so wait for completion.
    while busy.load(Ordering::Acquire) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(weak_lease.upgrade().is_none());
    assert!(source.upgrade().is_none());
    other.reserve(64 * 1024 * 1024).unwrap();
}
#[test]
fn maximum_projection_retains_only_32bit_record_ids_with_conservative_budget_charge() {
    let source = (0..MAX_RECORDS)
        .map(|row| format!("{row:05},x,x,x,x,x,x,x,x,x\n"))
        .collect::<String>();
    let preview = Preview::new(source, b',');
    let index = preview.index.as_ref().unwrap();
    assert_eq!(index.cells.len(), MAX_CELLS);
    let start = std::time::Instant::now();
    let map = Map::build(
        &preview.source,
        index,
        "x",
        Sort {
            column: 0,
            order: Order::Descending,
        },
    );
    let elapsed = start.elapsed();
    assert_eq!(map.records.len(), MAX_RECORDS);
    assert_eq!(std::mem::size_of_val(map.records.as_ref()), 40_000);
    assert!(
        Projection::reserved_bytes()
            >= 2 * std::mem::size_of_val(map.records.as_ref()) + 4 * QUERY_CHARS * 4
    );
    assert!(Projection::reserved_bytes() < 85_000);
    assert_eq!(map.records[0], 9999);
    eprintln!(
        "100000-cell filter/text sort: {elapsed:?}; map 40000 B; conservative per-table projection reservation {} B",
        Projection::reserved_bytes()
    );
}
#[test]
fn toolbar_owns_shortcuts_even_when_an_inline_question_arrives_and_tab_stays_native() {
    let (ctx, mut preview) = setup("name,note\nb,one\na,two\n".into(), true);
    let out = frame(&ctx, &mut preview, SIZE, vec![]);
    click(&ctx, &mut preview, point(&out, "Filter"));
    frame(&ctx, &mut preview, SIZE, vec![]);
    let controls = preview.projection.controls.clone();
    for id in controls {
        if !ctx
            .read_response(id)
            .is_some_and(|response| response.enabled())
        {
            continue;
        }
        frame(&ctx, &mut preview, SIZE, vec![]);
        ctx.memory_mut(|memory| {
            memory.request_focus_with_filter(
                id,
                egui::EventFilter {
                    escape: true,
                    ..Default::default()
                },
            )
        });
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    key(egui::Key::Enter, egui::Modifiers::COMMAND),
                    key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                    key(egui::Key::Escape, egui::Modifiers::NONE),
                    key(egui::Key::Space, egui::Modifiers::NONE),
                    egui::Event::Text(" ".into()),
                    key(egui::Key::Tab, egui::Modifiers::NONE),
                ],
                ..Default::default()
            },
            |ctx| {
                assert!(preview.projection.control_inputs(ctx, false));
                assert_eq!(ctx.input(|input| input.events.len()), 1);
                assert!(ctx.input(|input| matches!(
                    &input.events[0],
                    egui::Event::Key {
                        key: egui::Key::Tab,
                        ..
                    }
                )));
            },
        );
    }
    assert_eq!(preview.projection.started, 0);
}
#[test]
fn projected_narrow_theme_zoom_virtualizes_and_settles_without_timers() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.4] {
            let source = (0..5000)
                .map(|row| format!("{row:04},hello,world,שלום,end\n"))
                .collect();
            let (ctx, mut preview) = setup(source, false);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            apply(&ctx, &mut preview, "hello", Order::Descending);
            preview.projection.expanded = true;
            let size = Vec2::new(390.0, 560.0);
            frame(
                &ctx,
                &mut preview,
                size,
                vec![key(egui::Key::End, egui::Modifiers::COMMAND)],
            );
            for _ in 0..100 {
                frame(&ctx, &mut preview, size, vec![]);
            }
            assert_eq!(preview.grid.active, (0, 4));
            let out = frame(&ctx, &mut preview, size, vec![]);
            let painted = ctx
                .data(|data| data.get_temp::<usize>(egui::Id::new("delimited-painted-cells")))
                .unwrap();
            assert!(painted < 100);
            let viewport = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
                .unwrap();
            assert!(viewport.height() > 100.0 && ctx.screen_rect().contains_rect(viewport));
            assert!(
                out.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(1)
            );
            assert_eq!(preview.projection.started, 1);
        }
    }
}
