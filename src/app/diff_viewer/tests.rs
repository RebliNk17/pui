use super::*;

fn input(size: Vec2, events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        events,
        ..Default::default()
    }
}
fn key() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
fn text(output: &egui::FullOutput) -> Vec<&egui::epaint::TextShape> {
    output
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                Some(text)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn embedded_diff_is_bounded_numbered_neutral_and_never_creates_a_floating_window() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for (size, zoom) in [
            (Vec2::new(1200.0, 900.0), 1.0),
            (Vec2::new(420.0, 560.0), 1.25),
        ] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let path = format!("src/{}/tests.rs", "long-directory/".repeat(12));
            let diff = format!(
                "@@ -10,2 +10,3 @@\n before\n-old\n+new\n+{}",
                "long source ".repeat(200)
            );
            for _ in 0..5 {
                let out = ctx.run(input(size, vec![]), |ctx| {
                    assert!(show(ctx, &path, &diff, true));
                });
                assert!(
                    ctx.memory(|m| m.area_rect(egui::Id::new("diff-viewer")))
                        .is_none()
                );
                let labels = text(&out);
                let code = labels
                    .iter()
                    .find(|t| t.galley.job.text.contains("+new"))
                    .unwrap();
                assert!(
                    code.galley.job.text.contains("   10    before")
                        && code.galley.job.text.contains("   11   +new")
                );
                let section = code
                    .galley
                    .job
                    .sections
                    .iter()
                    .find(|s| code.galley.job.text[s.byte_range.clone()].contains("+new"))
                    .unwrap();
                assert_eq!(section.format.color, theme::palette(&ctx).text);
                for label in ["tests.rs", "+2", "−1", "Recorded edits · read-only"] {
                    assert!(
                        labels.iter().any(|t| t.galley.job.text == label),
                        "missing {label:?}: {:?}",
                        labels
                            .iter()
                            .map(|t| &t.galley.job.text)
                            .filter(|t| t.len() < 200)
                            .collect::<Vec<_>>()
                    );
                }
                for id in ["diff-close", "diff-copy"] {
                    let rect = ctx
                        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new(id)))
                        .unwrap();
                    assert!(ctx.screen_rect().contains_rect(rect), "{rect:?} {size:?}");
                }
            }
        }
    }
}

#[test]
fn built_in_changes_pane_resizes_and_expands_only_on_explicit_action() {
    let ctx = egui::Context::default();
    let mut app = files::tests::app(&ctx);
    app.prefs.hide_sidebar = true;
    app.open_diff_review(("large.rs".into(), "+line\n".repeat(12345)).into(), true);
    for _ in 0..4 {
        files::tests::frame(&ctx, &mut app, vec![]);
    }
    let before =
        egui::containers::panel::PanelState::load(&ctx, egui::Id::new("workspace-changes-dock"))
            .unwrap()
            .rect;
    assert!(!app.changes.expanded());
    let corner = before.left_center();
    for (pos, pressed) in [
        (corner, true),
        (corner + egui::vec2(80.0, 0.0), true),
        (corner + egui::vec2(80.0, 0.0), false),
    ] {
        files::tests::frame(
            &ctx,
            &mut app,
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
    let after =
        egui::containers::panel::PanelState::load(&ctx, egui::Id::new("workspace-changes-dock"))
            .unwrap()
            .rect;
    assert!(
        after.width() < before.width() - 40.0,
        "{before:?} {after:?}"
    );
    app.changes.set_expanded(true);
    files::tests::frame(&ctx, &mut app, vec![]);
    assert!(app.changes.expanded());
    app.changes.set_expanded(false);
    app.open_diff_review(("empty.rs".into(), String::new()).into(), false);
    let out = files::tests::frame(&ctx, &mut app, vec![]);
    assert!(
        text(&out)
            .iter()
            .any(|t| t.galley.job.text == "No changes to display.")
    );
}

#[test]
fn copy_preserves_complete_diff_and_back_or_focused_escape_return_to_list() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let size = Vec2::new(1100.0, 850.0);
    let diff = "--- a/file.rs\n+++ b/file.rs\n@@ -1 +1 @@\n-old\n+new\n";
    assert_eq!(counts(diff), (1, 1));
    for _ in 0..3 {
        let _ = ctx.run(input(size, vec![]), |ctx| {
            show(ctx, "src/file.rs", diff, false);
        });
    }
    for (id, back) in [("diff-copy", false), ("diff-close", true)] {
        let pos = ctx
            .data(|d| d.get_temp::<egui::Rect>(egui::Id::new(id)))
            .unwrap()
            .center();
        for pressed in [true, false] {
            let out = ctx.run(
                input(
                    size,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                ),
                |ctx| {
                    assert_eq!(show(ctx, "src/file.rs", diff, false), !back || pressed);
                },
            );
            if !back && !pressed {
                assert!(
                    out.platform_output
                        .commands
                        .iter()
                        .any(|c| matches!(c,egui::OutputCommand::CopyText(t) if t==diff))
                );
            }
        }
    }
    let _ = ctx.run(input(size, vec![key()]), |ctx| {
        assert!(!show(ctx, "src/file.rs", diff, false));
    });
}

#[test]
fn clicking_outputs_opens_builtin_changes_and_focused_escape_never_stops_agent() {
    let ctx = egui::Context::default();
    let mut app = files::tests::app(&ctx);
    app.prefs.hide_sidebar = true;
    app.prefs.hide_inspector = false;
    app.state.busy = true;
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    files::tests::frame(&ctx, &mut app, vec![]);
    let file = app.outputs.first().unwrap().clone();
    let out = files::tests::frame(&ctx, &mut app, vec![]);
    let name = std::path::Path::new(&file.path)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    let pos = files::tests::text(&out, name);
    files::tests::click(&ctx, &mut app, pos);
    assert!(app.inspector_panel);
    assert_eq!(app.changes.review.as_ref().unwrap().0, file.path);
    assert!(!app.changes.expanded());
    let out = files::tests::frame(&ctx, &mut app, vec![]);
    assert!(
        text(&out)
            .iter()
            .any(|t| t.galley.job.text == "Recorded edits · read-only"),
        "{:?}",
        text(&out)
            .iter()
            .map(|t| &t.galley.job.text)
            .filter(|t| t.len() < 200)
            .collect::<Vec<_>>()
    );
    assert!(
        ctx.memory(|m| m.area_rect(egui::Id::new("diff-viewer")))
            .is_none()
    );
    files::tests::frame(&ctx, &mut app, vec![key()]);
    assert!(app.changes.review.is_none());
    assert!(app.inspector_panel && app.state.busy);
    assert!(commands.try_recv().is_err());
}
