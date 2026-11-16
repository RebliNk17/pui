use super::*;
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
    for checked in [false, true] {
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
