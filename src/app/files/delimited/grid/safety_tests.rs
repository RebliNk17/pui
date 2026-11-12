use super::tests::{frame, key, setup};
use super::*;
const PATH: &str = "keyboard.csv";

#[test]
fn narrow_theme_zoom_navigation_keeps_virtual_budget_and_settles_without_timer() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.4] {
            let source = (0..5000)
                .map(|row| format!("{row},hello,world,שלום,{},end\n", "long".repeat(12)))
                .collect();
            let (ctx, mut preview) = setup(source, true);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let size = Vec2::new(390.0, 560.0);
            for _ in 0..5 {
                frame(&ctx, &mut preview, size, vec![]);
            }
            frame(
                &ctx,
                &mut preview,
                size,
                vec![key(egui::Key::End, egui::Modifiers::COMMAND)],
            );
            for _ in 0..100 {
                frame(&ctx, &mut preview, size, vec![]);
            }
            assert_eq!(preview.grid.active, (4999, 5));
            let output = frame(&ctx, &mut preview, size, vec![]);
            let painted = ctx
                .data(|data| data.get_temp::<usize>(egui::Id::new("delimited-painted-cells")))
                .unwrap();
            assert!(painted < 100);
            let rect = ctx
                .data(|data| {
                    data.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-active-rect"))
                })
                .unwrap();
            let viewport = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
                .unwrap();
            assert!(
                rect.width() > 100.0 && rect.height() > 20.0 && viewport.contains_rect(rect),
                "{rect:?}, {viewport:?}"
            );
            assert!(ctx.screen_rect().contains_rect(viewport));
            assert!(
                output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                    > Duration::from_secs(1)
            );
            assert_eq!(preview.index.as_ref().unwrap().records.len(), 5000);
        }
    }
}
#[test]
fn pointer_selected_substring_copy_is_not_replaced_with_full_cell() {
    let (ctx, mut preview) = setup("name,note\nAlpha,chosen letters\n".into(), true);
    let size = Vec2::new(620.0, 560.0);
    let out = frame(&ctx, &mut preview, size, vec![]);
    let text = out
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                (text.galley.job.text == "chosen letters").then_some(text)
            } else {
                None
            }
        })
        .unwrap();
    let start = text.pos
        + text
            .galley
            .pos_from_cursor(egui::text::CCursor::new(0))
            .center()
            .to_vec2();
    let end = text.pos
        + text
            .galley
            .pos_from_cursor(egui::text::CCursor::new(6))
            .center()
            .to_vec2();
    frame(
        &ctx,
        &mut preview,
        size,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    frame(
        &ctx,
        &mut preview,
        size,
        vec![egui::Event::PointerMoved(end)],
    );
    frame(
        &ctx,
        &mut preview,
        size,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(egui::text_selection::LabelSelectionState::load(&ctx).has_selection());
    let out = frame(&ctx, &mut preview, size, vec![egui::Event::Copy]);
    assert!(
        out.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(text) if text == "chosen")
        ),
        "{:?}",
        out.platform_output.commands
    );
}
#[test]
fn grid_state_is_constant_size_and_copies_event_time_cell_not_later_navigation() {
    let (ctx, mut preview) = setup("name,note\nAlpha,body\n".into(), true);
    let size = Vec2::new(620.0, 560.0);
    assert!(std::mem::size_of::<Grid>() <= 128);
    let source_ptr = preview.source.as_ptr();
    let cells_ptr = preview.index.as_ref().unwrap().cells.as_ptr();
    let out = frame(
        &ctx,
        &mut preview,
        size,
        vec![
            egui::Event::Copy,
            key(egui::Key::ArrowDown, egui::Modifiers::NONE),
            key(egui::Key::ArrowDown, egui::Modifiers::NONE),
            key(egui::Key::ArrowUp, egui::Modifiers::NONE),
        ],
    );
    assert_eq!(preview.grid.active, (0, 0));
    assert!(
        out.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(text) if text == "name")
        )
    );
    assert_eq!(source_ptr, preview.source.as_ptr());
    assert_eq!(cells_ptr, preview.index.as_ref().unwrap().cells.as_ptr());
    eprintln!(
        "CSV grid state: {} bytes, independent of indexed cells",
        std::mem::size_of::<Grid>()
    );
    let info = preview.grid.description(
        preview.index.as_ref().unwrap(),
        &preview.source,
        preview.header,
    );
    assert!(info.contains("header"));
    preview.header = false;
    assert!(
        !preview
            .grid
            .description(
                preview.index.as_ref().unwrap(),
                &preview.source,
                preview.header
            )
            .contains("header")
    );
    // A new preview of the same path must not reuse another tab/session's focus.
    let replacement = Preview::new("another,value\n".into(), b',');
    assert_ne!(
        preview.grid.id(Path::new(PATH)),
        replacement.grid.id(Path::new(PATH))
    );
}
