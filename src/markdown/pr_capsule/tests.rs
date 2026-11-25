use super::*;
use std::collections::BTreeMap;

const URL: &str = "https://github.com/example/repository/pull/220";

fn cache(ctx: &egui::Context, state: &str, checks: &str) {
    let mut statuses = BTreeMap::new();
    statuses.insert(
        URL.to_owned(),
        crate::pulls::Status {
            state: state.into(),
            checks: checks.into(),
            ..Default::default()
        },
    );
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("pull-status-cache"), statuses));
}

fn frame(
    ctx: &egui::Context,
    source: &str,
    width: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.data_mut(|data| data.remove::<Vec<egui::Rect>>(egui::Id::new(("pr-capsule-rects", URL))));
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                super::super::show_reply(ui, source, "capsule-test");
            });
        },
    )
}

fn texts(output: &egui::FullOutput) -> Vec<&egui::epaint::TextShape> {
    fn walk<'a>(shape: &'a egui::Shape, out: &mut Vec<&'a egui::epaint::TextShape>) {
        match shape {
            egui::Shape::Text(text) => out.push(text),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    for shape in &output.shapes {
        walk(&shape.shape, &mut out);
    }
    out
}

#[test]
fn capsules_keep_state_and_ci_colors_independent_in_both_themes() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        mode.apply(&ctx);
        for (state, checks) in [
            ("Merged", "Checks failed"),
            ("Merged", "Checks passed"),
            ("Open", "Checks running"),
            ("Closed", "Checks failed"),
            ("Draft", "Checks unavailable"),
        ] {
            cache(&ctx, state, checks);
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let capsule = Capsule::new(ui, URL).unwrap();
                    let colors = theme::palette(ctx);
                    let expected = match checks {
                        "Checks failed" => colors.red,
                        "Checks passed" => colors.green,
                        "Checks running" => colors.amber,
                        _ => colors.muted,
                    };
                    assert_eq!(capsule.ci_color, expected);
                    if state == "Merged" {
                        assert_eq!(capsule.state_color, colors.accent);
                    }
                    assert!(capsule.tooltip().contains(state));
                    assert!(capsule.tooltip().contains(checks));
                });
            });
        }
    }
}

#[test]
fn capsules_wrap_in_prose_lists_tables_and_rtl_without_clipping_or_losing_copy_text() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.4] {
            for width in [300.0, 760.0] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                cache(&ctx, "Merged", "Checks failed");
                for source in [
                    format!("The fix in [PR **#220**]({URL}) needs a follow-up."),
                    format!("Tracking {URL}, and https://example.com as well."),
                    format!("- The fix in [#220]({URL}) needs a follow-up."),
                    format!("שלום [#220]({URL}) כבר מוזג אבל הבדיקות נכשלו."),
                    format!("| Pull request |\n| --- |\n| [PR #220]({URL}) |"),
                ] {
                    frame(&ctx, &source, width, vec![]);
                    let output = frame(&ctx, &source, width, vec![]);
                    let rects = ctx
                        .data(|data| {
                            data.get_temp::<Vec<egui::Rect>>(egui::Id::new((
                                "pr-capsule-rects",
                                URL,
                            )))
                        })
                        .unwrap();
                    assert_eq!(rects.len(), 1, "{source}: {rects:?}");
                    assert!(
                        ctx.screen_rect().contains_rect(rects[0]),
                        "{source}: {:?}",
                        rects[0]
                    );
                    assert!((rects[0].height() - HEIGHT).abs() < 0.1);
                    let painted = texts(&output);
                    assert!(painted.iter().any(|t| t.galley.job.text == "#220"));
                    assert!(painted.iter().any(|t| t.galley.job.text == "CI failed"));
                    let prose = painted
                        .iter()
                        .find(|t| t.galley.job.text.contains('\u{a0}'))
                        .unwrap();
                    assert!(prose.galley.job.text.contains("#220\u{a0}CI\u{a0}failed"));
                    assert!(prose.galley.rows.iter().all(|r| r.size.y < 30.0));
                }
            }
        }
    }
}

#[test]
fn capsule_find_highlight_keeps_geometry_and_native_copy_keeps_readable_pr_identity() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    cache(&ctx, "Merged", "Checks passed");
    let source = format!("Before [PR #220]({URL}) after.");
    frame(&ctx, &source, 760.0, vec![]);
    let plain = frame(&ctx, &source, 760.0, vec![]);
    let rect = ctx
        .data(|data| data.get_temp::<Vec<egui::Rect>>(egui::Id::new(("pr-capsule-rects", URL))))
        .unwrap()[0];
    let painted = texts(&plain);
    let prose = painted
        .iter()
        .find(|t| t.galley.job.text.starts_with("Before"))
        .unwrap();
    let text = &prose.galley.job.text;
    let start = text[..text.find('#').unwrap()].chars().count();
    let end = text[..text.find(" after").unwrap()].chars().count();
    let position = |index| {
        prose.pos
            + prose
                .galley
                .pos_from_cursor(egui::text::CCursor::new(index))
                .center()
                .to_vec2()
    };
    let (start, end) = (position(start), position(end));
    let pointer = |pos, pressed| {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                pressed,
                button: egui::PointerButton::Primary,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    };
    frame(&ctx, &source, 760.0, pointer(start, true));
    frame(&ctx, &source, 760.0, vec![egui::Event::PointerMoved(end)]);
    frame(&ctx, &source, 760.0, pointer(end, false));
    let copied = frame(&ctx, &source, 760.0, vec![egui::Event::Copy]);
    assert!(copied.platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t.contains("#220") && t.contains("CI\u{a0}passed"))));
    super::super::set_find_highlight(
        &ctx,
        Some(super::super::FindHighlight {
            query: "220".into(),
            active_in_scope: Some(0),
            seen: Arc::new(AtomicUsize::new(0)),
            active_rect: Arc::new(Mutex::new(None)),
        }),
    );
    frame(&ctx, &source, 760.0, vec![]);
    let highlighted = ctx
        .data(|data| data.get_temp::<Vec<egui::Rect>>(egui::Id::new(("pr-capsule-rects", URL))))
        .unwrap()[0];
    assert!(
        (rect.width() - highlighted.width()).abs() <= 1.0,
        "{rect:?} {highlighted:?}"
    );
}

#[test]
fn capsule_opens_on_click_but_not_selection_drag_and_updates_without_reparsing() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let source = format!("Fix [PR #220]({URL}) now.");
    cache(&ctx, "Merged", "Checks running");
    frame(&ctx, &source, 760.0, vec![]);
    let output = frame(&ctx, &source, 760.0, vec![]);
    assert!(
        texts(&output)
            .iter()
            .any(|t| t.galley.job.text == "CI running")
    );
    cache(&ctx, "Merged", "Checks passed");
    let output = frame(&ctx, &source, 760.0, vec![]);
    assert!(
        texts(&output)
            .iter()
            .any(|t| t.galley.job.text == "CI passed")
    );
    let rect = ctx
        .data(|data| data.get_temp::<Vec<egui::Rect>>(egui::Id::new(("pr-capsule-rects", URL))))
        .unwrap()[0];
    let pos = rect.center();
    let pointer = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };
    frame(
        &ctx,
        &source,
        760.0,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    let output = frame(&ctx, &source, 760.0, vec![pointer(pos, false)]);
    assert!(
        output
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::OpenUrl(url) if url.url == URL))
    );
    frame(
        &ctx,
        &source,
        760.0,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    let end = pos + egui::vec2(40.0, 0.0);
    frame(&ctx, &source, 760.0, vec![egui::Event::PointerMoved(end)]);
    let output = frame(&ctx, &source, 760.0, vec![pointer(end, false)]);
    assert!(
        !output
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::OpenUrl(_)))
    );
}
