use super::tests::{frame, key, setup};
use super::*;
const SIZE: Vec2 = Vec2::new(620.0, 560.0);
#[test]
fn select_all_command_ctrl_copies_only_applied_view_and_preserves_original_values() {
    for modifier in [egui::Modifiers::COMMAND, egui::Modifiers::CTRL] {
        let (ctx, mut preview) = setup("Name,Note\nBeta,=1+2\nAlpha,世界\n".into(), true);
        projection::tests::apply(&ctx, &mut preview, "Alpha", projection::Order::Descending);
        preview.grid.focus(&ctx, Path::new("keyboard.csv"));
        let output = frame(
            &ctx,
            &mut preview,
            SIZE,
            vec![key(egui::Key::A, modifier), egui::Event::Copy],
        );
        assert_eq!(preview.grid.selection(), Some(((0, 0), (1, 1))));
        assert!(output.platform_output.commands.iter().any(|command|matches!(command,egui::OutputCommand::CopyText(text) if text=="Name\tNote\nAlpha\t世界\n")));
        assert_eq!(
            preview.source.as_str(),
            "Name,Note\nBeta,=1+2\nAlpha,世界\n"
        );
    }
}
#[test]
fn empty_view_and_modified_select_all_do_not_create_ranges() {
    let (ctx, mut preview) = setup("a,b\nc,d\n".into(), false);
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![key(
            egui::Key::A,
            egui::Modifiers {
                shift: true,
                ..egui::Modifiers::COMMAND
            },
        )],
    );
    assert!(preview.grid.selection().is_none());
    projection::tests::apply(&ctx, &mut preview, "no match", projection::Order::Original);
    preview.grid.focus(&ctx, Path::new("keyboard.csv"));
    let output = frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![
            key(egui::Key::A, egui::Modifiers::COMMAND),
            egui::Event::Copy,
        ],
    );
    assert!(preview.grid.selection().is_none());
    assert!(output.platform_output.commands.is_empty());
}
#[test]
fn select_all_maximum_table_is_constant_state_and_virtualized() {
    let source = (0..1000)
        .map(|_| "x,".repeat(99) + "x\n")
        .collect::<String>();
    let (ctx, mut preview) = setup(source, false);
    let start = std::time::Instant::now();
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![key(egui::Key::A, egui::Modifiers::COMMAND)],
    );
    for _ in 0..10 {
        frame(&ctx, &mut preview, SIZE, vec![]);
    }
    assert_eq!(preview.grid.selection(), Some(((0, 0), (999, 99))));
    let painted = ctx
        .data(|data| data.get_temp::<usize>(egui::Id::new("delimited-painted-cells")))
        .unwrap();
    assert!(painted < 100);
    assert_eq!(std::mem::size_of::<Grid>(), 128);
    eprintln!(
        "selectall100000cells+10frames {:?}; constant128Bgrid",
        start.elapsed()
    );
}
#[test]
fn select_all_foreign_modal_and_late_question_do_not_change_draft_or_approve() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    let path = app.prefs.cwd.join("all.csv");
    std::fs::write(&path, "a,b\nc,d\n").unwrap();
    app.files
        .open_target(&ctx, &app.prefs.cwd.clone(), path.to_str().unwrap())
        .unwrap();
    crate::app::files::tests::settle(&mut app.files);
    app.prefs.hide_sidebar = true;
    app.focus_composer = false;
    app.draft = "Kept".into();
    app.state.busy = true;
    for _ in 0..4 {
        crate::app::files::tests::frame(&ctx, &mut app, vec![]);
    }
    let Some(Ok(Content::Table(preview))) = &mut app.files.tabs[0].content else {
        panic!()
    };
    preview.grid.focus(&ctx, &path);
    app.state.dialogs.push_back(questions::demo_dialog());
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    crate::app::files::tests::frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::A, egui::Modifiers::COMMAND),
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
        ],
    );
    assert_eq!(app.draft, "Kept");
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(commands.try_recv().is_err());
    let Some(Ok(Content::Table(preview))) = &app.files.tabs[0].content else {
        panic!()
    };
    let before = preview.grid.selection();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        prompt_ui::modal(ctx, egui::Id::new("foreign-select-all"), 220.0, |ui| {
            prompt_ui::heading(ui, "Information", "Foreign")
        });
    });
    crate::app::files::tests::frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::A, egui::Modifiers::COMMAND)],
    );
    let Some(Ok(Content::Table(preview))) = &app.files.tabs[0].content else {
        panic!()
    };
    assert_eq!(preview.grid.selection(), before);
}
