use super::*;
#[test]
fn mode_lists_filter_navigation_and_match_exact_applied_statuses() {
    let workspace = Workspace {
        root: "/repo".into(),
        is_git: true,
        files: vec![
            workspace::FileChange {
                path: "a".into(),
                status: "MM".into(),
                ..Default::default()
            },
            workspace::FileChange {
                path: "b".into(),
                status: " M".into(),
                ..Default::default()
            },
            workspace::FileChange {
                path: "c".into(),
                status: "M ".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let staged = List::new_mode(&workspace, workspace::DiffMode::Staged).unwrap();
    assert_eq!(
        staged
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "c"]
    );
    let unstaged = List::new_mode(&workspace, workspace::DiffMode::Unstaged).unwrap();
    assert_eq!(
        unstaged
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert!(staged.matches(&workspace));
    let mut changed = workspace;
    changed.files[0].status = " M".into();
    assert!(!staged.matches(&changed));
}
#[test]
fn native_review_mode_pointer_themes_zoom_keeps_old_body_and_empty_mode_truthful() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let workspace = Workspace {
                root: "/unused".into(),
                is_git: true,
                files: vec![workspace::FileChange {
                    path: "x".into(),
                    status: " M".into(),
                    ..Default::default()
                }],
                ..Default::default()
            };
            let mut changes = Changes::default();
            changes.open_review(
                &ctx,
                ("x".into(), "original".into()).into(),
                false,
                Path::new("/unused"),
                "owner",
            );
            changes.bind_git_files(&workspace);
            let draw = |changes: &mut Changes, events| {
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
                        changes.git_mode_inputs(ctx, true);
                        egui::CentralPanel::default()
                            .show(ctx, |ui| changes.review_mode_controls(ui, &workspace));
                    },
                )
            };
            for _ in 0..3 {
                draw(&mut changes, vec![]);
            }
            let (_, rect) = ctx
                .data(|d| {
                    d.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new((
                        "review-diff-mode",
                        "Staged",
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
            assert!(changes.git_files.mode == workspace::DiffMode::Combined);
            assert!(changes.git_files.pending.is_none());
            assert!(
                changes
                    .git_files
                    .error
                    .as_deref()
                    .unwrap()
                    .contains("No files")
            );
            assert_eq!(changes.review.as_ref().unwrap().1, "original");
        }
    }
}
#[test]
fn real_mode_replacement_retains_old_body_until_success_and_filters_files() {
    let root = std::fs::canonicalize(workspace::tests::fixture()).unwrap();
    let cancel = AtomicBool::new(false);
    let stage = workspace::index_action::prepare(
        &root,
        "README.md",
        " M",
        workspace::index_action::Action::Stage,
        &cancel,
    )
    .unwrap();
    workspace::index_action::apply(stage, &cancel).unwrap();
    std::fs::write(root.join("README.md"), "new unstaged layer\n").unwrap();
    let workspace = workspace::read(&root, &cancel);
    let ctx = egui::Context::default();
    let mut changes = Changes::default();
    changes.open_review(
        &ctx,
        ("README.md".into(), "retained body".into()).into(),
        false,
        &root,
        "owner",
    );
    changes.bind_git_files(&workspace);
    changes.choose_review_mode(&ctx, &workspace, workspace::DiffMode::Staged);
    assert_eq!(changes.review.as_ref().unwrap().1, "retained body");
    let pending = changes.git_files.pending.as_ref().unwrap();
    let loaded = pending
        .rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let (tx, rx) = mpsc::channel();
    tx.send(loaded).ok();
    changes.git_files.pending.as_mut().unwrap().rx = rx.into();
    changes.poll_git_files(&ctx, &workspace);
    assert!(changes.git_files.mode == workspace::DiffMode::Staged);
    assert!(
        changes
            .review
            .as_ref()
            .unwrap()
            .1
            .contains("+fixture modified README")
    );
    assert!(
        !changes
            .review
            .as_ref()
            .unwrap()
            .1
            .contains("new unstaged layer")
    );
    assert!(
        changes
            .git_files
            .list
            .as_ref()
            .unwrap()
            .files
            .iter()
            .all(|f| includes(workspace::DiffMode::Staged, f))
    );
}
