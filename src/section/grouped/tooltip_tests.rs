use super::*;

#[test]
fn fully_visible_title_has_no_tooltip_but_elided_title_does() {
    for width in [160.0, 640.0] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.style_mut(|style| style.interaction.tooltip_delay = 0.0);
        let title = "Report from btw diagnosis (agent-257)";
        let mut pos = egui::pos2(60.0, 24.0);
        let mut output = None;
        for pass in 0..6 {
            output = Some(ctx.run(
                egui::RawInput {
                    time: Some(pass as f64),
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(width + 16.0, 500.0),
                    )),
                    events: vec![egui::Event::PointerMoved(pos)],
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.set_width(width);
                        let response = Section::new(title).grouped().show(ui, |_| {});
                        pos = response.rect.center();
                    });
                },
            ));
        }
        fn count(shape: &egui::Shape, title: &str) -> usize {
            match shape {
                egui::Shape::Text(text) => usize::from(text.galley.job.text == title),
                egui::Shape::Vec(shapes) => shapes.iter().map(|shape| count(shape, title)).sum(),
                _ => 0,
            }
        }
        let count: usize = output
            .unwrap()
            .shapes
            .iter()
            .map(|shape| count(&shape.shape, title))
            .sum();
        assert_eq!(
            count > 1,
            width < 200.0,
            "width {width}, title copies {count}"
        );
    }
}
