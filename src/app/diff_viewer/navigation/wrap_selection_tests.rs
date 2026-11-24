use super::*;
use crate::app::files::tests::{app, frame};

fn pointer(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        modifiers,
        physical_key: None,
        pressed: true,
        repeat: false,
    }
}
fn text(out: &egui::FullOutput) -> &egui::epaint::TextShape {
    out.shapes
        .iter()
        .find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                t.galley.job.text.contains("+chosen").then_some(t)
            } else {
                None
            }
        })
        .unwrap()
}
fn at(out: &egui::FullOutput, index: usize) -> egui::Pos2 {
    let t = text(out);
    t.pos
        + t.galley
            .pos_from_cursor(egui::text::CCursor::new(index))
            .center()
            .to_vec2()
}
#[test]
fn actual_wrapped_unicode_selection_omits_soft_breaks_and_preserves_live_approval_and_draft() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let chosen = format!("+chosen {} finish", "界e\u{301} ".repeat(45));
    let diff = format!("@@ -1 +1 @@\n-old\n{chosen}\n");
    app.open_diff_review(("src/wrapped.rs".into(), diff.clone()).into(), false);
    app.prefs.hide_sidebar = true;
    app.prefs.hide_inspector = true;
    app.draft = "Kept draft".into();
    app.context_chips.clear_quotes();
    app.state.busy = true;
    let permission = json!({"id":"wrap-permission","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."});
    assert!(prompts::approval(&permission).is_some());
    app.state.dialogs.push_back(permission);
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let count = app.state.messages.len();
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let pos = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("diff-wrap-lines")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut app,
            pointer(pos, egui::PointerButton::Primary, pressed),
        );
    }
    assert!(app.changes.review.as_ref().unwrap().2.wrap());
    let out = frame(&ctx, &mut app, vec![]);
    let t = text(&out);
    let raw = &t.galley.job.text;
    let start = raw[..raw.find("+chosen").unwrap()].chars().count();
    let end = start + chosen.chars().count();
    let a = at(&out, start);
    let b = at(&out, end);
    assert!(b.y > a.y, "selection must cross actual visual wraps");
    frame(
        &ctx,
        &mut app,
        pointer(a, egui::PointerButton::Primary, true),
    );
    frame(&ctx, &mut app, vec![egui::Event::PointerMoved(b)]);
    frame(
        &ctx,
        &mut app,
        pointer(b, egui::PointerButton::Primary, false),
    );
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut app,
            pointer(a, egui::PointerButton::Secondary, pressed),
        );
    }
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(egui::Popup::is_any_open(&ctx));
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)],
    );
    assert_eq!(app.state.dialogs.len(), 1);
    assert_eq!(app.outgoing_text(), "Kept draft");
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(app.outgoing_text().starts_with(
        "Kept draft\n\n[Context: src/wrapped.rs · Working tree vs HEAD · diff rows 3–3"
    ));
    assert!(
        app.outgoing_text()
            .ends_with(&format!("\n{chosen}\n[/Context]")),
        "{}",
        app.outgoing_text()
    );
    assert_eq!(app.changes.review.as_ref().unwrap().1, diff);
    assert!(app.state.busy);
    assert_eq!(app.state.messages.len(), count);
    assert_eq!(app.state.dialogs.len(), 1);
    assert!(commands.try_recv().is_err());
}
#[test]
fn wrapped_demo_is_real_readonly_view_without_provider() {
    let ctx = egui::Context::default();
    let mut app = crate::app::Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_diff_hunks: true,
            demo_diff_wrap: true,
            ..Default::default()
        },
    );
    for _ in 0..5 {
        frame(&ctx, &mut app, vec![]);
    }
    assert!(app.changes.review.as_ref().unwrap().2.wrap());
    assert!(app.client.is_none());
    let out = frame(&ctx, &mut app, vec![]);
    assert!(out.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text=="Recorded edits · read-only")));
}

#[test]
fn changing_wrap_clears_stale_selection_without_quoting_or_sending() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.open_diff_review(
        (
            "wrap.rs".into(),
            "@@ -1 +1 @@\n-old\n+chosen letters\n".into(),
        )
            .into(),
        false,
    );
    app.draft = "Keep".into();
    app.context_chips.clear_quotes();
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let out = frame(&ctx, &mut app, vec![]);
    let raw = &text(&out).galley.job.text;
    let start = raw[..raw.find("chosen").unwrap()].chars().count();
    let a = at(&out, start);
    let b = at(&out, start + 6);
    frame(
        &ctx,
        &mut app,
        pointer(a, egui::PointerButton::Primary, true),
    );
    frame(&ctx, &mut app, vec![egui::Event::PointerMoved(b)]);
    frame(
        &ctx,
        &mut app,
        pointer(b, egui::PointerButton::Primary, false),
    );
    assert!(egui::text_selection::LabelSelectionState::load(&ctx).has_selection());
    let pos = ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("diff-wrap-lines")))
        .unwrap()
        .center();
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut app,
            pointer(pos, egui::PointerButton::Primary, pressed),
        );
    }
    assert!(app.changes.review.as_ref().unwrap().2.wrap());
    assert!(!egui::text_selection::LabelSelectionState::load(&ctx).has_selection());
    assert_eq!(app.outgoing_text(), "Keep");
    assert!(
        app.changes.review.as_ref().unwrap().2.owns_focus(&ctx),
        "focus after wrap: {:?}",
        ctx.memory(|m| m.focused())
    );
    frame(
        &ctx,
        &mut app,
        vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(app.changes.review.is_none());
}
#[test]
fn light_dark_narrow_zoom_wrapped_labels_fit_and_settle() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let diff = format!(
                "@@ -1 +1 @@\n-old\n+{}\n",
                "unbrokenlongliteral界".repeat(50)
            );
            let mut doc: Document = ("wrap.rs".into(), diff).into();
            doc.2.wrap = true;
            let mut time = 0.0;
            let mut draw = || {
                time += 0.016;
                ctx.run(
                    egui::RawInput {
                        time: Some(time),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 560.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        show_document(ctx, &mut doc, true);
                    },
                )
            };
            for _ in 0..100 {
                draw();
            }
            let out = draw();
            let window = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("diff-review-bounds")))
                .unwrap();
            let wrap = ctx
                .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("diff-wrap-lines")))
                .unwrap();
            assert!(window.contains_rect(wrap));
            let code = out
                .shapes
                .iter()
                .find_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape {
                        t.galley
                            .job
                            .text
                            .contains("unbroken")
                            .then_some((s.clip_rect, t))
                    } else {
                        None
                    }
                })
                .unwrap();
            assert!(
                code.1.galley.size().x <= code.0.width() + 1.0,
                "{} > {}",
                code.1.galley.size().x,
                code.0.width()
            );
            assert!(
                out.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(1)
            );
        }
    }
}
