use super::*;
fn frame(
    ctx: &egui::Context,
    browser: &mut BrowserView,
    events: Vec<egui::Event>,
    enabled: bool,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(360.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    browser.history_menu(ui, "https://example.test")
                });
            });
        },
    )
}
fn labels(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(text) => out.push((
            text.galley.job.text.clone(),
            egui::Rect::from_min_size(text.pos, text.galley.size()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                labels(shape, out);
            }
        }
        _ => {}
    }
}
#[test]
fn history_surface_clear_is_group_scoped_and_disabled_actions_do_not_clear() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        mode.apply(&ctx);
        let mut browser = BrowserView::default();
        let key = "https://example.test";
        browser.active = key.into();
        let context = browser.privacy.ensure(key);
        let other = browser.privacy.ensure("other");
        browser
            .history
            .recorder(context)
            .record("https://example.test/path?secret=hidden");
        browser.history.recorder(other).record("https://other.test");
        for enabled in [false, true] {
            for _ in 0..3 {
                frame(&ctx, &mut browser, vec![], enabled);
            }
            let output = frame(&ctx, &mut browser, vec![], enabled);
            let mut text = Vec::new();
            for shape in output.shapes {
                labels(&shape.shape, &mut text);
            }
            assert!(
                !text
                    .iter()
                    .any(|(value, _)| value.contains("secret=hidden"))
            );
            let point = text
                .iter()
                .find(|(value, _)| value == "Clear this group's visited history")
                .unwrap()
                .1
                .center();
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut browser,
                    vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    enabled,
                );
            }
            assert_eq!(browser.history.entries(context).is_empty(), enabled);
            assert_eq!(browser.history.entries(other).len(), 1);
        }
    }
}
