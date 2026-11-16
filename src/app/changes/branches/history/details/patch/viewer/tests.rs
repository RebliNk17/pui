use super::*;
#[test]
fn hunk_index_is_bounded_and_controls_are_bound_to_patch_generation() {
    let source =
        "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n@@ -20 +20 @@\n-c\n+d\n";
    let first = Viewer::new(source);
    assert_eq!(first.hunks, vec![3, 6]);
    let second = Viewer::new(source);
    assert_ne!(first.identity, second.identity);
    let max = Viewer::new(&"@@ -1 +1 @@\n".repeat(1024));
    assert_eq!(max.hunks.len(), 1024);
    assert!(max.hunks.capacity() <= 2048);
}
#[test]
fn pointer_hunks_themes_zoom_boundaries_and_replaced_patch_refuses_old_key() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let text = "@@ -1 +1 @@\n-a\n+b\n@@ -20 +20 @@\n-c\n+d\n";
            let mut viewer = Viewer::new(text);
            let draw = |viewer: &mut Viewer, events: Vec<egui::Event>| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 620.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| viewer.show(ui, text));
                    },
                )
            };
            for _ in 0..3 {
                draw(&mut viewer, vec![]);
            }
            for expected in [0, 1] {
                let (_, rect, enabled) = ctx
                    .data(|d| {
                        d.get_temp::<(egui::Id, egui::Rect, bool)>(
                            popup_id().with(("commit-patch-hunk", 1usize)),
                        )
                    })
                    .unwrap();
                assert!(enabled);
                for pressed in [true, false] {
                    draw(
                        &mut viewer,
                        vec![
                            egui::Event::PointerMoved(rect.center()),
                            egui::Event::PointerButton {
                                pos: rect.center(),
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    );
                }
                assert_eq!(viewer.selected, Some(expected));
            }
            draw(&mut viewer, vec![]);
            assert!(
                !ctx.data(|d| d.get_temp::<(egui::Id, egui::Rect, bool)>(
                    popup_id().with(("commit-patch-hunk", 1usize))
                ))
                .unwrap()
                .2
            );
            let old = viewer.controls[0];
            viewer = Viewer::new(text);
            viewer.key = old;
            draw(&mut viewer, vec![]);
            assert_eq!(viewer.selected, None);
            let fold = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect, bool)>(
                        popup_id().with(("commit-patch-fold", 0usize)),
                    )
                })
                .unwrap();
            for pressed in [true, false] {
                draw(
                    &mut viewer,
                    vec![
                        egui::Event::PointerMoved(fold.1.center()),
                        egui::Event::PointerButton {
                            pos: fold.1.center(),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert!(viewer.folded[0]);
        }
    }
}
#[test]
fn folds_are_independent_navigation_reveals_and_new_result_resets() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let text =
        "@@ -1 +1 @@\n-a\n+b\n@@ -20 +20 @@\n-c\n+d\ndiff --git a/y b/y\nBinary files differ\n";
    let mut viewer = Viewer::new(text);
    let draw = |v: &mut Viewer| {
        ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| v.show(ui, text));
        })
    };
    draw(&mut viewer);
    let old = viewer.fold_controls[0];
    viewer.key = Some(old);
    draw(&mut viewer);
    assert_eq!(viewer.folded, vec![true, false]);
    viewer.key = viewer.controls[1];
    draw(&mut viewer);
    assert_eq!(viewer.folded, vec![false, false]);
    assert_eq!(viewer.selected, Some(0));
    viewer.key = viewer.fold_controls.get(1).copied();
    draw(&mut viewer);
    assert_eq!(viewer.folded, vec![false, true]);
    viewer = Viewer::new(text);
    viewer.key = Some(old);
    draw(&mut viewer);
    assert_eq!(viewer.folded, vec![false, false]);
    assert_eq!(text.lines().count(), 8);
}
#[test]
fn native_popup_hunk_enter_scrolls_and_does_not_dispatch_or_answer() {
    use super::super::super::super::tests::{fixture, key};
    use crate::app::files::tests::frame;
    let (ctx, mut app) = fixture();
    app.changes
        .branches
        .history
        .open_detail(&ctx, &"a".repeat(40), true);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let load = ctx
        .data(|d| d.get_temp::<(egui::Id, egui::Rect)>(popup_id().with("commit-patch")))
        .unwrap()
        .0;
    ctx.memory_mut(|m| m.request_focus(load));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Space, egui::Modifiers::NONE)],
    );
    frame(&ctx, &mut app, vec![]);
    let next = ctx
        .data(|d| {
            d.get_temp::<(egui::Id, egui::Rect, bool)>(
                popup_id().with(("commit-patch-hunk", 1usize)),
            )
        })
        .unwrap()
        .0;
    ctx.memory_mut(|m| m.request_focus(next));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.dialogs.push_back(questions::demo_dialog());
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Enter, egui::Modifiers::NONE),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
        ],
    );
    assert_eq!(
        app.changes.branches.history.detail.patch.viewer.selected,
        Some(0)
    );
    assert_eq!(
        ctx.data(|d| d.get_temp::<usize>(popup_id().with("commit-patch-hunk-target"))),
        Some(3)
    );
    assert!(commands.try_recv().is_err());
    assert_eq!(app.draft, "Kept draft");
    assert_eq!(app.state.dialogs.len(), 1);
    let fold = app
        .changes
        .branches
        .history
        .detail
        .patch
        .viewer
        .fold_controls[0];
    ctx.memory_mut(|m| m.request_focus(fold));
    let mut release = key(egui::Key::Enter, egui::Modifiers::NONE);
    if let egui::Event::Key { pressed, .. } = &mut release {
        *pressed = false;
    }
    frame(&ctx, &mut app, vec![release]);
    frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Enter, egui::Modifiers::NONE),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
        ],
    );
    assert!(app.changes.branches.history.detail.patch.viewer.folded[0]);
    assert!(commands.try_recv().is_err());
    assert_eq!(app.draft, "Kept draft");
    assert_eq!(app.state.dialogs.len(), 1);
}
