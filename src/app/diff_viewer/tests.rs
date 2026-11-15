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
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text),
            _ => None,
        })
        .collect()
}

#[test]
fn utility_diff_is_compact_bounded_numbered_and_neutral_in_both_themes() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for (size, zoom) in [
            (Vec2::new(1200.0, 900.0), 1.0),
            (Vec2::new(420.0, 560.0), 1.25),
        ] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            let _ = ctx.run(input(size, vec![]), |_| {});
            ctx.set_zoom_factor(zoom);
            let path = format!("src/{}/tests.rs", "long-directory/".repeat(12));
            let diff = format!(
                "@@ -10,2 +10,3 @@\n before\n-old\n+new\n+{}",
                "long source ".repeat(200)
            );
            let mut widths = vec![];
            for _ in 0..10 {
                let output = ctx.run(input(size, vec![]), |ctx| {
                    assert!(show(ctx, &path, &diff, true));
                });
                let rect = ctx
                    .memory(|memory| memory.area_rect(egui::Id::new("diff-viewer")))
                    .unwrap();
                widths.push(rect.width());
                assert!(
                    rect.width() <= size.x - 20.0,
                    "Window too wide at {size:?}, zoom {zoom}: {rect:?}"
                );
                assert!(rect.height() <= size.y - 20.0, "Window too tall: {rect:?}");
                let labels = text(&output);
                if labels.is_empty() {
                    continue;
                } // First sizing pass is invisible.
                let code = labels
                    .iter()
                    .find(|text| text.galley.job.text.contains("+new"))
                    .unwrap();
                assert!(code.galley.job.text.contains("   10    before"));
                assert!(code.galley.job.text.contains("   11   +new"));
                let source = code
                    .galley
                    .job
                    .sections
                    .iter()
                    .find(|section| {
                        code.galley.job.text[section.byte_range.clone()].contains("+new")
                    })
                    .unwrap();
                assert_eq!(
                    source.format.color,
                    theme::palette(&ctx).text,
                    "Code should not be solid green"
                );
                assert!(labels.iter().any(|text| text.galley.job.text == "tests.rs"));
                assert!(labels.iter().any(|text| text.galley.job.text == "+2"));
                assert!(labels.iter().any(|text| text.galley.job.text == "−1"));
                assert!(
                    labels
                        .iter()
                        .any(|text| text.galley.job.text == "Recorded edits · read-only")
                );
                let filename = labels
                    .iter()
                    .find(|text| text.galley.job.text == "tests.rs")
                    .unwrap();
                assert!(filename.galley.size().y < 24.0, "No oversized title");
                let close = ctx
                    .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("diff-close")))
                    .unwrap();
                let copy = ctx
                    .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("diff-copy")))
                    .unwrap();
                assert!(rect.contains_rect(close) && rect.contains_rect(copy));
                assert!(copy.right() < close.left());
            }
            assert!(
                widths[3..]
                    .iter()
                    .all(|width| (width - widths[3]).abs() < 0.5),
                "Long lines must not grow the window: {widths:?}"
            );
        }
    }
}

#[test]
fn utility_window_remains_resizable_and_handles_large_counts_and_empty_diffs() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let size = Vec2::new(1200.0, 900.0);
    let diff = "+line\n".repeat(12_345);
    for _ in 0..3 {
        let _ = ctx.run(input(size, vec![]), |ctx| {
            show(ctx, "large.rs", &diff, true);
        });
    }
    let before = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("diff-viewer")))
        .unwrap();
    let corner = before.max - Vec2::splat(1.0);
    for (pos, pressed) in [
        (corner, true),
        (corner - egui::vec2(160.0, 100.0), true),
        (corner - egui::vec2(160.0, 100.0), false),
    ] {
        let _ = ctx.run(
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
                show(ctx, "large.rs", &diff, true);
            },
        );
    }
    let resized = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("diff-viewer")))
        .unwrap();
    assert!(
        resized.width() < before.width() - 100.0,
        "Window must remain resizable: {before:?} -> {resized:?}"
    );
    let output = ctx.run(input(size, vec![]), |ctx| {
        show(ctx, "large.rs", &diff, true);
    });
    let labels = text(&output);
    let title = labels
        .iter()
        .find(|text| text.galley.job.text == "large.rs")
        .unwrap();
    let count = labels
        .iter()
        .find(|text| text.galley.job.text == "+12345")
        .unwrap();
    assert!(title.pos.x + title.galley.size().x < count.pos.x);
    let output = ctx.run(input(size, vec![]), |ctx| {
        show(ctx, "empty.rs", "", false);
    });
    assert!(
        text(&output)
            .iter()
            .any(|text| text.galley.job.text == "No changes to display.")
    );
}

#[test]
fn copy_preserves_diff_and_both_close_actions_work() {
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
    let copy = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("diff-copy")))
        .unwrap()
        .center();
    let close = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("diff-close")))
        .unwrap()
        .center();
    for (pos, closing) in [(copy, false), (close, true)] {
        for pressed in [true, false] {
            let output = ctx.run(
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
                    assert_eq!(show(ctx, "src/file.rs", diff, false), !closing || pressed);
                },
            );
            if !closing && !pressed {
                assert!(output.platform_output.commands.iter().any(
                    |command| matches!(command, egui::OutputCommand::CopyText(text) if text == diff)
                ));
            }
        }
    }
    let _ = ctx.run(input(size, vec![key()]), |ctx| {
        assert!(!show(ctx, "src/file.rs", diff, false));
    });
}

#[test]
fn clicking_outputs_opens_numbered_diff_and_escape_never_stops_the_agent() {
    let ctx = egui::Context::default();
    let mut app = files::tests::app(&ctx);
    app.prefs.hide_sidebar = true;
    app.prefs.hide_inspector = false;
    app.state.busy = true;
    let (client, _, commands) = rpc::Client::stub_with_commands();
    app.client = Some(client);
    files::tests::frame(&ctx, &mut app, vec![]);
    let file = app.outputs.first().unwrap().clone();
    let output = files::tests::frame(&ctx, &mut app, vec![]);
    let name = std::path::Path::new(&file.path)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    let row = files::tests::text(&output, name);
    files::tests::click(&ctx, &mut app, row);
    assert_eq!(app.selected_diff.as_ref().unwrap().0, file.path);
    let output = files::tests::frame(&ctx, &mut app, vec![]);
    assert!(
        text(&output)
            .iter()
            .any(|text| text.galley.job.text == "Recorded edits · read-only")
    );
    files::tests::frame(&ctx, &mut app, vec![key()]);
    assert!(app.selected_diff.is_none());
    assert!(app.state.busy);
    app.send(json!({"type":"diff-close-barrier"}));
    loop {
        let command = commands.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_ne!(command["type"], "abort");
        assert_ne!(command["type"], "clear_queue");
        if command["type"] == "diff-close-barrier" {
            break;
        }
    }
}
