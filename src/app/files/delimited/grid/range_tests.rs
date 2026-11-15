use super::tests::{frame, key, setup};
use super::*;
const SIZE: Vec2 = Vec2::new(620.0, 560.0);
#[test]
fn shift_rectangle_copies_applied_unicode_ragged_tsv_without_formula_evaluation() {
    let (ctx, mut preview) = setup("Name,Note\nBeta,=1+2\nAlpha,世界\nshort\n".into(), true);
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
    );
    let output = frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![
            key(egui::Key::ArrowDown, egui::Modifiers::SHIFT),
            key(egui::Key::ArrowRight, egui::Modifiers::SHIFT),
            key(egui::Key::C, egui::Modifiers::COMMAND),
        ],
    );
    assert_eq!(preview.grid.selection(), Some(((1, 0), (2, 1))));
    assert!(output.platform_output.commands.iter().any(|command|matches!(command,egui::OutputCommand::CopyText(text) if text=="Beta\t=1+2\nAlpha\t世界\n")));
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::SHIFT)],
    );
    let output = frame(&ctx, &mut preview, SIZE, vec![egui::Event::Copy]);
    assert!(output.platform_output.commands.iter().any(|command|matches!(command,egui::OutputCommand::CopyText(text) if text.ends_with("short\t\"\"\n"))));
}
#[test]
fn range_escape_plain_navigation_pointer_projection_and_header_clear_anchor() {
    let (ctx, mut preview) = setup("a,b\nc,d\ne,f\n".into(), true);
    for clear in 0..5 {
        frame(
            &ctx,
            &mut preview,
            SIZE,
            vec![key(egui::Key::ArrowDown, egui::Modifiers::SHIFT)],
        );
        assert!(preview.grid.selection().is_some());
        match clear {
            0 => {
                frame(
                    &ctx,
                    &mut preview,
                    SIZE,
                    vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
                );
                assert!(
                    ctx.memory(
                        |memory| memory.has_focus(preview.grid.id(Path::new("keyboard.csv")))
                    )
                );
            }
            1 => {
                frame(
                    &ctx,
                    &mut preview,
                    SIZE,
                    vec![key(egui::Key::ArrowRight, egui::Modifiers::NONE)],
                );
            }
            2 => preview
                .grid
                .select(&ctx, Path::new("keyboard.csv"), 1, 0, 1),
            3 => {
                projection::tests::apply(&ctx, &mut preview, "", projection::Order::Descending);
            }
            _ => {
                preview.set_header(false);
                frame(&ctx, &mut preview, SIZE, vec![]);
            }
        }
        assert!(preview.grid.selection().is_none());
        preview.grid.focus(&ctx, Path::new("keyboard.csv"));
    }
}
#[test]
fn range_copy_freezes_event_time_bounds_before_later_plain_navigation() {
    let (ctx, mut preview) = setup("a,b\nc,d\ne,f\n".into(), false);
    let output = frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![
            key(egui::Key::ArrowRight, egui::Modifiers::SHIFT),
            key(egui::Key::C, egui::Modifiers::COMMAND),
            key(egui::Key::ArrowDown, egui::Modifiers::NONE),
        ],
    );
    assert!(
        output
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command,egui::OutputCommand::CopyText(text) if text=="a\tb\n"))
    );
    assert!(preview.grid.selection().is_none());
    assert_eq!(preview.grid.active, (1, 1));
}
#[test]
fn range_oversize_copy_is_whole_rejected_without_cell_fallback() {
    let (ctx, mut preview) = setup(format!("{}\n", "\t".repeat(512 * 1024 - 2)), false);
    let output = frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![
            key(egui::Key::ArrowRight, egui::Modifiers::SHIFT),
            egui::Event::Copy,
        ],
    );
    assert!(output.platform_output.commands.is_empty());
    assert!(
        preview
            .grid
            .range_notice
            .unwrap()
            .contains("nothing copied")
    );
}
#[test]
fn range_full_app_approval_and_foreign_popup_never_receive_keys() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    let path = app.prefs.cwd.join("range.csv");
    std::fs::write(&path, "a,b\nc,d\n").unwrap();
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
    let Some(Ok(Content::Table(preview))) = &mut app.files.tabs[0].content else {
        panic!()
    };
    preview.grid.focus(&ctx, &path);
    crate::app::files::tests::frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::SHIFT)],
    );
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.dialogs.push_back(questions::demo_dialog());
    crate::app::files::tests::frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
        ],
    );
    assert!(commands.try_recv().is_err());
    assert_eq!(app.state.dialogs.len(), 1);
    assert_eq!(app.draft, "Kept");
}
#[test]
fn maximum_rectangle_virtualization_and_narrow_theme_zoom_remain_bounded() {
    let source = (0..1000)
        .map(|_| "x,".repeat(99) + "x\n")
        .collect::<String>();
    let (ctx, mut preview) = setup(source, false);
    let start = std::time::Instant::now();
    let output = frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![
            key(
                egui::Key::End,
                egui::Modifiers {
                    shift: true,
                    ..egui::Modifiers::COMMAND
                },
            ),
            egui::Event::Copy,
        ],
    );
    assert!(output.platform_output.commands.iter().any(
        |command| matches!(command,egui::OutputCommand::CopyText(text) if text.len()==200000)
    ));
    eprintln!(
        "100000cellrectangle copy/fullframe {:?}; Grid{}B",
        start.elapsed(),
        std::mem::size_of::<Grid>()
    );
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        mode.apply(&ctx);
        for zoom in [0.8, 1.4] {
            ctx.set_zoom_factor(zoom);
            for _ in 0..8 {
                frame(&ctx, &mut preview, Vec2::new(390.0, 560.0), vec![]);
            }
            let painted = ctx
                .data(|data| data.get_temp::<usize>(egui::Id::new("delimited-painted-cells")))
                .unwrap();
            assert!(painted < 100);
            assert_eq!(preview.grid.selection(), Some(((0, 0), (999, 99))));
        }
    }
}
#[test]
fn rectangle_sorted_filtered_order_and_bounds_are_exact() {
    let (ctx, mut preview) = setup("Name,Note\nBeta,two\nAlpha,one\n".into(), true);
    projection::tests::apply(&ctx, &mut preview, "", projection::Order::Ascending);
    let index = preview.index.as_ref().unwrap();
    let rows = preview.projection.rows(index.records.len(), true);
    assert_eq!(
        export::rectangle::serialize(&preview.source, index, rows, (1, 0), (2, 1)).unwrap(),
        "Alpha\tone\nBeta\ttwo\n"
    );
    assert!(export::rectangle::serialize(&preview.source, index, rows, (0, 0), (4, 1)).is_err());
    assert!(export::rectangle::serialize(&preview.source, index, rows, (1, 1), (0, 0)).is_err());
}
