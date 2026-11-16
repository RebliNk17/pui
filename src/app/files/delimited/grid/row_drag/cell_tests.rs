use super::super::tests::{frame, key, setup};
use super::*;
const SIZE: Vec2 = Vec2::new(620.0, 560.0);
pub(super) fn pointer(
    ctx: &egui::Context,
    preview: &mut Preview,
    pos: egui::Pos2,
    pressed: Option<bool>,
) {
    let mut events = vec![egui::Event::PointerMoved(pos)];
    if let Some(pressed) = pressed {
        events.push(egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::ALT,
        });
    }
    let time = ctx.input(|i| i.time) + 0.016;
    let _ = ctx.run(
        egui::RawInput {
            time: Some(time),
            modifiers: egui::Modifiers::ALT,
            events,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE)),
            ..Default::default()
        },
        |ctx| {
            preview.grid_inputs(ctx, Path::new("keyboard.csv"), true);
            egui::CentralPanel::default()
                .show(ctx, |ui| preview.show(ui, Path::new("keyboard.csv")));
        },
    );
}
#[test]
fn alt_cell_rectangle_forward_reverse_sorted_unicode_copy_preserves_grid_size() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut preview) = setup(
                "Name,Value,Extra\nZulu,z,Z\nAlpha,世界,A\nBeta,b,B\n".into(),
                true,
            );
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            projection::tests::apply(&ctx, &mut preview, "", projection::Order::Ascending);
            for _ in 0..3 {
                frame(&ctx, &mut preview, SIZE, vec![]);
            }
            let rect = ctx
                .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
                .unwrap();
            let start = rect.min + egui::vec2(NUMBER_WIDTH + COLUMN_WIDTH * 1.5, ROW_HEIGHT * 1.5);
            let end = start + egui::vec2(COLUMN_WIDTH, ROW_HEIGHT);
            for (a, b) in [(start, end), (end, start)] {
                pointer(&ctx, &mut preview, a, Some(true));
                pointer(&ctx, &mut preview, b, None);
                pointer(&ctx, &mut preview, b, Some(false));
                assert_eq!(preview.grid.selection(), Some(((1, 1), (2, 2))));
                let out = frame(&ctx, &mut preview, SIZE, vec![egui::Event::Copy]);
                assert!(
                    out.platform_output.commands.iter().any(
                        |c| matches!(c,egui::OutputCommand::CopyText(t)if t=="世界\tA\nb\tB\n")
                    )
                );
                assert!(preview.grid.row_drag.is_none());
            }
            assert_eq!(std::mem::size_of::<Grid>(), 128);
        }
    }
}
#[test]
fn maximum_encoded_anchor_roundtrips_without_grid_allocation() {
    for row in [0, 9999] {
        for column in [0, 127] {
            assert_eq!(
                decode(CELL_DRAG | (row * 128 + column)),
                (row, Some(column))
            );
        }
    }
    assert_eq!(decode(10000), (9999, None));
}
#[test]
fn escape_and_projection_change_cancel_cell_gesture() {
    let (ctx, mut preview) = setup("a,b,c\nd,e,f\ng,h,i\n".into(), false);
    let rect = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
        .unwrap();
    let start = rect.min + egui::vec2(NUMBER_WIDTH + COLUMN_WIDTH * 0.5, ROW_HEIGHT * 1.5);
    pointer(&ctx, &mut preview, start, Some(true));
    assert!(preview.grid.row_drag.is_some());
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(preview.grid.row_drag.is_none());
    pointer(&ctx, &mut preview, start, Some(false));
    projection::tests::apply(&ctx, &mut preview, "d", projection::Order::Original);
    assert!(preview.grid.row_drag.is_none());
    assert!(preview.grid.selection().is_none());
}
