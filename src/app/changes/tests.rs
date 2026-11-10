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
                        &outputs,
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
        position(&output, "Recorded edits");
        position(&output, "Current Git");
        let path = app.outputs.last().unwrap().path.clone();
        click(&ctx, &mut app, position(&output, &path));
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![]);
        assert!(app.changes.open_rows.contains(&(false, path.clone())));
        assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text.contains("updated implementation"))));
        assert!(
            app.selected_diff.is_none(),
            "Diffs must be inline, not floating windows"
        );
        click(&ctx, &mut app, position(&output, &path));
        assert!(!app.changes.open_rows.contains(&(false, path)));
        let output = frame(&ctx, &mut app, vec![]);
        click(&ctx, &mut app, position(&output, "Current Git"));
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
    click(&ctx, &mut app, position(&output, "Refresh"));
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
    use std::process::Command;
    let root = std::env::temp_dir().join(format!("pui-changes-git-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .current_dir(&root)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "Git fixture failed");
    };
    git(&["init", "--quiet"]);
    std::fs::write(root.join("sample.rs"), "before\n").unwrap();
    git(&["add", "sample.rs"]);
    git(&[
        "-c",
        "user.name=Pui fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.hooksPath=/dev/null",
        "commit",
        "--quiet",
        "-m",
        "fixture",
    ]);
    std::fs::write(root.join("sample.rs"), "after\nnew line\n").unwrap();
    let before = Command::new("git")
        .current_dir(&root)
        .args(["status", "--porcelain"])
        .output()
        .unwrap()
        .stdout;
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.prefs.cwd = root.clone();
    app.workspace = workspace::inspect(&root);
    assert!(app.workspace.is_git);
    assert_eq!(app.workspace.files[0].added, "2");
    assert_eq!(app.workspace.files[0].removed, "1");
    app.inspector_panel = true;
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    click(&ctx, &mut app, position(&output, "Current Git"));
    let output = frame(&ctx, &mut app, vec![]);
    click(&ctx, &mut app, position(&output, "sample.rs"));
    if let Some(diff) = app.changes.diffs.get_mut("sample.rs")
        && let Some(rx) = diff.rx.take()
    {
        diff.content = Some(rx.recv_timeout(Duration::from_secs(5)).unwrap());
    }
    let output = frame(&ctx, &mut app, vec![]);
    assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t)
        if t.galley.job.text.contains("-before") && t.galley.job.text.contains("+after") && t.galley.job.text.contains("    2   +new line"))));
    assert!(app.selected_diff.is_none());
    let after = Command::new("git")
        .current_dir(&root)
        .args(["status", "--porcelain"])
        .output()
        .unwrap()
        .stdout;
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
    assert!(content.contains("No diff available"));
    changes.invalidate();
    assert!(changes.diffs.is_empty());
}
