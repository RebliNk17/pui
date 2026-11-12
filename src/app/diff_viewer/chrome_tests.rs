use super::*;
fn frame(
    ctx: &egui::Context,
    doc: &mut Document,
    size: Vec2,
    files: &FileNavigation,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(ctx.input(|input| input.time) + 0.1),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                contents_with_files(ui, doc, Some(files));
            });
        },
    )
}
fn files() -> FileNavigation {
    FileNavigation {
        previous: false,
        next: true,
        position: Some((0, 3)),
        status: "Current Git file 1 of 3".into(),
        warning: None,
        git: true,
        loading: false,
    }
}
fn label<'a>(out: &'a egui::FullOutput, value: &str) -> &'a egui::epaint::TextShape {
    out.shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == value => Some(text),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {value}"))
}

#[test]
fn untracked_empty_state_is_neutral_compact_has_one_left_aligned_filename_and_no_dead_hunk_controls()
 {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.4] {
            for width in [320.0, 760.0] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let mut doc: Document = (
                    "src/first.txt".into(),
                    "Untracked file — Git has no diff yet. Pui does not stage or modify files."
                        .into(),
                )
                    .into();
                doc.2.set_untracked(true);
                let mut out = egui::FullOutput::default();
                for _ in 0..6 {
                    out = frame(&ctx, &mut doc, Vec2::new(width, 600.0), &files());
                }
                let title = label(&out, "first.txt");
                let back = ctx
                    .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("diff-close")))
                    .unwrap();
                assert!(
                    (title.pos.x - back.right()).abs() < 16.0,
                    "title not left aligned: {:?} {:?}",
                    title.pos,
                    back
                );
                assert_eq!(out.shapes.iter().filter(|shape|matches!(&shape.shape,egui::Shape::Text(text)if text.galley.job.text=="first.txt")).count(),1);
                label(&out, "Untracked file");
                label(
                    &out,
                    "This file isn’t tracked by Git, so there’s no diff to show.",
                );
                label(&out, "1 / 3");
                for forbidden in [
                    "+0",
                    "−0",
                    "Previous hunk",
                    "Next hunk",
                    "Wrap",
                    "No unified hunks · read-only",
                    doc.1.as_str(),
                ] {
                    assert!(!out.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text)if text.galley.job.text==forbidden)),"irrelevant {forbidden}");
                }
                let heading = ctx
                    .data(|data| {
                        data.get_temp::<egui::Rect>(egui::Id::new("diff-review-file-heading"))
                    })
                    .unwrap();
                assert!(heading.height() <= 28.0);
                let body = ctx
                    .data(|data| {
                        data.get_temp::<egui::Rect>(egui::Id::new("diff-review-code-bounds"))
                    })
                    .unwrap();
                let bg = theme::palette(&ctx).bg;
                assert!(out.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Rect(rect)if rect.fill==bg&&rect.rect.contains(body.center()))));
                for name in ["git-previous-file", "git-next-file"] {
                    let (_, rect, _) = ctx
                        .data(|data| {
                            data.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new(name))
                        })
                        .unwrap();
                    assert!(ctx.screen_rect().contains_rect(rect));
                    assert!((rect.center().y - heading.center().y).abs() < 1.0);
                }
                let copy = ctx
                    .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("diff-copy")))
                    .unwrap();
                assert!(ctx.screen_rect().contains_rect(copy));
                assert!(
                    ctx.screen_rect().right() - copy.right() < 48.0,
                    "actions must stay at trailing edge: {copy:?}"
                );
            }
        }
    }
}

#[test]
fn actual_patch_keeps_hunk_wrap_and_counts_while_no_hunk_recorded_text_stays_selectable() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut doc: Document = ("src/patch.rs".into(), "@@ -1 +1 @@\n-old\n+new\n".into()).into();
    for _ in 0..5 {
        frame(&ctx, &mut doc, Vec2::new(480.0, 600.0), &files());
    }
    let out = frame(&ctx, &mut doc, Vec2::new(480.0, 600.0), &files());
    for text in ["patch.rs", "+1", "−1", "Wrap", "1 hunk"] {
        label(&out, text);
    }
    assert!(
        ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("diff-next-hunk")))
            .is_some()
    );
    let mut recorded: Document = ("recorded.rs".into(), "-old\n+new\n".into()).into();
    recorded.3 = true;
    let out = frame(&ctx, &mut recorded, Vec2::new(480.0, 600.0), &files());
    assert!(out.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text)if text.galley.job.text.contains("+new"))));
}

#[test]
fn untracked_presentation_is_explicit_metadata_never_inferred_from_source_text() {
    let mut doc: Document = (
        "source.rs".into(),
        "Untracked file — Git has no diff yet. Pui does not stage or modify files.".into(),
    )
        .into();
    assert!(!doc.2.untracked());
    doc.2.set_untracked(true);
    assert!(doc.2.untracked());
    let replaced: Document = ("source.rs".into(), doc.1.clone()).into();
    assert!(!replaced.2.untracked());
}
