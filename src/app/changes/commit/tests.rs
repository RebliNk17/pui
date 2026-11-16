use super::*;
#[test]
fn native_cancel_commit_signals_worker_discards_late_commit_and_never_retries() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut changes = Changes {
                index_writable: true,
                ..Default::default()
            };
            let (tx, rx) = mpsc::channel();
            let signal = Arc::new(AtomicBool::new(false));
            changes.commit.applying = Some(workspace::read_only::Request::new(rx, signal.clone()));
            let draw = |changes: &mut Changes, events| {
                ctx.run(
                    egui::RawInput {
                        events,
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 620.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        changes.commit_inputs(ctx, true);
                        egui::CentralPanel::default().show(ctx, |ui| {
                            changes.commit_control(ui, &Workspace::default(), false)
                        });
                    },
                )
            };
            for _ in 0..3 {
                draw(&mut changes, vec![]);
            }
            let (_, rect, enabled) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new(
                        "cancel-commit-control",
                    ))
                })
                .unwrap();
            assert!(enabled);
            for pressed in [true, false] {
                draw(
                    &mut changes,
                    vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::PointerButton {
                            pos: rect.center(),
                            pressed,
                            button: egui::PointerButton::Primary,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert!(signal.load(Ordering::Acquire));
            assert!(changes.commit.applying.is_none());
            assert!(
                changes
                    .commit
                    .notice
                    .as_ref()
                    .unwrap()
                    .contains("may already have completed")
            );
            assert!(tx.send(Ok("a".repeat(40))).is_err());
            draw(&mut changes, vec![]);
            assert!(changes.commit.pending.is_none());
            assert!(
                changes
                    .commit
                    .notice
                    .as_ref()
                    .unwrap()
                    .contains("cancellation requested")
            );
        }
    }
}
#[test]
fn hidden_commit_signals_cancel_with_uncertain_completion_notice() {
    let ctx = egui::Context::default();
    let mut changes = Changes {
        index_writable: true,
        ..Default::default()
    };
    let (_tx, rx) = mpsc::channel();
    let signal = Arc::new(AtomicBool::new(false));
    changes.commit.applying = Some(workspace::read_only::Request::new(rx, signal.clone()));
    changes.commit_inputs(&ctx, false);
    assert!(signal.load(Ordering::Acquire));
    assert!(
        changes
            .commit
            .notice
            .as_ref()
            .unwrap()
            .contains("inspect History")
    );
}
#[test]
fn hook_failure_notice_is_literal_themes_zoom_without_capture() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut changes = Changes::default();
            changes.commit.notice = Some(
                "Untrusted local hook output — may contain secrets\n<script>literal</script> 世界"
                    .into(),
            );
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(420.0, 620.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        changes.commit_control(ui, &Workspace::default(), false)
                    });
                },
            );
            assert!(out.platform_output.commands.is_empty());
            assert!(out.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t)if t.galley.job.text.contains("<script>literal</script>"))));
            assert!(changes.commit.applying.is_none());
        }
    }
}
#[test]
fn native_confirm_commit_worker_updates_disposable_branch_once() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let root = std::fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
    crate::workspace::tests::git(&root, &["config", "user.name", "Pui Test"]);
    crate::workspace::tests::git(&root, &["config", "user.email", "pui@example.invalid"]);
    crate::workspace::tests::git(&root, &["add", "--", "README.md"]);
    let snapshot = workspace::commit::prepare(&root, &AtomicBool::new(false)).unwrap();
    assert_eq!(snapshot.files.len(), 1);
    let old = snapshot.head.clone();
    let mut changes = Changes {
        index_writable: true,
        ..Default::default()
    };
    changes.commit.snapshot = Some(snapshot);
    changes.commit.message = "native explicit commit".into();
    let draw = |changes: &mut Changes, events| {
        ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| changes.commit_dialog(ctx),
        )
    };
    for _ in 0..4 {
        draw(&mut changes, vec![]);
    }
    let out = draw(&mut changes, vec![]);
    let pos = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == "Commit without hooks" => {
                Some(t.pos + t.galley.size() / 2.0)
            }
            _ => None,
        })
        .unwrap();
    for pressed in [true, false] {
        draw(
            &mut changes,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(changes.commit.snapshot.is_none());
    let result = changes
        .commit
        .applying
        .take()
        .unwrap()
        .recv_timeout(std::time::Duration::from_secs(8))
        .unwrap()
        .unwrap();
    assert_ne!(result, old);
    assert_eq!(
        String::from_utf8(crate::workspace::tests::git(
            &root,
            &["log", "-1", "--format=%s"]
        ))
        .unwrap()
        .trim(),
        "native explicit commit"
    );
    assert!(changes.commit.message.is_empty());
    assert!(changes.commit.applying.is_none());
}
#[test]
fn native_commit_dialog_escape_themes_zoom_preserves_message_and_never_executes() {
    for (checked, message_checked) in [(false, false), (true, false), (false, true), (true, true)] {
        for signed in [false, true] {
            for mode in [theme::Theme::Light, theme::Theme::Dark] {
                for zoom in [0.8, 1.4] {
                    let ctx = egui::Context::default();
                    theme::install(&ctx);
                    mode.apply(&ctx);
                    ctx.set_zoom_factor(zoom);
                    let root = std::fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
                    crate::workspace::tests::git(&root, &["config", "user.name", "Pui Test"]);
                    crate::workspace::tests::git(
                        &root,
                        &["config", "user.email", "pui@example.invalid"],
                    );
                    crate::workspace::tests::git(&root, &["add", "--", "README.md"]);
                    let snapshot =
                        workspace::commit::prepare(&root, &AtomicBool::new(false)).unwrap();
                    let mut changes = Changes {
                        index_writable: true,
                        ..Default::default()
                    };
                    changes.commit.snapshot = Some(snapshot);
                    changes.commit.message = "draft".into();
                    changes.commit.sign = signed;
                    changes.commit.pre_commit = checked;
                    changes.commit.commit_msg = message_checked;
                    for n in 0..4 {
                        let output = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    Vec2::new(420.0, 620.0),
                                )),
                                events: if n == 3 {
                                    vec![egui::Event::Key {
                                        key: egui::Key::Escape,
                                        pressed: true,
                                        repeat: false,
                                        physical_key: None,
                                        modifiers: egui::Modifiers::NONE,
                                    }]
                                } else {
                                    vec![]
                                },
                                ..Default::default()
                            },
                            |ctx| changes.commit_dialog(ctx),
                        );
                        if n == 2 {
                            assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(t)if t.galley.job.text.contains("Author: Pui Test <pui@example.invalid>")&&t.galley.job.text.contains("Committer: Pui Test <pui@example.invalid>"))));
                        }
                    }
                    assert!(changes.commit.snapshot.is_none());
                    assert!(changes.commit.message.is_empty());
                    assert!(changes.commit.applying.is_none());
                }
            }
        }
    }
}
