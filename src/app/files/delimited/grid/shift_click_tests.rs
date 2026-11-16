use super::tests::{frame, key, setup};
use super::*;
const SIZE: Vec2 = Vec2::new(620.0, 560.0);
fn click(ctx: &egui::Context, preview: &mut Preview, record: usize, column: usize, shift: bool) {
    let rect = ctx
        .data(|data| {
            data.get_temp::<egui::Rect>(egui::Id::new(("delimited-cell", record, column, false)))
        })
        .unwrap();
    for pressed in [true, false] {
        frame(
            ctx,
            preview,
            SIZE,
            vec![
                egui::Event::PointerMoved(rect.center()),
                egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: if shift {
                        egui::Modifiers::SHIFT
                    } else {
                        egui::Modifiers::NONE
                    },
                },
            ],
        );
    }
}
#[test]
fn shift_click_extends_exact_applied_cells_and_plain_click_resets() {
    let (ctx, mut preview) = setup("h1,h2\na,b\nc,d\n".into(), false);
    click(&ctx, &mut preview, 1, 1, false);
    click(&ctx, &mut preview, 2, 2, true);
    assert_eq!(preview.grid.selection(), Some(((0, 0), (1, 1))));
    let output = frame(&ctx, &mut preview, SIZE, vec![egui::Event::Copy]);
    assert!(output.platform_output.commands.iter().any(
        |command| matches!(command,egui::OutputCommand::CopyText(text) if text=="h1\th2\na\tb\n")
    ));
    click(&ctx, &mut preview, 1, 2, false);
    assert!(preview.grid.selection().is_none());
    assert_eq!(preview.grid.active, (0, 1));
}
#[test]
fn shift_click_keeps_original_keyboard_anchor_and_reverses_rectangle() {
    let (ctx, mut preview) = setup("h1,h2\na,b\nc,d\n".into(), false);
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![
            key(egui::Key::ArrowDown, egui::Modifiers::NONE),
            key(egui::Key::ArrowRight, egui::Modifiers::NONE),
        ],
    );
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::SHIFT)],
    );
    click(&ctx, &mut preview, 1, 1, true);
    assert_eq!(preview.grid.selection(), Some(((0, 0), (1, 1))));
}
#[test]
fn full_app_shift_pointer_phases_chords_new_question_and_foreign_modal_are_isolated() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    let path = app.prefs.cwd.join("shift.csv");
    std::fs::write(&path, "h1,h2\na,b\nc,d\n").unwrap();
    app.files
        .open_target(&ctx, &app.prefs.cwd.clone(), path.to_str().unwrap())
        .unwrap();
    crate::app::files::tests::settle(&mut app.files);
    app.prefs.hide_sidebar = true;
    app.draft = "Kept".into();
    app.focus_composer = false;
    app.state.busy = true;
    for _ in 0..4 {
        crate::app::files::tests::frame(&ctx, &mut app, vec![]);
    }
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.dialogs.push_back(questions::demo_dialog());
    let rect = ctx
        .data(|data| {
            data.get_temp::<egui::Rect>(egui::Id::new(("delimited-cell", 2usize, 2usize, false)))
        })
        .unwrap();
    for pressed in [true, false] {
        crate::app::files::tests::frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(rect.center()),
                egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::SHIFT,
                },
                key(egui::Key::Enter, egui::Modifiers::COMMAND),
                key(egui::Key::Backspace, egui::Modifiers::COMMAND),
            ],
        );
    }
    assert!(commands.try_recv().is_err());
    assert_eq!(app.draft, "Kept");
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.state.busy);
    let Some(Ok(Content::Table(preview))) = &app.files.tabs[0].content else {
        panic!()
    };
    let before = preview.grid.selection();
    assert!(before.is_some());
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        prompt_ui::modal(ctx, egui::Id::new("foreign-shift-table"), 220.0, |ui| {
            prompt_ui::heading(ui, "Information", "Foreign")
        });
    });
    for pressed in [true, false] {
        crate::app::files::tests::frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(rect.center()),
                egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::SHIFT,
                },
            ],
        );
    }
    let Some(Ok(Content::Table(preview))) = &app.files.tabs[0].content else {
        panic!()
    };
    assert_eq!(preview.grid.selection(), before);
    assert_eq!(app.draft, "Kept");
}
#[test]
fn shift_click_sorted_projection_uses_position_not_original_record_span() {
    let (ctx, mut preview) = setup("Name,Note\nBeta,two\nAlpha,one\n".into(), false);
    projection::tests::apply(&ctx, &mut preview, "", projection::Order::Descending);
    let rows = preview.projection.rows(3, false);
    let position = rows.position(1).unwrap();
    preview
        .grid
        .select(&ctx, Path::new("keyboard.csv"), 1, 0, position);
    preview.grid.extend_to(
        &ctx,
        Path::new("keyboard.csv"),
        2,
        1,
        rows.position(2).unwrap(),
    );
    let (first, last) = preview.grid.selection().unwrap();
    assert_eq!(
        export::rectangle::serialize(
            &preview.source,
            preview.index.as_ref().unwrap(),
            rows,
            first,
            last
        )
        .unwrap(),
        "Beta\ttwo\nAlpha\tone\n"
    );
    let before = preview.source.clone();
    assert_eq!(before.as_str(), "Name,Note\nBeta,two\nAlpha,one\n");
}
