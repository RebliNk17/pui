use super::*;

const PATH: &str = "keyboard.csv";
pub(in crate::app::files::delimited) fn key(
    key: egui::Key,
    modifiers: egui::Modifiers,
) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        pressed: true,
        repeat: false,
        physical_key: None,
    }
}
pub(in crate::app::files::delimited) fn frame(
    ctx: &egui::Context,
    preview: &mut Preview,
    size: Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let time = ctx.input(|input| input.time) + 0.016;
    ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            preview.grid_inputs(ctx, Path::new(PATH), true);
            egui::CentralPanel::default().show(ctx, |ui| {
                preview.show(ui, Path::new(PATH));
            });
        },
    )
}
pub(in crate::app::files::delimited) fn setup(
    source: String,
    header: bool,
) -> (egui::Context, Preview) {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut preview = Preview::new(source, b',');
    preview.header = header;
    for _ in 0..4 {
        frame(&ctx, &mut preview, Vec2::new(620.0, 560.0), vec![]);
    }
    preview.grid.focus(&ctx, Path::new(PATH));
    for _ in 0..3 {
        frame(&ctx, &mut preview, Vec2::new(620.0, 560.0), vec![]);
    }
    (ctx, preview)
}
#[test]
fn arrows_repeat_and_home_end_navigate_original_records_without_editing_values() {
    let source = "name,note,empty\nAlpha,=1+2,\nBeta,שלום,\n";
    for header in [false, true] {
        let (ctx, mut preview) = setup(source.into(), header);
        let size = Vec2::new(620.0, 560.0);
        frame(
            &ctx,
            &mut preview,
            size,
            vec![
                key(egui::Key::ArrowRight, egui::Modifiers::NONE),
                key(egui::Key::ArrowDown, egui::Modifiers::NONE),
                key(egui::Key::ArrowDown, egui::Modifiers::NONE),
            ],
        );
        assert_eq!(preview.grid.active, (2, 1));
        assert!(ctx.memory(|memory| memory.has_focus(preview.grid.id(Path::new(PATH)))));
        frame(
            &ctx,
            &mut preview,
            size,
            vec![key(egui::Key::End, egui::Modifiers::NONE)],
        );
        assert_eq!(preview.grid.active, (2, 2));
        frame(
            &ctx,
            &mut preview,
            size,
            vec![key(egui::Key::Home, egui::Modifiers::NONE)],
        );
        assert_eq!(preview.grid.active, (2, 0));
        frame(
            &ctx,
            &mut preview,
            size,
            vec![key(egui::Key::Home, egui::Modifiers::COMMAND)],
        );
        assert_eq!(preview.grid.active, (0, 0));
        frame(
            &ctx,
            &mut preview,
            size,
            vec![
                key(egui::Key::ArrowLeft, egui::Modifiers::NONE),
                key(egui::Key::ArrowUp, egui::Modifiers::NONE),
            ],
        );
        assert_eq!(preview.grid.active, (0, 0));
        frame(
            &ctx,
            &mut preview,
            size,
            vec![
                egui::Event::Text("not an editor".into()),
                key(egui::Key::Delete, egui::Modifiers::NONE),
            ],
        );
        assert_eq!(preview.source.as_str(), source);
    }
}
#[test]
fn virtual_grid_reaches_last_cell_and_page_navigation_keeps_active_cell_in_view() {
    let source = (0..500)
        .map(|row| {
            (0..20)
                .map(|col| format!("r{row}c{col}"))
                .collect::<Vec<_>>()
                .join(",")
                + "\n"
        })
        .collect();
    let (ctx, mut preview) = setup(source, true);
    let size = Vec2::new(450.0, 560.0);
    for _ in 0..4 {
        frame(&ctx, &mut preview, size, vec![]);
    }
    frame(
        &ctx,
        &mut preview,
        size,
        vec![key(egui::Key::End, egui::Modifiers::COMMAND)],
    );
    for _ in 0..4 {
        frame(&ctx, &mut preview, size, vec![]);
    }
    assert_eq!(preview.grid.active, (499, 19));
    let rect = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-active-rect")))
        .unwrap();
    let viewport = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
        .unwrap();
    assert!(
        rect.width() >= COLUMN_WIDTH - 2.0
            && rect.height() >= ROW_HEIGHT - 2.0
            && viewport.contains_rect(rect),
        "{rect:?} {viewport:?}"
    );
    frame(
        &ctx,
        &mut preview,
        size,
        vec![key(egui::Key::PageUp, egui::Modifiers::NONE)],
    );
    let row = preview.grid.active.0;
    assert!(row < 499 && row > 470);
    frame(
        &ctx,
        &mut preview,
        size,
        vec![key(egui::Key::PageDown, egui::Modifiers::NONE)],
    );
    assert_eq!(preview.grid.active, (499, 19));
    let output = frame(&ctx, &mut preview, size, vec![egui::Event::Copy]);
    assert!(output.platform_output.commands.iter().any(
        |command| matches!(command, egui::OutputCommand::CopyText(text) if text == "r499c19")
    ));
    let painted = ctx
        .data(|data| data.get_temp::<usize>(egui::Id::new("delimited-painted-cells")))
        .unwrap();
    assert!(painted < 100);
}
#[test]
fn copy_keeps_full_unicode_formula_or_empty_value_and_labels_bound_ragged_missing_cells() {
    let value = format!("=SUM(A1:A2){}", "שלום世界".repeat(5000));
    let (ctx, mut preview) = setup(format!("name,note\nAlpha,{value}\nBeta\n"), true);
    let size = Vec2::new(620.0, 560.0);
    frame(
        &ctx,
        &mut preview,
        size,
        vec![
            key(egui::Key::ArrowDown, egui::Modifiers::NONE),
            key(egui::Key::ArrowRight, egui::Modifiers::NONE),
        ],
    );
    let output = frame(&ctx, &mut preview, size, vec![egui::Event::Copy]);
    assert!(
        output.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(text) if text == &value)
        )
    );
    let description = preview.grid.description(
        preview.index.as_ref().unwrap(),
        &preview.source,
        preview.header,
    );
    assert!(description.chars().count() < 450 && description.contains("label shortened"));
    assert!(output.platform_output.events.iter().all(|event| {
        event
            .widget_info()
            .label
            .as_ref()
            .is_none_or(|label| label.chars().count() < 500)
    }));
    frame(
        &ctx,
        &mut preview,
        size,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
    );
    let description = preview.grid.description(
        preview.index.as_ref().unwrap(),
        &preview.source,
        preview.header,
    );
    assert!(
        description.contains("record 3 of 3, column 2 of 2")
            && description.contains("missing cell")
    );
    let output = frame(&ctx, &mut preview, size, vec![egui::Event::Copy]);
    assert!(
        output.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(text) if text.is_empty())
        )
    );
}
#[test]
fn keyboard_focus_can_enter_by_tab_or_button_and_escape_or_tab_leaves_without_trap() {
    let (ctx, mut preview) = setup("name,note\nAlpha,body\n".into(), true);
    let size = Vec2::new(620.0, 560.0);
    frame(
        &ctx,
        &mut preview,
        size,
        vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
    );
    assert!(!ctx.memory(|memory| memory.has_focus(preview.grid.id(Path::new(PATH)))));
    // Other controls, including scrollbars, remain in egui's Tab sequence.
    for _ in 0..12 {
        frame(
            &ctx,
            &mut preview,
            size,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
        );
        if ctx.memory(|memory| memory.has_focus(preview.grid.id(Path::new(PATH)))) {
            break;
        }
    }
    assert!(ctx.memory(|memory| memory.has_focus(preview.grid.id(Path::new(PATH)))));
    let output = frame(
        &ctx,
        &mut preview,
        size,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!ctx.memory(|memory| memory.has_focus(preview.grid.id(Path::new(PATH)))));
    assert!(output.platform_output.commands.is_empty());
    frame(&ctx, &mut preview, size, vec![]);
    let id = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new(("table-toolbar", 4usize))))
        .unwrap();
    let pos = ctx.read_response(id).unwrap().rect.center();
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut preview,
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
    let output = frame(&ctx, &mut preview, size, vec![]);
    let pos = crate::app::files::tests::text(&output, "Navigate cells");
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut preview,
            size,
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
    assert!(ctx.memory(|memory| memory.has_focus(preview.grid.id(Path::new(PATH)))));
}
#[test]
fn focused_table_summary_emits_truthful_bounded_widget_metadata_on_navigation() {
    let (ctx, mut preview) = setup("name,note\nAlpha,שלום\n".into(), true);
    let size = Vec2::new(620.0, 560.0);
    let output = frame(
        &ctx,
        &mut preview,
        size,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
    );
    let info = output
        .platform_output
        .events
        .iter()
        .map(|event| event.widget_info())
        .find(|info| {
            info.label
                .as_deref()
                .is_some_and(|label| label.contains("Read-only table"))
        })
        .unwrap();
    assert!(info.enabled);
    assert!(
        info.label
            .as_ref()
            .unwrap()
            .contains("record 2 of 2, column 1 of 2 · Alpha")
    );
    assert!(info.current_text_value.is_none());
}
