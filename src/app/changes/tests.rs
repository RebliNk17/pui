use super::*;

fn frame(ctx: &egui::Context, app: &mut Pui, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1134.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    )
}
fn click(ctx: &egui::Context, app: &mut Pui, pos: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            ctx,
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}
fn position(output: &egui::FullOutput, needle: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == needle => {
                Some(t.pos + t.galley.size() / 2.0)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing text: {needle}"))
}
fn app(ctx: &egui::Context) -> Pui {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.prefs.hide_sidebar = true;
    app.workspace = Workspace::default();
    app.state.busy = true;
    app.draft = "Keep draft".into();
    app
}

#[test]
fn changes_dock_does_not_grow_across_frames_resizes_or_long_diff_lines() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        mode.apply(&ctx);
        let mut changes = Changes::default();
        let outputs = vec![RecordedFile {
            path: "src/very-long-filename-that-must-not-expand-the-panel.rs".into(),
            diff: format!("+{}", "long line ".repeat(300)),
            ..Default::default()
        }];
        let render = |changes: &mut Changes, width, events| {
            let mut panel_width = 0.0;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(width, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    changes.show(
                        ctx,
                        std::path::Path::new("/fixture"),
                        "chat",
                        &Workspace::default(),
                        (&outputs, 0),
                        false,
                    );
                    panel_width = width - ctx.available_rect().width();
                    egui::CentralPanel::default().show(ctx, |_| {});
                },
            );
            (panel_width, output)
        };
        for width in [1134.0, 1400.0, 860.0, 1134.0] {
            let mut widths = vec![];
            for _ in 0..50 {
                widths.push(render(&mut changes, width, vec![]).0);
            }
            assert!(
                widths[3..].iter().all(|w| (w - widths[3]).abs() < 0.5),
                "Changes dock grows on redraw at {width}: {widths:?}"
            );
            let handle = egui::pos2(width - widths[49], 450.0);
            for (pos, pressed) in [
                (handle, true),
                (handle + egui::vec2(80.0, 0.0), true),
                (handle + egui::vec2(80.0, 0.0), false),
            ] {
                render(
                    &mut changes,
                    width,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            let resized = render(&mut changes, width, vec![]).0;
            for _ in 0..20 {
                assert!(
                    (render(&mut changes, width, vec![]).0 - resized).abs() < 0.5,
                    "manual resize must stay put"
                );
            }
            changes.open_rows.insert((false, outputs[0].path.clone()));
            for _ in 0..20 {
                assert!(
                    (render(&mut changes, width, vec![]).0 - resized).abs() < 0.5,
                    "long diffs scroll, never widen the dock"
                );
            }
            changes.open_rows.clear();
        }
    }
}

#[test]
fn second_toolbar_button_opens_changes_and_inline_recorded_diff_in_both_themes() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.prefs.theme = theme;
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![]);
        // Folder button is the rightmost toolbar action; Changes is immediately left.
        let folder = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Path(p) if p.closed && p.points.len() == 6 => {
                    Some(egui::Rect::from_points(&p.points).center())
                }
                _ => None,
            })
            .unwrap();
        click(&ctx, &mut app, folder - egui::vec2(36.0, 0.0));
        assert!(
            app.inspector_panel,
            "Second toolbar action must open Changes"
        );
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![]);
        position(&output, "Changes");
        assert!(
            ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new("changes-source-recorded")))
                .is_some()
        );
        let git_tab = ctx
            .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("changes-source-git")))
            .expect("Current Git icon rect")
            .center();
        let path = app.outputs.last().unwrap().path.clone();
        click(&ctx, &mut app, position(&output, &path));
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![]);
        assert!(app.changes.open_rows.contains(&(false, path.clone())));
        assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text.contains("updated implementation"))));
        assert!(
            app.changes.review.is_none(),
            "Diffs must be inline, not floating windows"
        );
        click(&ctx, &mut app, position(&output, &path));
        assert!(!app.changes.open_rows.contains(&(false, path)));
        frame(&ctx, &mut app, vec![]);
        click(&ctx, &mut app, git_tab);
        assert!(app.changes.source == Source::Git);
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![]);
        position(
            &output,
            "Not a Git repository. Recorded edits are still available.",
        );
        assert!(app.state.busy);
        assert_eq!(app.draft, "Keep draft");
        assert!(app.demo);
    }
}

#[test]
fn changes_search_expand_refresh_and_file_panel_switching() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.inspector_panel = true;
    frame(&ctx, &mut app, vec![]);
    app.changes.query = "no-match-for-this-path".into();
    let output = frame(&ctx, &mut app, vec![]);
    // The titlebar breadcrumb also says Changes; target the panel heading below it.
    let heading = output
        .shapes
        .iter()
        .rev()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Changes" => {
                Some(text.pos + text.galley.size() / 2.0)
            }
            _ => None,
        })
        .expect("Changes panel heading");
    let expand = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::LineSegment { points, .. }
                if (points[0].x - points[1].x).abs() == 12.0
                    && (points[0].y - points[1].y).abs() == 12.0
                    && (points[0].y.min(points[1].y) + 6.0 - heading.y).abs() < 3.0 =>
            {
                Some((points[0] + points[1].to_vec2()) / 2.0)
            }
            _ => None,
        })
        .expect("Expand control");
    click(&ctx, &mut app, expand);
    assert!(app.changes.expanded);
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    position(&output, "No matching changes.");
    let refresh = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("changes-refresh")))
        .expect("Refresh icon rect")
        .center();
    click(&ctx, &mut app, refresh);
    assert!(app.changes.diffs.is_empty());
    let output = frame(&ctx, &mut app, vec![]);
    let folder = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Path(p) if p.closed && p.points.len() == 6 => {
                Some(egui::Rect::from_points(&p.points).center())
            }
            _ => None,
        })
        .unwrap();
    click(&ctx, &mut app, folder);
    assert!(app.files.is_open());
    assert!(!app.inspector_panel);
    click(&ctx, &mut app, folder - egui::vec2(36.0, 0.0));
    assert!(!app.files.is_open());
    assert!(app.inspector_panel);
    app.state.session_file = Some("another-chat".into());
    frame(&ctx, &mut app, vec![]);
    assert!(app.changes.query.is_empty());
    assert!(app.changes.open_rows.is_empty());
}

#[test]
fn current_git_diff_is_inline_numbered_and_refresh_preserves_repository() {
    let root = workspace::tests::fixture();
    std::fs::write(root.join("README.md"), "after\nnew line\n").unwrap();
    let before = workspace::tests::snapshot(&root);
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.prefs.cwd = root.clone();
    app.workspace = workspace::inspect(&root);
    assert!(app.workspace.is_git);
    app.workspace.files.retain(|file| file.path == "README.md");
    assert_eq!(app.workspace.files[0].added, "2");
    app.inspector_panel = true;
    frame(&ctx, &mut app, vec![]);
    let _output = frame(&ctx, &mut app, vec![]);
    let git_tab = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("changes-source-git")))
        .expect("Current Git icon rect")
        .center();
    click(&ctx, &mut app, git_tab);
    let output = frame(&ctx, &mut app, vec![]);
    click(&ctx, &mut app, position(&output, "README.md"));
    if let Some(diff) = app.changes.diffs.get_mut("README.md")
        && let Some(rx) = diff.rx.take()
    {
        diff.content = Some(rx.recv_timeout(Duration::from_secs(5)).unwrap());
    }
    let output = frame(&ctx, &mut app, vec![]);
    assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t)
        if t.galley.job.text.contains("+after") && t.galley.job.text.contains("    2   +new line"))));
    assert!(app.changes.review.is_none());
    let after = workspace::tests::snapshot(&root);
    assert_eq!(before, after);
}

#[test]
fn git_diff_loads_off_thread_and_session_switch_discards_old_results() {
    let ctx = egui::Context::default();
    let mut changes = Changes::default();
    let root = PathBuf::from("/missing/pui-fixture");
    changes.load_diff(
        &ctx,
        &root,
        workspace::FileChange {
            path: "file.rs".into(),
            status: " M".into(),
            ..Default::default()
        },
    );
    let diff = changes.diffs.get_mut("file.rs").unwrap();
    let content = diff
        .rx
        .take()
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    assert!(content.unwrap_err().contains("Cannot start read-only Git"));
    changes.invalidate();
    assert!(changes.diffs.is_empty());
}

#[test]
fn inline_errors_are_labels_not_numbered_patch_and_disconnection_is_visible() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        mode.apply(&ctx);
        let mut changes = Changes::default();
        changes.bind(std::path::Path::new("/fixture"), "chat");
        changes.source = Source::Git;
        let workspace = Workspace {
            root: "/fixture".into(),
            files: vec![workspace::FileChange {
                path: "file.rs".into(),
                ..Default::default()
            }],
            is_git: true,
            ..Default::default()
        };
        changes.open_rows.insert((true, "file.rs".into()));
        let (tx, rx) = mpsc::channel();
        changes.diffs.insert(
            "file.rs".into(),
            Diff {
                content: None,
                rx: Some(rx.into()),
            },
        );
        tx.send(Err("+not a patch: review failed".into())).unwrap();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            changes.show(
                ctx,
                std::path::Path::new("/fixture"),
                "chat",
                &workspace,
                (&[], 0),
                false,
            );
            egui::CentralPanel::default().show(ctx, |_| {});
        });
        position(&output, "+not a patch: review failed");
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.job.text.contains("1   +not a patch"))));
        let (tx, rx) = mpsc::channel();
        changes.diffs.insert(
            "file.rs".into(),
            Diff {
                content: None,
                rx: Some(rx.into()),
            },
        );
        drop(tx);
        changes.poll_inline_diffs();
        assert!(
            changes.diffs["file.rs"]
                .content
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap_err()
                .contains("without a result")
        );
    }
}

#[test]
fn narrow_changes_heading_keeps_pills_clear_of_icon_controls() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        mode.apply(&ctx);
        let mut changes = Changes::default();
        let workspace = Workspace {
            root: "/fixture/a-rather-long-worktree-name".into(),
            branch: "feature/very-long-branch-name-for-overflow".into(),
            ..Default::default()
        };
        for width in [560.0, 640.0, 900.0] {
            for _ in 0..3 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        changes.show(
                            ctx,
                            std::path::Path::new("/fixture"),
                            "chat",
                            &workspace,
                            (&[], 0),
                            false,
                        );
                        egui::CentralPanel::default().show(ctx, |_| {});
                    },
                );
            }
            let refresh = ctx
                .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("changes-refresh")))
                .expect("Refresh icon rect");
            let (_, branch) = ctx
                .data(|d| d.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("branch-trigger")))
                .expect("Branch trigger rect");
            // The search toggle (26px) sits immediately left of refresh.
            assert!(
                branch.max.x <= refresh.min.x - 26.0,
                "width {width}: branch {branch:?} overlaps icons left of {refresh:?}"
            );
        }
    }
}
