use super::*;
fn prepared() -> (PathBuf, Snapshot) {
    let root = std::fs::canonicalize(workspace::tests::fixture()).unwrap();
    let snapshot = workspace::index_action::prepare(
        &root,
        "README.md",
        " M",
        Action::Stage,
        &AtomicBool::new(false),
    )
    .unwrap();
    (root, snapshot)
}
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
#[test]
fn focused_action_chords_escape_and_text_do_not_reach_busy_chat_question() {
    use crate::app::files::tests as app_test;
    let ctx = egui::Context::default();
    let mut app = app_test::app(&ctx);
    app.demo = false;
    app.inspector_panel = true;
    app.prefs.hide_sidebar = true;
    let owner = app.review_comment_owner();
    app.changes.bind(&app.prefs.cwd.clone(), &owner);
    app.changes.source = Source::Git;
    app.state.busy = true;
    app.draft = "Keep index draft".into();
    app.focus_composer = false;
    app.workspace = Workspace {
        root: app.prefs.cwd.clone(),
        is_git: true,
        branch: "main".into(),
        files: vec![workspace::FileChange {
            path: "file.rs".into(),
            status: " M".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    for _ in 0..4 {
        app_test::frame(&ctx, &mut app, vec![]);
    }
    let (id, _, _) = ctx
        .data(|d| {
            d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new((
                "index-action",
                true,
                "file.rs",
            )))
        })
        .unwrap();
    ctx.memory_mut(|m| m.request_focus(id));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    app.state.dialogs.push_back(questions::demo_dialog());
    let mut enter = key(egui::Key::Enter);
    if let egui::Event::Key { modifiers, .. } = &mut enter {
        *modifiers = egui::Modifiers::COMMAND;
    }
    let mut stop = key(egui::Key::Backspace);
    if let egui::Event::Key { modifiers, .. } = &mut stop {
        *modifiers = egui::Modifiers::COMMAND;
    }
    app_test::frame(
        &ctx,
        &mut app,
        vec![
            enter,
            stop,
            key(egui::Key::Escape),
            egui::Event::Text("bad".into()),
            egui::Event::Paste("bad".into()),
        ],
    );
    assert!(commands.try_recv().is_err());
    assert_eq!(app.draft, "Keep index draft");
    assert!(app.state.busy);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(app.changes.index_actions.pending.is_none());
}
#[test]
fn hunk_choice_pointer_themes_zoom_is_draft_only_until_confirm_and_escape_cancels() {
    for action in [Action::Stage, Action::Unstage] {
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            for zoom in [0.8, 1.4] {
                let (root, mut snapshot) = prepared();
                snapshot.action = action;
                snapshot.status = "MM".into();
                snapshot.preview="diff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-old\n+one\n@@ -20 +20 @@\n-old\n+two\n".into();
                let before = std::fs::read(root.join(".git/index")).unwrap();
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let mut changes = Changes {
                    index_writable: true,
                    ..Default::default()
                };
                changes.identity = (root.clone(), "owner".into());
                changes.index_actions.owner = Some(changes.identity.clone());
                changes.index_actions.inspected = Some(snapshot);
                let draw = |changes: &mut Changes, events| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(620.0, 900.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| {
                            changes.index_inputs(ctx, true);
                            changes.index_dialog(ctx);
                        },
                    )
                };
                for _ in 0..4 {
                    draw(&mut changes, vec![]);
                }
                let (_, rect) = ctx
                    .data(|d| {
                        d.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new(("stage-hunk", 1usize)))
                    })
                    .unwrap();
                for pressed in [true, false] {
                    draw(
                        &mut changes,
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
                assert_eq!(
                    changes.index_actions.inspected.as_ref().unwrap().hunk.0,
                    Some(2)
                );
                assert!(changes.index_actions.applying.is_none());
                let (_, first) = ctx
                    .data(|d| {
                        d.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new(("stage-hunk", 0usize)))
                    })
                    .unwrap();
                for pressed in [true, false] {
                    draw(
                        &mut changes,
                        vec![
                            egui::Event::PointerMoved(first.center()),
                            egui::Event::PointerButton {
                                pos: first.center(),
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    );
                }
                assert_eq!(
                    changes.index_actions.inspected.as_ref().unwrap().hunk.0,
                    Some(3)
                );
                assert_eq!(before, std::fs::read(root.join(".git/index")).unwrap());
                draw(&mut changes, vec![key(egui::Key::Escape)]);
                assert!(changes.index_actions.inspected.is_none());
                assert_eq!(before, std::fs::read(root.join(".git/index")).unwrap());
            }
        }
    }
}
#[test]
fn confirmation_escape_themes_zoom_keeps_index_unchanged_and_owner_hide_cancels() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (root, _) = prepared();
            let snapshot = workspace::index_action::prepare(
                &root,
                "README.md",
                " M",
                Action::Discard,
                &AtomicBool::new(false),
            )
            .unwrap();
            let before = std::fs::read(root.join(".git/index")).unwrap();
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut changes = Changes {
                index_writable: true,
                ..Default::default()
            };
            changes.identity = (root.clone(), "owner".into());
            changes.index_actions.owner = Some(changes.identity.clone());
            let preview_len = snapshot.preview.len();
            changes.index_actions.inspected = Some(snapshot);
            for n in 0..4 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 620.0),
                        )),
                        events: if n == 3 {
                            vec![key(egui::Key::Escape)]
                        } else {
                            vec![]
                        },
                        ..Default::default()
                    },
                    |ctx| {
                        changes.index_inputs(ctx, true);
                        changes.index_dialog(ctx);
                    },
                );
            }
            assert_eq!(
                ctx.data(|d| d.get_temp::<usize>(egui::Id::new("index-preview-shown"))),
                Some(preview_len)
            );
            assert!(changes.index_actions.inspected.is_none());
            assert!(changes.index_actions.applying.is_none());
            assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), before);
        }
    }
}
#[test]
fn native_confirmation_tab_then_enter_applies_only_after_explicit_confirm() {
    let (root, snapshot) = prepared();
    let before = std::fs::read(root.join(".git/index")).unwrap();
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut changes = Changes {
        index_writable: true,
        ..Default::default()
    };
    changes.identity = (root.clone(), "owner".into());
    changes.index_actions.owner = Some(changes.identity.clone());
    changes.index_actions.inspected = Some(snapshot);
    let draw = |changes: &mut Changes, events| {
        ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                changes.index_inputs(ctx, true);
                changes.index_dialog(ctx);
            },
        )
    };
    for _ in 0..3 {
        draw(&mut changes, vec![]);
    }
    assert_eq!(before, std::fs::read(root.join(".git/index")).unwrap());
    let mut tab = key(egui::Key::Tab);
    if let egui::Event::Key { modifiers, .. } = &mut tab {
        *modifiers = egui::Modifiers::SHIFT;
    }
    draw(&mut changes, vec![tab]);
    draw(&mut changes, vec![]);
    draw(&mut changes, vec![key(egui::Key::Enter)]);
    assert!(changes.index_actions.inspected.is_none());
    let result = changes
        .index_actions
        .applying
        .as_ref()
        .expect("explicit confirm dispatch")
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(result.is_ok(), "{result:?}");
    assert_ne!(before, std::fs::read(root.join(".git/index")).unwrap());
}
#[test]
fn native_row_pointer_starts_inspection_not_write_and_hide_cancels_confirmation() {
    let (root, _) = prepared();
    let before = std::fs::read(root.join(".git/index")).unwrap();
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut changes = Changes {
        index_writable: true,
        ..Default::default()
    };
    changes.identity = (root.clone(), "owner".into());
    let workspace = Workspace {
        root: root.clone(),
        is_git: true,
        ..Default::default()
    };
    let file = workspace::FileChange {
        path: "README.md".into(),
        status: " M".into(),
        ..Default::default()
    };
    let draw = |changes: &mut Changes, events| {
        ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                changes.index_inputs(ctx, true);
                egui::CentralPanel::default()
                    .show(ctx, |ui| changes.index_row(ui, &workspace, &file, false));
            },
        )
    };
    for _ in 0..3 {
        draw(&mut changes, vec![]);
    }
    let (_, rect, _) = ctx
        .data(|d| {
            d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new((
                "index-action",
                true,
                "README.md",
            )))
        })
        .unwrap();
    for pressed in [true, false] {
        draw(
            &mut changes,
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
    let result = changes
        .index_actions
        .pending
        .as_ref()
        .unwrap()
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap()
        .unwrap();
    changes.index_actions.pending = None;
    changes.index_actions.inspected = Some(result);
    assert_eq!(before, std::fs::read(root.join(".git/index")).unwrap());
    changes.index_inputs(&ctx, false);
    assert!(changes.index_actions.inspected.is_none());
    assert!(changes.index_actions.applying.is_none());
}
