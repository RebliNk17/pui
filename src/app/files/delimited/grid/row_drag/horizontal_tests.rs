use super::super::tests::{frame, setup};
use super::cell_tests::pointer;
use super::*;
const SIZE: Vec2 = Vec2::new(620.0, 560.0);
#[test]
fn held_cell_edge_scrolls_right_left_preserves_vertical_and_stops_on_release() {
    for header in [false, true] {
        let row = (0..20)
            .map(|n| format!("c{n}"))
            .collect::<Vec<_>>()
            .join(",");
        let (ctx, mut preview) = setup(format!("{row}\n{row}\n{row}\n"), header);
        let rect = ctx
            .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
            .unwrap();
        let start = rect.min + egui::vec2(NUMBER_WIDTH + COLUMN_WIDTH * 0.5, ROW_HEIGHT * 1.5);
        pointer(&ctx, &mut preview, start, Some(true));
        let right = egui::pos2(rect.right() - 5.0, start.y);
        for _ in 0..45 {
            pointer(&ctx, &mut preview, right, None);
        }
        assert!(preview.grid.offset.x > 200.0);
        assert_eq!(preview.grid.offset.y, 0.0);
        let (first, last) = preview.grid.selection().unwrap();
        assert_eq!(first.1, 0);
        assert!(last.1 >= 3);
        let before = preview.grid.offset.x;
        let left = egui::pos2(rect.left() + 5.0, start.y);
        for _ in 0..20 {
            pointer(&ctx, &mut preview, left, None);
        }
        assert!(preview.grid.offset.x < before);
        pointer(&ctx, &mut preview, left, Some(false));
        let stopped = preview.grid.offset;
        for _ in 0..5 {
            frame(&ctx, &mut preview, SIZE, vec![]);
        }
        assert_eq!(preview.grid.offset, stopped);
        assert!(preview.grid.row_drag.is_none());
        assert_eq!(std::mem::size_of::<Grid>(), 128);
    }
}
#[test]
fn horizontal_refuses_popup_and_respects_content_and_step_bounds() {
    let row = (0..128)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let (ctx, mut preview) = setup(format!("{row}\n{row}\n"), false);
    let rect = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
        .unwrap();
    let start = rect.min + egui::vec2(NUMBER_WIDTH + COLUMN_WIDTH * 0.5, ROW_HEIGHT * 1.5);
    pointer(&ctx, &mut preview, start, Some(true));
    let right = egui::pos2(rect.right() - 4.0, start.y);
    pointer(&ctx, &mut preview, right, None);
    let offset = preview
        .grid
        .row_drag_scroll(&ctx, Path::new("keyboard.csv"), false, 2, 128)
        .unwrap();
    assert!(offset.x - preview.grid.offset.x <= 24.0);
    let max = NUMBER_WIDTH + 128.0 * COLUMN_WIDTH - preview.grid.viewport.x;
    preview.grid.offset.x = max;
    assert!(
        preview
            .grid
            .row_drag_scroll(&ctx, Path::new("keyboard.csv"), false, 2, 128)
            .is_none()
    );
    egui::Popup::open_id(&ctx, egui::Id::new("horizontal-foreign"));
    assert!(
        preview
            .grid
            .row_drag_scroll(&ctx, Path::new("keyboard.csv"), false, 2, 128)
            .is_none()
    );
}
