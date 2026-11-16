use super::*;
use crate::app::files::tests;
#[test]
fn pathless_section_row_and_menu_remain_inside_sidebar_at_narrow_sizes_and_zoom() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [400.0, 1134.0] {
            for zoom in [0.8, 1.5] {
                let ctx = egui::Context::default();
                let mut app = tests::app(&ctx);
                app.sessions.clear();
                app.state = State::default();
                app.demo_sidebar_sections();
                app.prefs.theme = mode;
                ctx.set_zoom_factor(zoom);
                let id = app.agents.active.clone();
                let key = format!("agent:{id}");
                let mut render = |events| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 800.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| app.sidebar(ctx),
                    )
                };
                for _ in 0..4 {
                    render(vec![]);
                }
                let rect = ctx
                    .data(|data| {
                        data.get_temp::<egui::Rect>(egui::Id::new(("sidebar-unsaved-menu", &key)))
                    })
                    .unwrap();
                assert!(
                    rect.left() >= 0.0 && rect.right() <= width,
                    "overflow outside narrow sidebar {rect:?}"
                );
                let point = rect.center();
                for pressed in [true, false] {
                    render(vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]);
                }
                let output = render(vec![]);
                assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(t) if t.galley.job.text=="Move to section")));
                assert!(app.section_view.unsaved_popup.is_some());
            }
        }
    }
}
#[test]
fn runtime_association_metadata_bound_does_not_retain_transcript_or_session_paths() {
    let ctx = egui::Context::default();
    let mut app = tests::app(&ctx);
    for n in 0..256 {
        let id = format!("{n:036}");
        app.set_unsaved_section(&id, Some("s".repeat(36)));
    }
    let bytes = app
        .section_view
        .unsaved
        .iter()
        .map(|(id, section)| id.capacity() + section.capacity())
        .sum::<usize>();
    assert!(bytes <= 256 * 72);
    let start = std::time::Instant::now();
    for _ in 0..10_000 {
        assert_eq!(app.section_view.unsaved.len(), 256);
    }
    eprintln!(
        "256pathless associations <= {bytes} string capacity bytes,10000settledlenchecks {:?}; BTree/string overhead excluded",
        start.elapsed()
    );
}
