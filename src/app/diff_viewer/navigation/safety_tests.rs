use super::*;
use crate::app::files::tests::frame;
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
#[test]
fn ordered_repeated_navigation_and_modified_shortcuts_do_not_retarget() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut document = demo(&ctx);
    document.2.selected = None;
    document.2.target = None;
    document.2.focus(&ctx);
    let mut draw = |events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1100.0, 850.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                show_document(ctx, &mut document, true);
            },
        )
    };
    for _ in 0..4 {
        draw(vec![]);
    }
    draw(vec![
        key(egui::Key::ArrowDown, egui::Modifiers::ALT),
        key(egui::Key::ArrowDown, egui::Modifiers::ALT),
        key(egui::Key::ArrowDown, egui::Modifiers::ALT),
        key(egui::Key::ArrowUp, egui::Modifiers::ALT),
    ]);
    assert_eq!(document.2.selected, Some(1));
    let before = document.2.rows.as_ptr();
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(
                egui::Key::End,
                egui::Modifiers::ALT | egui::Modifiers::COMMAND,
            )],
            ..Default::default()
        },
        |ctx| {
            show_document(ctx, &mut document, true);
        },
    );
    assert_eq!(document.2.selected, Some(1));
    assert_eq!(before, document.2.rows.as_ptr());
}
#[test]
fn keyboard_focused_buttons_activate_with_enter_without_draft_submission() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut document = demo(&ctx);
    document.2.selected = None;
    document.2.target = None;
    for _ in 0..4 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            show_document(ctx, &mut document, true);
        });
    }
    let next = ctx
        .data(|data| data.get_temp::<egui::Id>(egui::Id::new("diff-next-hunk-focus")))
        .unwrap();
    ctx.memory_mut(|memory| memory.request_focus(next));
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
            ..Default::default()
        },
        |ctx| {
            show_document(ctx, &mut document, true);
        },
    );
    assert_eq!(document.2.selected, Some(0));
}
#[test]
fn selection_menu_and_information_modal_keep_navigation_keys_unclaimed() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut document = demo(&ctx);
    document.2.selected = None;
    document.2.target = None;
    for _ in 0..4 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            show_document(ctx, &mut document, true);
        });
    }
    let menu = egui::Id::new("navigation-blocking-menu");
    egui::Popup::open_id(&ctx, menu);
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::End, egui::Modifiers::ALT)],
            ..Default::default()
        },
        |ctx| {
            show_document(ctx, &mut document, true);
        },
    );
    assert!(document.2.selected.is_none());
    egui::Popup::close_id(&ctx, menu);
    for _ in 0..2 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            show_document(ctx, &mut document, true);
            prompt_ui::modal(ctx, egui::Id::new("nav-info"), 320.0, |ui| {
                prompt_ui::heading(ui, "Info", "");
            });
        });
    }
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::ArrowDown, egui::Modifiers::ALT)],
            ..Default::default()
        },
        |ctx| {
            show_document(ctx, &mut document, true);
            prompt_ui::modal(ctx, egui::Id::new("nav-info"), 320.0, |ui| {
                prompt_ui::heading(ui, "Info", "");
            });
        },
    );
    assert!(document.2.selected.is_none());
}
#[test]
fn no_unified_hunks_omit_irrelevant_actions_and_copy_remains_complete() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut doc: Document = (
        "recorded.rs".into(),
        "-old recorded\n+new recorded\n".into(),
    )
        .into();
    for _ in 0..4 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            show_document(ctx, &mut doc, true);
        });
    }
    assert!(
        ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new("diff-next-hunk")))
            .is_none()
    );
    doc.2.focus(&ctx);
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::ArrowDown, egui::Modifiers::ALT)],
            ..Default::default()
        },
        |ctx| {
            show_document(ctx, &mut doc, true);
        },
    );
    assert!(doc.2.selected.is_none());
    assert!(doc.2.rows.is_empty());
    assert!(doc.2.summary().contains("No unified hunks"));
    let pos = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("diff-copy")))
        .unwrap()
        .center();
    let mut copied = false;
    for pressed in [true, false] {
        let output = ctx.run(
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ctx| {
                show_document(ctx, &mut doc, true);
            },
        );
        copied |=
            output.platform_output.commands.iter().any(
                |command| matches!(command,egui::OutputCommand::CopyText(text) if text==&doc.1),
            );
    }
    assert!(copied);
}
#[test]
fn hidden_utility_does_not_consume_settings_or_other_surface_navigation() {
    let ctx = egui::Context::default();
    let mut app = crate::app::files::tests::app(&ctx);
    let document: Document = ("hidden.rs".into(), "@@ -1 +1 @@\n-a\n+b\n".into()).into();
    app.open_diff_review(document, false);
    app.settings_open = true;
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::ArrowDown, egui::Modifiers::ALT)],
    );
    assert!(app.changes.review.as_ref().unwrap().2.selected.is_none());
}
#[test]
fn demo_uses_real_retained_document_and_close_reopen_discards_previous_target() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_diff_hunks: true,
            ..Default::default()
        },
    );
    for _ in 0..5 {
        frame(&ctx, &mut app, vec![]);
    }
    assert_eq!(app.changes.review.as_ref().unwrap().2.selected, Some(2));
    assert!(app.client.is_none());
    let out = frame(&ctx, &mut app, vec![]);
    assert!(
        out.shapes
            .iter()
            .any(|s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text=="Hunk 3 / 5"))
    );
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(app.changes.review.is_none());
    app.open_diff_review(
        (
            "src/hunk-navigation-fixture.rs".into(),
            "@@ -2 +2 @@\n-a\n+b\n".into(),
        )
            .into(),
        true,
    );
    frame(&ctx, &mut app, vec![]);
    assert!(app.changes.review.as_ref().unwrap().2.selected.is_none());
}
