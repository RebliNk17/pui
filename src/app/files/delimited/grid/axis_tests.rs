use super::tests::{frame, key, setup};
use super::*;
const SIZE: Vec2 = Vec2::new(620.0, 560.0);
#[test]
fn shift_space_selects_active_applied_row_including_missing_ragged_cells() {
    let (ctx, mut preview) = setup("Name,Note,Extra\nshort\nBeta,=1+2,x\n".into(), true);
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
            key(egui::Key::Space, egui::Modifiers::SHIFT),
            egui::Event::Text(" ".into()),
            egui::Event::Copy,
        ],
    );
    assert_eq!(preview.grid.selection(), Some(((1, 0), (1, 2))));
    assert!(output.platform_output.commands.iter().any(|command|matches!(command,egui::OutputCommand::CopyText(text) if text=="short\t\"\"\t\"\"\n")));
}
#[test]
fn ctrl_space_selects_only_current_column_in_applied_order_with_header_policy() {
    for header in [true, false] {
        let (ctx, mut preview) = setup("Name,Note\nBeta,=1+2\nAlpha,世界\n".into(), header);
        projection::tests::apply(&ctx, &mut preview, "Alpha", projection::Order::Ascending);
        preview.grid.focus(&ctx, Path::new("keyboard.csv"));
        frame(
            &ctx,
            &mut preview,
            SIZE,
            vec![key(egui::Key::ArrowRight, egui::Modifiers::NONE)],
        );
        let output = frame(
            &ctx,
            &mut preview,
            SIZE,
            vec![
                key(egui::Key::Space, egui::Modifiers::CTRL),
                egui::Event::Copy,
            ],
        );
        let expected = if header { "Note\n世界\n" } else { "世界\n" };
        assert!(output.platform_output.commands.iter().any(
            |command| matches!(command,egui::OutputCommand::CopyText(text) if text==expected)
        ));
        assert_eq!(preview.grid.selection().unwrap().0.1, 1);
        assert_eq!(preview.grid.selection().unwrap().1.1, 1);
    }
}
#[test]
fn axis_selection_replaces_old_rectangle_and_escape_clears_without_extra_state() {
    let (ctx, mut preview) = setup("a,b,c\nd,e,f\n".into(), false);
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![key(egui::Key::A, egui::Modifiers::COMMAND)],
    );
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![key(egui::Key::Space, egui::Modifiers::SHIFT)],
    );
    assert_eq!(preview.grid.selection(), Some(((1, 0), (1, 2))));
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(preview.grid.selection().is_none());
    assert_eq!(std::mem::size_of::<Grid>(), 128);
}
#[test]
fn axis_maximum_table_narrow_themes_zoom_stays_virtual_and_constant_state() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let source = (0..1000)
                .map(|_| "x,".repeat(99) + "x\n")
                .collect::<String>();
            let (ctx, mut preview) = setup(source, false);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let start = std::time::Instant::now();
            for modifier in [egui::Modifiers::SHIFT, egui::Modifiers::CTRL] {
                frame(
                    &ctx,
                    &mut preview,
                    Vec2::new(390.0, 560.0),
                    vec![key(egui::Key::Space, modifier)],
                );
                for _ in 0..10 {
                    frame(&ctx, &mut preview, Vec2::new(390.0, 560.0), vec![]);
                }
                let painted = ctx
                    .data(|data| data.get_temp::<usize>(egui::Id::new("delimited-painted-cells")))
                    .unwrap();
                assert!(painted < 100);
                assert!(preview.grid.selection().is_some());
            }
            assert_eq!(std::mem::size_of::<Grid>(), 128);
            eprintln!("100kcell axis+20frames {:?}", start.elapsed());
        }
    }
}
#[test]
fn foreign_popup_and_modal_have_first_refusal_over_axis_shortcuts() {
    let (ctx, mut preview) = setup("a,b\nc,d\n".into(), false);
    let popup = egui::Id::new("foreign-axis-popup");
    egui::Popup::open_id(&ctx, popup);
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![key(egui::Key::Space, egui::Modifiers::SHIFT)],
    );
    assert!(preview.grid.selection().is_none());
    egui::Popup::close_all(&ctx);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        prompt_ui::modal(ctx, egui::Id::new("foreign-axis-modal"), 220.0, |ui| {
            prompt_ui::heading(ui, "Information", "Foreign")
        });
    });
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![key(egui::Key::Space, egui::Modifiers::CTRL)],
    );
    assert!(preview.grid.selection().is_none());
}
#[test]
fn axis_shortcuts_do_not_replace_pointer_selected_substring() {
    let (ctx, mut preview) = setup("name,note\nAlpha,chosen letters\n".into(), true);
    let output = frame(&ctx, &mut preview, SIZE, vec![]);
    let text = output
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
    let at = |index| {
        text.pos
            + text
                .galley
                .pos_from_cursor(egui::text::CCursor::new(index))
                .center()
                .to_vec2()
    };
    let start = at(0);
    let end = at(6);
    frame(
        &ctx,
        &mut preview,
        SIZE,
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
        SIZE,
        vec![egui::Event::PointerMoved(end)],
    );
    frame(
        &ctx,
        &mut preview,
        SIZE,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(egui::text_selection::LabelSelectionState::load(&ctx).has_selection());
    for modifier in [egui::Modifiers::SHIFT, egui::Modifiers::CTRL] {
        frame(
            &ctx,
            &mut preview,
            SIZE,
            vec![key(egui::Key::Space, modifier)],
        );
        assert!(preview.grid.selection().is_none());
    }
    let output = frame(&ctx, &mut preview, SIZE, vec![egui::Event::Copy]);
    assert!(
        output
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command,egui::OutputCommand::CopyText(text) if text=="chosen"))
    );
}
#[test]
fn full_app_axis_chords_and_new_question_do_not_send_stop_or_type_in_draft() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    let path = app.prefs.cwd.join("axis.csv");
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
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    for modifier in [egui::Modifiers::SHIFT, egui::Modifiers::CTRL] {
        crate::app::files::tests::frame(
            &ctx,
            &mut app,
            vec![
                key(egui::Key::Space, modifier),
                egui::Event::Text(" ".into()),
            ],
        );
    }
    app.state.dialogs.push_back(questions::demo_dialog());
    crate::app::files::tests::frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Space, egui::Modifiers::SHIFT),
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
        ],
    );
    assert_eq!(app.draft, "Kept");
    assert!(app.state.busy);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(commands.try_recv().is_err());
}
