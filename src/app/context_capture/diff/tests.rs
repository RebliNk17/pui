use super::*;

pub(super) fn display(diff: &str) -> String {
    changes::lines::rows(diff)
        .map(|row| {
            if row.header {
                "        …\n".into()
            } else {
                format!("{}{}\n", row.gutter(), row.text)
            }
        })
        .collect()
}
pub(super) fn range(display: &str, from: &str, to: &str) -> Range<usize> {
    let start = display.find(from).unwrap();
    let end = display.find(to).unwrap() + to.len();
    display[..start].chars().count()..display[..end].chars().count()
}
#[test]
fn selected_diff_omits_synthetic_gutters_preserves_markers_and_maps_old_new_lines() {
    let diff = "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -10,2 +20,3 @@ fn item()\n before\n-old 世界\n+new e\u{301}\n+another\n";
    let rendered = display(diff);
    let captured = selected(
        diff,
        range(&rendered, "-old", "+another"),
        "src/lib.rs",
        true,
    )
    .unwrap();
    assert_eq!(captured.text, "-old 世界\n+new e\u{301}\n+another");
    assert_eq!(
        captured.label,
        "src/lib.rs · Recorded edits (not net Git) · diff rows 5–7 · old 11–11 · new 21–22"
    );
    let captured = selected(diff, range(&rendered, "old", "世界"), "src/lib.rs", false).unwrap();
    assert_eq!(captured.text, "old 世界");
    assert!(captured.label.contains("Working tree vs HEAD"));
    assert!(!captured.text.contains("11"));
}

#[test]
fn header_cross_hunk_empty_and_outside_ranges_are_not_mislabeled_as_source() {
    let diff = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n@@ -20 +20 @@\n-next\n+last\n";
    let rendered = display(diff);
    let multi = selected(diff, range(&rendered, "-old", "+last"), "a", false).unwrap();
    assert_eq!(multi.text, "-old\n+new\n@@ -20 +20 @@\n-next\n+last");
    assert!(multi.label.contains("2 hunks"));
    assert!(multi.label.contains("discontinuous"));
    assert!(selected(diff, range(&rendered, "diff --git", "a/a"), "a", false).is_none());
    assert!(selected(diff, range(&rendered, "+++ b/a", "+++ b/a"), "a", false).is_none());
    assert!(selected(diff, range(&rendered, "…", "-old"), "a", false).is_none());
    assert!(selected(diff, 0..0, "a", false).is_none());
    assert!(selected(diff, 0..10_000, "a", false).is_none());
    let start = rendered.find("    1   -old").unwrap();
    assert!(selected(diff, start..start + 5, "a", false).is_none());
    let diff = "@@ -0,0 +1,2 @@\n+one\n+two\n\\ No newline at end of file\n";
    let rendered = display(diff);
    let captured = selected(diff, range(&rendered, "+one", "+two"), "created", false).unwrap();
    assert!(captured.label.ends_with("new 1–2"));
    assert!(!captured.label.contains("old "));
    assert!(selected(diff, range(&rendered, "+one", "file"), "created", false).is_none());
}

#[test]
fn cross_hunk_unicode_partial_endpoints_three_hunks_and_caption_only_refuse() {
    let diff = "@@ -1 +1 @@\n-old 世界\n+new 界\n@@ -20 +20 @@\n-next\n+middle\n@@ -50 +50 @@\n-last\n+final é\n";
    let rendered = display(diff);
    let capture = selected(diff, range(&rendered, "世界", "final"), "file", true).unwrap();
    assert_eq!(
        capture.text,
        "世界\n+new 界\n@@ -20 +20 @@\n-next\n+middle\n@@ -50 +50 @@\n-last\n+final"
    );
    assert!(capture.label.contains("3 hunks"));
    assert!(!capture.text.contains('…'));
    let caption = rendered.find('…').unwrap();
    let start = rendered[..caption].chars().count();
    assert!(selected(diff, start..start + 1, "file", true).is_none());
}
#[test]
fn marker_prefixed_content_is_not_mistaken_for_file_headers_within_a_hunk() {
    let diff = "--- a/file\n+++ b/file\n@@ -1,2 +1,2 @@\n--- removed literal\n+++ added literal\n context\n";
    let rendered = display(diff);
    let captured = selected(
        diff,
        range(&rendered, "--- removed", "+++ added literal"),
        "file",
        false,
    )
    .unwrap();
    assert_eq!(captured.text, "--- removed literal\n+++ added literal");
    assert!(captured.label.ends_with("old 1–1 · new 1–1"));
    for header in [
        "@@ -5,invalid +6 @@",
        "@@ -5 +6 missing-close",
        "@@ -0 +1 @@",
        "@@@ -1 +1 @@@",
    ] {
        let diff = format!("{header}\n-old\n+new\n");
        let rendered = display(&diff);
        let captured = selected(&diff, range(&rendered, "-old", "+new"), "file", false).unwrap();
        assert!(
            captured.label.contains("coordinates unavailable"),
            "{}",
            captured.label
        );
    }
}

#[test]
fn missing_or_bad_hunk_coordinates_are_truthfully_unknown_and_bounded() {
    for diff in [
        "-before\n+after\n".to_owned(),
        "@@ bad header\n-before\n+after\n".to_owned(),
    ] {
        let rendered = display(&diff);
        let captured = selected(
            &diff,
            range(&rendered, "-before", "+after"),
            "recorded",
            true,
        )
        .unwrap();
        assert!(captured.label.contains("coordinates unavailable"));
        assert_eq!(captured.text, "-before\n+after");
    }
    let diff = format!(
        "@@ -1 +1 @@\n-{}\n+{}\n",
        "界".repeat(100_000),
        "e\u{301}".repeat(100_000)
    );
    let rendered = display(&diff);
    let count = rendered.chars().count();
    let start = std::time::Instant::now();
    let captured = selected(&diff, 10..count, "bounded.rs", true).unwrap();
    eprintln!(
        "Diff selection mapping: {} retained bytes in {:?}",
        captured.text.len(),
        start.elapsed()
    );
    assert!(captured.text.len() > MAX_BYTES && captured.text.len() <= MAX_BYTES + 4);
    let ctx = egui::Context::default();
    queue(&ctx, &captured.label, &captured.text);
    let mut app = crate::app::files::tests::app(&ctx);
    app.collect_context_capture();
    assert_eq!(
        app.draft
            .matches("Context excerpt limited to 24 KiB")
            .count(),
        1
    );
}

fn input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(1000.0, 700.0),
        )),
        events,
        ..Default::default()
    }
}
pub(super) fn pos(output: &egui::FullOutput, character: usize) -> egui::Pos2 {
    let text = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.contains("+new selected") => Some(text),
            _ => None,
        })
        .unwrap();
    text.pos
        + text
            .galley
            .pos_from_cursor(egui::text::CCursor::new(character))
            .center()
            .to_vec2()
}
pub(super) fn pointer(
    position: egui::Pos2,
    button: egui::PointerButton,
    pressed: bool,
) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(position),
        egui::Event::PointerButton {
            pos: position,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
#[test]
fn frozen_diff_snapshot_is_not_replaced_when_same_widget_updates() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let original = "@@ -10,2 +10,2 @@\n-old\n+new selected\n context\n";
    let draw = |events, source| {
        ctx.run(input(events), |ctx| {
            context_capture::inputs(ctx);
            egui::CentralPanel::default().show(ctx, |ui| {
                changes::numbered_diff(ui, "same.rs", source, false)
            });
        })
    };
    for _ in 0..3 {
        draw(vec![], original);
    }
    let output = draw(vec![], original);
    let selected = range(&display(original), "-old", "+new selected");
    let start = pos(&output, selected.start);
    let end = pos(&output, selected.end);
    draw(pointer(start, egui::PointerButton::Primary, true), original);
    draw(vec![egui::Event::PointerMoved(end)], original);
    draw(pointer(end, egui::PointerButton::Primary, false), original);
    for pressed in [true, false] {
        draw(
            pointer(start, egui::PointerButton::Secondary, pressed),
            original,
        );
    }
    for _ in 0..3 {
        draw(vec![], "@@ -1 +1 @@\n+replacement\n");
    }
    let target = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        draw(
            pointer(target, egui::PointerButton::Primary, pressed),
            "@@ -1 +1 @@\n+replacement\n",
        );
    }
    let mut app = crate::app::files::tests::app(&ctx);
    app.collect_context_capture();
    assert!(app.draft.contains("-old\n+new selected"));
    assert!(!app.draft.contains("replacement"));
}

#[test]
fn actual_numbered_diff_selection_adds_exact_text_once_and_never_includes_gutters() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for backwards in [false, true] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            let diff =
                "@@ -10,2 +10,2 @@\n-old\n+new selected\n context\n@@ -50 +50 @@\n-next\n+last\n";
            let draw = |events| {
                ctx.run(input(events), |ctx| {
                    context_capture::inputs(ctx);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        changes::numbered_diff(ui, "src/one.rs", diff, true)
                    });
                })
            };
            for _ in 0..3 {
                draw(vec![]);
            }
            let rendered = display(diff);
            let range = range(&rendered, "-old", "+last");
            let output = draw(vec![]);
            let (start, end) = if backwards {
                (range.end, range.start)
            } else {
                (range.start, range.end)
            };
            let start = pos(&output, start);
            let end = pos(&output, end);
            draw(pointer(start, egui::PointerButton::Primary, true));
            draw(vec![egui::Event::PointerMoved(end)]);
            draw(pointer(end, egui::PointerButton::Primary, false));
            for pressed in [true, false] {
                draw(pointer(start, egui::PointerButton::Secondary, pressed));
            }
            for _ in 0..3 {
                draw(vec![]);
            }
            let target = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("source-context-selection")))
                .unwrap()
                .center();
            for pressed in [true, false] {
                draw(pointer(target, egui::PointerButton::Primary, pressed));
            }
            let mut app = crate::app::files::tests::app(&ctx);
            app.draft = "Keep draft".into();
            app.state.busy = true;
            let count = app.state.messages.len();
            app.collect_context_capture();
            assert_eq!(
                app.draft,
                "Keep draft\n\n[Context: src/one.rs · Recorded edits (not net Git) · diff rows 2–7 · 2 hunks · original hunk headers included · discontinuous file ranges]\n-old\n+new selected\n context\n@@ -50 +50 @@\n-next\n+last\n[/Context]"
            );
            let draft = app.draft.clone();
            app.collect_context_capture();
            assert_eq!(app.draft, draft);
            assert!(app.state.busy);
            assert_eq!(app.state.messages.len(), count);
        }
    }
}
