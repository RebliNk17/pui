use super::*;

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
fn frame(
    ctx: &egui::Context,
    doc: &mut Document,
    size: Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let time = ctx.input(|i| i.time) + 0.016;
    ctx.run(
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            context_capture::inputs(ctx);
            assert!(show_document(ctx, doc, true));
        },
    )
}
fn toggle(ctx: &egui::Context, doc: &mut Document, size: Vec2) {
    let pos = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("diff-wrap-lines")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        frame(
            ctx,
            doc,
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
        );
    }
}
fn source() -> String {
    format!(
        "@@ -1,2 +1,2 @@\n-{}\n+{}\n context\n@@ -50 +50 @@\n-old second\n+new second\n",
        "old 界e\u{301} ".repeat(30),
        "new שלום ".repeat(40)
    )
}
fn code(output: &egui::FullOutput) -> (&egui::Rect, &egui::epaint::TextShape) {
    output
        .shapes
        .iter()
        .find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                t.galley
                    .job
                    .text
                    .contains("old 界")
                    .then_some((&s.clip_rect, t))
            } else {
                None
            }
        })
        .unwrap()
}
#[test]
fn actual_wrap_toggle_preserves_display_characters_copy_and_expands_logical_line_backgrounds() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let source = source();
    let mut doc: Document = ("wrap.rs".into(), source.clone()).into();
    let size = Vec2::new(620.0, 680.0);
    for _ in 0..4 {
        frame(&ctx, &mut doc, size, vec![]);
    }
    let before = frame(&ctx, &mut doc, size, vec![]);
    let text = code(&before).1.galley.job.text.clone();
    let rows = code(&before).1.galley.rows.len();
    toggle(&ctx, &mut doc, size);
    assert!(doc.2.wrap);
    for _ in 0..5 {
        frame(&ctx, &mut doc, size, vec![]);
    }
    let out = frame(&ctx, &mut doc, size, vec![]);
    let (_, shape) = code(&out);
    assert_eq!(shape.galley.job.text, text);
    assert!(shape.galley.rows.len() > rows + 5);
    assert!(shape.galley.size().x < 600.0);
    assert_eq!(doc.1, source);
    let colors = theme::palette(&ctx);
    let backgrounds = out
        .shapes
        .iter()
        .find_map(|s| {
            if let egui::Shape::Vec(v) = &s.shape {
                v.iter()
                    .any(|s| matches!(s,egui::Shape::Rect(r) if r.fill==colors.diff_remove_bg))
                    .then_some(v)
            } else {
                None
            }
        })
        .unwrap();
    let mut logical = 0;
    let mut removed = 0;
    let mut added = 0;
    for row in &shape.galley.rows {
        if logical == 1 || logical == 2 {
            let pos = shape.pos + row.rect().center().to_vec2();
            let expected = if logical == 1 {
                colors.diff_remove_bg
            } else {
                colors.diff_add_bg
            };
            assert!(
                backgrounds.iter().any(
                    |s| matches!(s,egui::Shape::Rect(r) if r.fill==expected && r.rect.contains(pos))
                ),
                "missing wrapped background for logical {logical}"
            );
            if logical == 1 {
                removed += 1;
            } else {
                added += 1;
            }
        }
        if row.ends_with_newline {
            logical += 1;
        }
    }
    assert!(removed > 1 && added > 1);
    let pos = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("diff-copy")))
        .unwrap()
        .center();
    let mut copied = false;
    for pressed in [true, false] {
        let out = frame(
            &ctx,
            &mut doc,
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
        );
        copied |= out
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c,egui::OutputCommand::CopyText(t) if t==&source));
    }
    assert!(copied);
    toggle(&ctx, &mut doc, size);
    assert!(!doc.2.wrap);
    let out = frame(&ctx, &mut doc, size, vec![]);
    assert_eq!(code(&out).1.galley.job.text, text);
    assert_eq!(code(&out).1.galley.rows.len(), rows);
}
#[test]
fn wrapped_hunk_navigation_uses_visual_geometry_and_reveals_after_resize() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut doc: Document = ("wrap.rs".into(), source()).into();
    let size = Vec2::new(620.0, 680.0);
    for _ in 0..4 {
        frame(&ctx, &mut doc, size, vec![]);
    }
    toggle(&ctx, &mut doc, size);
    frame(
        &ctx,
        &mut doc,
        size,
        vec![key(egui::Key::End, egui::Modifiers::ALT)],
    );
    for _ in 0..100 {
        frame(&ctx, &mut doc, size, vec![]);
    }
    assert_eq!(doc.2.selected, Some(1));
    for size in [Vec2::new(420.0, 560.0), Vec2::new(900.0, 760.0)] {
        for _ in 0..100 {
            frame(&ctx, &mut doc, size, vec![]);
        }
        let out = frame(&ctx, &mut doc, size, vec![]);
        let (clip, text) = code(&out);
        let mut logical = 0;
        let visual = text
            .galley
            .rows
            .iter()
            .find(|row| {
                let found = logical == 5;
                if row.ends_with_newline {
                    logical += 1;
                }
                found
            })
            .unwrap();
        let rect = visual.rect().translate(text.pos.to_vec2());
        assert!(clip.contains(rect.center()), "{clip:?} {rect:?}");
        assert!(
            out.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(1)
        );
    }
}
#[test]
fn maximum_wrapped_input_is_bounded_and_reuses_cached_layout_without_more_body_storage() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let size = Vec2::new(420.0, 560.0);
    let source = format!("@@ -1 +1 @@\n-{}\n+end\n", "a".repeat(WRAP_MAX_BYTES - 64));
    let mut doc: Document = ("bounded.rs".into(), source).into();
    assert!(doc.2.wrap_available);
    doc.2.wrap = true;
    let pointer = doc.1.as_ptr();
    let started = std::time::Instant::now();
    let out = frame(&ctx, &mut doc, size, vec![]);
    let _ = out; // first sizing pass may not paint text
    let out = frame(&ctx, &mut doc, size, vec![]);
    let galley = out
        .shapes
        .iter()
        .find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                t.galley.job.text.contains("aaaa").then(|| t.galley.clone())
            } else {
                None
            }
        })
        .expect("painted wrapped source after sizing");
    let rows = galley.rows.len();
    eprintln!(
        "wrapped maximum input {} source bytes, first layout {:?}, visual rows {:?}; no second retained document body",
        doc.1.len(),
        started.elapsed(),
        rows
    );
    for _ in 0..5 {
        frame(&ctx, &mut doc, size, vec![]);
    }
    let settled = frame(&ctx, &mut doc, size, vec![]);
    let previous = settled
        .shapes
        .iter()
        .find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                t.galley.job.text.contains("aaaa").then(|| t.galley.clone())
            } else {
                None
            }
        })
        .unwrap();
    let out = frame(&ctx, &mut doc, size, vec![]);
    let same = out
        .shapes
        .iter()
        .find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                t.galley.job.text.contains("aaaa").then_some(&t.galley)
            } else {
                None
            }
        })
        .unwrap();
    assert!(
        std::sync::Arc::ptr_eq(&previous, same),
        "unchanged layout should reuse egui font cache"
    );
    assert_eq!(pointer, doc.1.as_ptr());
    assert!(doc.2.wrap);
}

#[test]
fn wrap_is_opt_in_per_document_keyboard_toggle_and_large_inputs_are_disabled() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let size = Vec2::new(620.0, 680.0);
    let mut doc: Document = ("wrap.rs".into(), source()).into();
    assert!(!doc.2.wrap);
    for _ in 0..4 {
        frame(&ctx, &mut doc, size, vec![]);
    }
    let id = ctx
        .data(|d| d.get_temp::<egui::Id>(egui::Id::new("diff-wrap-lines-focus")))
        .unwrap();
    ctx.memory_mut(|m| m.request_focus(id));
    frame(
        &ctx,
        &mut doc,
        size,
        vec![key(egui::Key::Space, egui::Modifiers::NONE)],
    );
    assert!(doc.2.wrap);
    let fresh: Document = ("wrap.rs".into(), doc.1.clone()).into();
    assert!(!fresh.2.wrap);
    for source in [
        "x".repeat(WRAP_MAX_BYTES + 1),
        "+x\n".repeat(WRAP_MAX_ROWS + 1),
    ] {
        let mut large: Document = ("large.rs".into(), source).into();
        assert!(!large.2.wrap_available);
        for _ in 0..3 {
            frame(&ctx, &mut large, size, vec![]);
        }
        toggle(&ctx, &mut large, size);
        assert!(!large.2.wrap);
    }
}
