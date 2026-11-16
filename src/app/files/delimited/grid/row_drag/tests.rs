use super::super::tests::{frame, key, setup};
use super::*;
const SIZE: Vec2 = Vec2::new(620.0, 560.0);
fn pointer(
    ctx: &egui::Context,
    preview: &mut Preview,
    pos: egui::Pos2,
    button: Option<bool>,
) -> egui::FullOutput {
    let mut events = vec![egui::Event::PointerMoved(pos)];
    if let Some(pressed) = button {
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    frame(ctx, preview, SIZE, events)
}
#[test]
fn gutter_drag_selects_whole_applied_rows_copy_and_reverse_without_cell_text_changes() {
    for header in [false, true] {
        let (ctx, mut preview) = setup("h1,h2\na,b\nc,d\ne,f\n".into(), header);
        let rect = ctx
            .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
            .unwrap();
        let start = rect.min + egui::vec2(20.0, ROW_HEIGHT * 1.5);
        let end = start + egui::vec2(0.0, ROW_HEIGHT);
        pointer(&ctx, &mut preview, start, None);
        pointer(&ctx, &mut preview, start, Some(true));
        pointer(&ctx, &mut preview, end, None);
        pointer(&ctx, &mut preview, end, Some(false));
        let first = usize::from(header);
        assert_eq!(preview.grid.selection(), Some(((first, 0), (first + 1, 1))));
        let output = frame(&ctx, &mut preview, SIZE, vec![egui::Event::Copy]);
        let expected = if header {
            "a\tb\nc\td\n"
        } else {
            "h1\th2\na\tb\n"
        };
        assert!(
            output
                .platform_output
                .commands
                .iter()
                .any(|c| matches!(c,egui::OutputCommand::CopyText(text) if text==expected))
        );
        assert!(preview.grid.row_drag.is_none());
        pointer(&ctx, &mut preview, end, Some(true));
        pointer(&ctx, &mut preview, start, None);
        pointer(&ctx, &mut preview, start, Some(false));
        assert_eq!(preview.grid.selection(), Some(((first, 0), (first + 1, 1))));
    }
}
#[test]
fn sorted_drag_uses_applied_rows_and_stays_virtual_in_themes_and_zoom() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut preview) = setup("Name,Value\nZulu,z\nAlpha,a\nBeta,b\n".into(), true);
            ctx.set_zoom_factor(zoom);
            theme.apply(&ctx);
            projection::tests::apply(&ctx, &mut preview, "", projection::Order::Ascending);
            for _ in 0..3 {
                frame(&ctx, &mut preview, SIZE, vec![]);
            }
            let rect = ctx
                .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
                .unwrap();
            let start = rect.min + egui::vec2(20.0, ROW_HEIGHT * 1.5);
            pointer(&ctx, &mut preview, start, Some(true));
            pointer(
                &ctx,
                &mut preview,
                start + egui::vec2(0.0, ROW_HEIGHT),
                Some(false),
            );
            let output = frame(&ctx, &mut preview, SIZE, vec![egui::Event::Copy]);
            assert!(output.platform_output.commands.iter().any(
                |c| matches!(c,egui::OutputCommand::CopyText(text) if text=="Alpha\ta\nBeta\tb\n")
            ));
            assert_eq!(std::mem::size_of::<Grid>(), 128);
        }
    }
}
#[test]
fn full_app_drag_phases_preserve_question_busy_draft_and_never_send() {
    use crate::app::files::tests as app_test;
    let ctx = egui::Context::default();
    let mut app = app_test::app(&ctx);
    let path = app.prefs.cwd.join("row-drag.csv");
    std::fs::write(&path, "h1,h2\na,b\nc,d\n").unwrap();
    app.files
        .open_target(&ctx, &app.prefs.cwd.clone(), path.to_str().unwrap())
        .unwrap();
    app_test::settle(&mut app.files);
    app.prefs.hide_sidebar = true;
    app.focus_composer = false;
    app.draft = "Keep".into();
    app.state.busy = true;
    for _ in 0..4 {
        app_test::frame(&ctx, &mut app, vec![]);
    }
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let rect = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
        .unwrap();
    let start = rect.min + egui::vec2(20.0, ROW_HEIGHT * 1.5);
    app.state.dialogs.push_back(questions::demo_dialog());
    for (pos, pressed) in [(start, true), (start + egui::vec2(0.0, ROW_HEIGHT), false)] {
        app_test::frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
                key(egui::Key::Enter, egui::Modifiers::COMMAND),
                key(egui::Key::Backspace, egui::Modifiers::COMMAND),
                egui::Event::Text("bad".into()),
                egui::Event::Paste("bad".into()),
            ],
        );
    }
    assert!(commands.try_recv().is_err());
    assert_eq!(app.draft, "Keep");
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.state.busy);
    let Some(Ok(Content::Table(preview))) = &app.files.tabs[0].content else {
        panic!()
    };
    assert!(preview.grid.selection().is_some());
    assert!(preview.grid.row_drag.is_none());
}
#[test]
fn popup_and_lost_focus_cancel_without_retargeting() {
    for popup in [false, true] {
        let (ctx, mut preview) = setup("a,b\nc,d\n".into(), false);
        let rect = ctx
            .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
            .unwrap();
        let start = rect.min + egui::vec2(20.0, ROW_HEIGHT * 1.5);
        pointer(&ctx, &mut preview, start, Some(true));
        assert!(preview.grid.row_drag.is_some());
        let before = preview.grid.selection();
        if popup {
            egui::Popup::open_id(&ctx, egui::Id::new("foreign-row-drag"));
            preview.grid.row_drag_inputs(&ctx, true);
        } else {
            let _ = ctx.run(
                egui::RawInput {
                    focused: false,
                    ..Default::default()
                },
                |ctx| preview.grid.row_drag_inputs(ctx, true),
            );
        }
        assert!(preview.grid.row_drag.is_none());
        assert_eq!(preview.grid.selection(), before);
    }
}
#[test]
fn held_edge_scrolls_both_directions_extends_rows_and_release_stops() {
    for header in [false, true] {
        let source = (0..500)
            .map(|i| format!("row{i},value{i}\n"))
            .collect::<String>();
        let (ctx, mut preview) = setup(source, header);
        let rect = ctx
            .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
            .unwrap();
        let start = rect.min + egui::vec2(20.0, ROW_HEIGHT * 1.5);
        let bottom = egui::pos2(start.x, rect.bottom() - 4.0);
        pointer(&ctx, &mut preview, start, Some(true));
        for _ in 0..24 {
            pointer(&ctx, &mut preview, bottom, None);
        }
        assert!(preview.grid.offset.y > 60.0, "{:?}", preview.grid.offset);
        let selection = preview.grid.selection().unwrap();
        assert_eq!(selection.0.0, usize::from(header));
        assert!(selection.1.0 > 15);
        let down = preview.grid.offset.y;
        let top = egui::pos2(start.x, rect.top() + ROW_HEIGHT + 4.0);
        for _ in 0..12 {
            pointer(&ctx, &mut preview, top, None);
        }
        assert!(preview.grid.offset.y < down);
        pointer(&ctx, &mut preview, top, Some(false));
        for _ in 0..2 {
            pointer(&ctx, &mut preview, top, None);
        }
        let stopped = preview.grid.offset;
        assert!(
            preview
                .grid
                .row_drag_scroll(&ctx, Path::new("keyboard.csv"), header, 500, 2)
                .is_none()
        );
        for _ in 0..8 {
            pointer(&ctx, &mut preview, bottom, None);
        }
        assert_eq!(preview.grid.offset, stopped);
        assert!(preview.grid.row_drag.is_none());
    }
}
#[test]
fn edge_scroll_refuses_outside_release_foreign_popup_and_clamps_step() {
    let (ctx, mut preview) = setup((0..100).map(|i| format!("{i},v\n")).collect(), false);
    let rect = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
        .unwrap();
    let start = rect.min + egui::vec2(20.0, ROW_HEIGHT * 1.5);
    pointer(&ctx, &mut preview, start, Some(true));
    pointer(
        &ctx,
        &mut preview,
        egui::pos2(start.x, rect.bottom() - 2.0),
        None,
    );
    let next = preview
        .grid
        .row_drag_scroll(&ctx, Path::new("keyboard.csv"), false, 100, 2)
        .unwrap();
    assert!(next.y - preview.grid.offset.y <= 24.0);
    let max = (101.0 * ROW_HEIGHT - preview.grid.viewport.y).max(0.0);
    preview.grid.offset.y = max;
    assert!(
        preview
            .grid
            .row_drag_scroll(&ctx, Path::new("keyboard.csv"), false, 100, 2)
            .is_none()
    );
    preview.grid.offset.y = 0.0;
    pointer(
        &ctx,
        &mut preview,
        egui::pos2(start.x, rect.bottom() + 20.0),
        None,
    );
    assert!(
        preview
            .grid
            .row_drag_scroll(&ctx, Path::new("keyboard.csv"), false, 100, 2)
            .is_none()
    );
    egui::Popup::open_id(&ctx, egui::Id::new("foreign-scroll"));
    assert!(
        preview
            .grid
            .row_drag_scroll(&ctx, Path::new("keyboard.csv"), false, 100, 2)
            .is_none()
    );
}
#[test]
fn projection_change_and_escape_cancel_drag() {
    let (ctx, mut preview) = setup("a,b\nc,d\n".into(), false);
    let rect = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("delimited-grid-viewport")))
        .unwrap();
    let start = rect.min + egui::vec2(20.0, ROW_HEIGHT * 1.5);
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
    pointer(&ctx, &mut preview, start, Some(true));
    preview
        .grid
        .projection_changed(&ctx, preview.projection.rows(2, false));
    assert!(preview.grid.row_drag.is_none());
    assert!(preview.grid.selection().is_none());
}
