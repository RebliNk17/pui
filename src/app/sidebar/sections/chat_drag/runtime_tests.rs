use super::*;
use crate::app::files::tests::app;
fn frame(ctx: &egui::Context, app: &mut Pui, events: Vec<egui::Event>) {
    let mut native = eframe::Frame::_new_kittest();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(620.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    );
}
#[test]
fn pathless_pointer_gesture_assigns_exact_runtime_and_promotion_refuses_old_drop() {
    for dark in [false, true] {
        for zoom in [0.8, 1.4] {
            for promote in [false, true] {
                let ctx = egui::Context::default();
                let mut app = app(&ctx);
                app.sessions.clear();
                app.state = State::default();
                let id = app.ensure_agent();
                app.prefs.theme = if dark {
                    theme::Theme::Dark
                } else {
                    theme::Theme::Light
                };
                ctx.set_zoom_factor(zoom);
                app.prefs.sidebar_view.empty_groups = true;
                let section = Section::new("Drop runtime chat".into());
                let target_id = section.id.clone();
                app.prefs.sidebar_view.sections.push(section);
                app.draft = "runtime draft".into();
                app.state.busy = true;
                for _ in 0..4 {
                    frame(&ctx, &mut app, vec![]);
                }
                let source = ctx
                    .data(|data| {
                        data.get_temp::<egui::Rect>(egui::Id::new((
                            "sidebar-row-rect",
                            format!("agent:{id}"),
                        )))
                    })
                    .unwrap()
                    .center();
                let target = ctx
                    .data(|data| {
                        data.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new((
                            "section-header-rects",
                            &target_id,
                        )))
                    })
                    .unwrap()
                    .0
                    .center();
                let pointer = |pos, pressed| egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                frame(&ctx, &mut app, vec![egui::Event::PointerMoved(source)]);
                frame(
                    &ctx,
                    &mut app,
                    vec![egui::Event::PointerMoved(source), pointer(source, true)],
                );
                frame(
                    &ctx,
                    &mut app,
                    vec![egui::Event::PointerMoved(source + egui::vec2(12.0, 0.0))],
                );
                frame(&ctx, &mut app, vec![egui::Event::PointerMoved(target)]);
                if promote {
                    app.agents.nodes.get_mut(&id).unwrap().path =
                        Some(app.prefs.cwd.join("promoted-runtime.jsonl"));
                }
                frame(
                    &ctx,
                    &mut app,
                    vec![
                        pointer(target, false),
                        egui::Event::Paste("never insert".into()),
                    ],
                );
                assert_eq!(
                    app.section_view.unsaved.get(&id).map(String::as_str),
                    if promote {
                        None
                    } else {
                        Some(target_id.as_str())
                    }
                );
                assert_eq!(app.agents.active, id);
                assert_eq!(app.draft, "runtime draft");
                assert!(app.state.busy);
            }
        }
    }
}
