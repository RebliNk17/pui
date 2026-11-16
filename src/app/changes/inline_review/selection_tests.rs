use super::tests::{DIFF, PATH, click_control, key, nav, pointer, setup};
use super::*;
use crate::app::files::tests::frame;
fn code(output: &egui::FullOutput) -> &egui::epaint::TextShape {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.contains("+chosen") => Some(text),
            _ => None,
        })
        .unwrap()
}
fn select(ctx: &egui::Context, app: &mut Pui, chosen: &str) -> egui::Pos2 {
    let output = frame(ctx, app, vec![]);
    let text = code(&output);
    let raw = &text.galley.job.text;
    let start = raw[..raw.find(chosen).unwrap()].chars().count();
    let at = |index| {
        text.pos
            + text
                .galley
                .pos_from_cursor(egui::text::CCursor::new(index))
                .center()
                .to_vec2()
    };
    let a = at(start);
    let b = at(start + chosen.chars().count());
    assert!(b.y > a.y, "selection must cross actual soft wraps");
    frame(ctx, app, pointer(a, true));
    frame(ctx, app, vec![egui::Event::PointerMoved(b)]);
    frame(ctx, app, pointer(b, false));
    a
}
fn secondary(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Secondary,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
#[test]
fn inline_git_recorded_actual_wrapped_capture_is_exact_one_hunk_and_copy_survives() {
    for git in [false, true] {
        let chosen = format!("+chosen {} finish", "界e\u{301} ".repeat(32));
        let diff =
            format!("@@ -10 +10 @@\n-old\n{chosen}\n@@ -50 +50 @@\n-old second\n+new second\n");
        let (ctx, mut app) = setup(git, &diff);
        let commands = super::tests::approval(&mut app);
        click_control(&ctx, &mut app, "diff-wrap-lines-focus");
        let a = select(&ctx, &mut app, &chosen);
        let output = frame(&ctx, &mut app, vec![egui::Event::Copy]);
        assert!(output.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(text) if text == &chosen)
        ));
        for pressed in [true, false] {
            frame(&ctx, &mut app, secondary(a, pressed));
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
        assert_eq!(app.draft, "Kept draft");
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        let source = if git {
            "Working tree vs HEAD"
        } else {
            "Recorded edits (not net Git)"
        };
        assert!(
            app.draft.starts_with(&format!(
                "Kept draft\n\n[Context: {PATH} · {source} · diff rows 3–3 · new 10–10]"
            )),
            "{}",
            app.draft
        );
        assert!(
            app.draft.ends_with(&format!("\n{chosen}\n[/Context]")),
            "{}",
            app.draft
        );
        assert!(app.changes.review.is_none());
        assert_eq!(app.outputs[0].diff, diff);
        assert!(app.state.busy);
        assert_eq!(app.state.dialogs.len(), 1);
        assert!(commands.try_recv().is_err());
    }
}
#[test]
fn wrap_limits_and_narrow_light_dark_zoom_keep_unmodified_bounded_navigation() {
    for source in [
        format!("@@ -1 +1 @@\n+{}", "x".repeat(64 * 1024)),
        " context\n".repeat(4097),
    ] {
        let (ctx, mut app) = setup(false, &source);
        let wrap = ctx
            .read_response(super::tests::control(&ctx, "diff-wrap-lines-focus"))
            .unwrap();
        assert!(!wrap.enabled());
        for pressed in [true, false] {
            frame(&ctx, &mut app, pointer(wrap.rect.center(), pressed));
        }
        assert!(!nav(&app, false).wrap());
    }
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let (ctx, mut app) = setup(false, DIFF);
            app.prefs.theme = theme;
            app.state.busy = false;
            ctx.set_zoom_factor(zoom);
            let mut time = 0.0;
            let mut draw = |events| {
                time += 0.1;
                let mut native = eframe::Frame::_new_kittest();
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(420.0, 780.0),
                        )),
                        events,
                        time: Some(time),
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut native),
                )
            };
            for _ in 0..8 {
                draw(vec![]);
            }
            let wrap = ctx
                .read_response(super::tests::control(&ctx, "diff-wrap-lines-focus"))
                .unwrap();
            for pressed in [true, false] {
                draw(pointer(wrap.rect.center(), pressed));
            }
            draw(vec![egui::Event::PointerGone]);
            for _ in 0..20 {
                draw(vec![]);
            }
            let output = draw(vec![]);
            assert!(
                output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                    > Duration::from_secs(1),
                "theme {theme:?} zoom {zoom} repaint {:?}",
                output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
            );
            let wrap = ctx
                .read_response(super::tests::control(&ctx, "diff-wrap-lines-focus"))
                .unwrap();
            assert!(wrap.rect.width() < 100.0);
        }
    }
}
