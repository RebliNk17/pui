use super::tests::{DIFF, PATH, approval, key, pointer, setup};
use super::*;
use crate::app::files::tests::frame;

#[test]
fn source_reverse_tab_immediate_escape_uses_native_wrap_owner() {
    let (ctx, mut app) = setup(false, DIFF);
    let commands = approval(&mut app);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let navigation = super::tests::nav(&app, false);
    let source = navigation.selection_response(&ctx).unwrap();
    let owner = navigation.focus_id();
    let wrap = super::tests::control(&ctx, "diff-wrap-lines-focus");
    let pos = source.interact_rect.center();
    for pressed in [true, false] {
        frame(&ctx, &mut app, pointer(pos, pressed));
    }
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(owner));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)],
    );
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(ctx.memory(|memory| memory.focus_lost_by_escape(wrap)));
    assert!(app.changes.open_rows.is_empty());
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.pending_dialog.is_none());
    assert!(commands.try_recv().is_err());
    assert!(app.state.busy);
    assert_eq!(app.draft, "Kept draft");
}

#[test]
fn visible_recorded_header_beyond_512_keeps_native_escape_and_approval_isolation() {
    let (ctx, mut app) = setup(false, DIFF);
    app.changes.open_rows.clear();
    app.outputs = (0..600)
        .map(|index| RecordedFile {
            path: format!("file-{index:03}.rs"),
            diff: DIFF.into(),
            ..Default::default()
        })
        .collect();
    app.output_revision = app.state.outputs_revision;
    let commands = approval(&mut app);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let target = app.changes.inline_header_id(false, "file-000.rs");
    let header = ctx.read_response(target).unwrap();
    assert!(header.rect.top() > 512.0 * 42.0);
    let visible = ctx
        .read_response(app.changes.inline_header_id(false, "file-599.rs"))
        .unwrap()
        .interact_rect;
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(visible.center()),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -30_000.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    // Mouse-wheel deltas are smoothed by egui across native passes.
    for _ in 0..32 {
        frame(&ctx, &mut app, vec![]);
    }
    let header = ctx.read_response(target).unwrap();
    assert!(
        header.enabled() && header.interact_rect.is_positive(),
        "{header:?}"
    );
    assert!(header.interact_rect.bottom() <= 900.0, "{header:?}");
    // Direct first focus uses the same native Escape metadata as deferred Tab.
    // No hidden-control index or 512-header prefix is allowed to own this event.
    ctx.memory_mut(|memory| memory.request_focus(target));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(ctx.memory(|memory| memory.focus_lost_by_escape(target)));
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.pending_dialog.is_none());
    assert!(commands.try_recv().is_err());
    assert!(app.state.busy);
    assert_eq!(app.draft, "Kept draft");
    assert!(app.changes.open_rows.is_empty());
}

#[test]
fn actual_code_pointer_press_release_cannot_answer_underlying_approval() {
    for phase in [true, false] {
        for chord in [egui::Key::Enter, egui::Key::Backspace] {
            let (ctx, mut app) = setup(false, DIFF);
            let commands = approval(&mut app);
            for _ in 0..3 {
                frame(&ctx, &mut app, vec![]);
            }
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
            let id = app.changes.inline.rows[&(false, PATH.into())]
                .navigation
                .as_ref()
                .unwrap()
                .selection_response(&ctx)
                .unwrap();
            let pos = id.interact_rect.center();
            for pressed in [true, false] {
                let mut events = pointer(pos, pressed);
                if pressed == phase {
                    events.push(key(chord, egui::Modifiers::COMMAND));
                }
                frame(&ctx, &mut app, events);
            }
            assert_eq!(app.state.dialogs.len(), 1);
            assert!(app.pending_dialog.is_none());
            assert!(commands.try_recv().is_err());
            assert_eq!(app.draft, "Kept draft");
            assert!(app.state.busy);
        }
    }
}
#[test]
fn open_rows_reverse_boundary_closes_exact_native_predecessor_not_path_sorted_departure() {
    let (ctx, mut app) = setup(false, DIFF);
    app.outputs = vec![
        RecordedFile {
            path: "a.rs".into(),
            diff: DIFF.into(),
            ..Default::default()
        },
        RecordedFile {
            path: "z.rs".into(),
            diff: DIFF.into(),
            ..Default::default()
        },
    ];
    app.output_revision = app.state.outputs_revision;
    app.changes.open_rows =
        std::collections::BTreeSet::from([(false, "a.rs".into()), (false, "z.rs".into())]);
    let commands = approval(&mut app);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let a = app.changes.inline_header_id(false, "a.rs");
    ctx.memory_mut(|m| m.request_focus(a));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)],
    );
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!app.changes.open_rows.contains(&(false, "z.rs".into())));
    assert!(app.changes.open_rows.contains(&(false, "a.rs".into())));
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(commands.try_recv().is_err());
    assert!(app.state.busy);
    assert_eq!(app.draft, "Kept draft");
}

#[test]
fn two_closed_headers_reverse_tab_immediate_escape_retains_approval_and_busy_run() {
    for approve in [false, true] {
        let (ctx, mut app) = setup(false, DIFF);
        app.changes.open_rows.clear();
        app.outputs = vec![
            RecordedFile {
                path: "a-first.rs".into(),
                diff: DIFF.into(),
                ..Default::default()
            },
            RecordedFile {
                path: "z-second.rs".into(),
                diff: DIFF.into(),
                ..Default::default()
            },
        ];
        app.output_revision = app.state.outputs_revision;
        let commands = approval(&mut app);
        if !approve {
            app.state.dialogs.clear();
        }
        for _ in 0..5 {
            frame(&ctx, &mut app, vec![]);
        }
        let a = app.changes.inline_header_id(false, "a-first.rs");
        let z = app.changes.inline_header_id(false, "z-second.rs");
        assert!(
            ctx.read_response(z).unwrap().rect.top() < ctx.read_response(a).unwrap().rect.top()
        );
        ctx.memory_mut(|memory| memory.request_focus(a));
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Tab, egui::Modifiers::SHIFT)],
        );
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(app.changes.open_rows.is_empty());
        assert!(app.state.busy);
        assert_eq!(app.state.dialogs.len(), usize::from(approve));
        assert!(commands.try_recv().is_err());
        assert_eq!(app.draft, "Kept draft");
    }
}
