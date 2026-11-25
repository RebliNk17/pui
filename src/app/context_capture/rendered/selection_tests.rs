use super::tests::{pointer, position, range_in, text};
use super::*;

fn frame(
    ctx: &egui::Context,
    source: &str,
    scoped: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(620.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            context_capture::inputs(ctx);
            egui::CentralPanel::default().show(ctx, |ui| {
                if scoped {
                    markdown::show_reply(ui, source, "reply");
                } else {
                    markdown::show(ui, source);
                }
            });
        },
    )
}
fn open(ctx: &egui::Context, source: &str, needle: &str, chosen: &str) {
    for _ in 0..4 {
        frame(ctx, source, true, vec![]);
    }
    let output = frame(ctx, source, true, vec![]);
    let range = range_in(&text(&output, needle).galley.job.text, chosen);
    let start = position(&output, needle, range.start);
    let end = position(&output, needle, range.end);
    frame(
        ctx,
        source,
        true,
        pointer(start, egui::PointerButton::Primary, true),
    );
    frame(ctx, source, true, vec![egui::Event::PointerMoved(end)]);
    frame(
        ctx,
        source,
        true,
        pointer(end, egui::PointerButton::Primary, false),
    );
    for pressed in [true, false] {
        frame(
            ctx,
            source,
            true,
            pointer(start, egui::PointerButton::Secondary, pressed),
        );
    }
}

#[test]
fn streaming_rendered_replacement_keeps_open_snapshot_and_no_selection_has_no_fallback() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let source = "prefix **chosen text** suffix";
    open(&ctx, source, "prefix chosen text suffix", "chosen text");
    for _ in 0..3 {
        frame(&ctx, "REPLACED paragraph", true, vec![]);
    }
    let target = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        frame(
            &ctx,
            "REPLACED paragraph",
            true,
            pointer(target, egui::PointerButton::Primary, pressed),
        );
    }
    let mut app = crate::app::files::tests::app(&ctx);
    app.draft = "Kept".into();
    app.collect_context_capture();
    let out = app.outgoing_text();
    assert!(out.contains("\nchosen text\n[/Context]"));
    assert!(
        out.starts_with(
            "Kept

[Context:"
        ),
        "draft first, then the quote block"
    );
    assert_eq!(app.draft, "Kept");
    assert!(!out.contains("REPLACED"));
    let output = frame(&ctx, "Unselected text", true, vec![]);
    let target = position(&output, "Unselected text", 5);
    for pressed in [true, false] {
        frame(
            &ctx,
            "Unselected text",
            true,
            pointer(target, egui::PointerButton::Secondary, pressed),
        );
    }
    let target = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        frame(
            &ctx,
            "Unselected text",
            true,
            pointer(target, egui::PointerButton::Primary, pressed),
        );
    }
    let draft = app.outgoing_text();
    app.collect_context_capture();
    assert_eq!(app.outgoing_text(), draft);
}

#[test]
fn cross_paragraph_selection_is_disabled_and_unscoped_markdown_has_no_quote_menu() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let source = "First **paragraph**.\n\nSecond **paragraph**.";
    for _ in 0..4 {
        frame(&ctx, source, true, vec![]);
    }
    let output = frame(&ctx, source, true, vec![]);
    let start = position(&output, "First paragraph.", 2);
    let end = position(&output, "Second paragraph.", 7);
    frame(
        &ctx,
        source,
        true,
        pointer(start, egui::PointerButton::Primary, true),
    );
    frame(&ctx, source, true, vec![egui::Event::PointerMoved(end)]);
    frame(
        &ctx,
        source,
        true,
        pointer(end, egui::PointerButton::Primary, false),
    );
    for pressed in [true, false] {
        frame(
            &ctx,
            source,
            true,
            pointer(start, egui::PointerButton::Secondary, pressed),
        );
    }
    for _ in 0..3 {
        frame(&ctx, source, true, vec![]);
    }
    let target = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        frame(
            &ctx,
            source,
            true,
            pointer(target, egui::PointerButton::Primary, pressed),
        );
    }
    let mut app = crate::app::files::tests::app(&ctx);
    app.draft = "Kept".into();
    app.collect_context_capture();
    assert_eq!(app.draft, "Kept");
    egui::Popup::close_all(&ctx);
    for _ in 0..3 {
        frame(&ctx, "Unscoped tool or thinking", false, vec![]);
    }
    let output = frame(&ctx, "Unscoped tool or thinking", false, vec![]);
    let target = position(&output, "Unscoped tool or thinking", 4);
    for pressed in [true, false] {
        frame(
            &ctx,
            "Unscoped tool or thinking",
            false,
            pointer(target, egui::PointerButton::Secondary, pressed),
        );
    }
    assert!(!egui::Popup::is_any_open(&ctx));
}

#[test]
fn rendered_menu_fits_narrow_zoom_themes_and_settles_without_source_reparse() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let source = "Keep **chosen** text.";
            open(&ctx, source, "Keep chosen text.", "chosen");
            let mut time = ctx.input(|input| input.time);
            let mut draw = || {
                time += 0.016;
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(390.0, 500.0),
                        )),
                        time: Some(time),
                        ..Default::default()
                    },
                    |ctx| {
                        context_capture::inputs(ctx);
                        egui::CentralPanel::default()
                            .show(ctx, |ui| markdown::show_reply(ui, source, "reply"));
                    },
                )
            };
            for _ in 0..100 {
                draw();
            }
            let output = draw();
            let target = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
                .unwrap();
            assert!(ctx.screen_rect().contains_rect(target));
            assert!(
                output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                    > Duration::from_secs(1)
            );
        }
    }
}
