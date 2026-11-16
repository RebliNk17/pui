use super::*;

#[test]
fn status_and_console_stay_outside_native_rect_in_every_device_and_theme() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for device in [
            Device::Responsive,
            Device::Desktop,
            Device::Tablet,
            Device::Mobile,
            Device::Custom,
        ] {
            for zoom in [0.8, 1.0, 1.4] {
                for error in [false, true] {
                    let ctx = egui::Context::default();
                    theme::install(&ctx);
                    mode.apply(&ctx);
                    ctx.set_zoom_factor(zoom);
                    let mut browser = BrowserView::default();
                    let key = "http://localhost/fixture";
                    browser.options.insert(
                        key.into(),
                        Options {
                            device,
                            console: true,
                            fit: false,
                            ..Default::default()
                        },
                    );
                    let mut rect = egui::Rect::NOTHING;
                    let mut console = egui::Rect::NOTHING;
                    let mut status_bottom = 0.0_f32;
                    for _ in 0..3 {
                        let output = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    Vec2::new(650.0, 700.0),
                                )),
                                ..Default::default()
                            },
                            |ctx| {
                                egui::CentralPanel::default().show(ctx, |ui| {
                                    rect = browser.viewport_with_state(
                                        ui,
                                        key,
                                        PageState {
                                            loading: !error,
                                            error: error.then(|| "Page returned HTTP 503.".into()),
                                            ..Default::default()
                                        },
                                    );
                                });
                                console = ctx
                                    .data(|data| {
                                        data.get_temp::<egui::Rect>(egui::Id::new((
                                            "browser-control",
                                            "Hide console",
                                        )))
                                    })
                                    .unwrap();
                            },
                        );
                        for shape in output.shapes {
                            if let egui::Shape::Text(text) = shape.shape {
                                if text.galley.job.text.contains("Loading page")
                                    || text.galley.job.text.contains("Certificate checks")
                                {
                                    status_bottom = text.pos.y + text.galley.size().y;
                                }
                            }
                        }
                    }
                    assert!(
                        rect.is_finite() && rect.is_positive(),
                        "{device:?} {zoom}: {rect:?}"
                    );
                    assert!(
                        status_bottom > 0.0 && rect.top() >= status_bottom,
                        "Page obscures status: {device:?} {zoom}: {rect:?} / {status_bottom}"
                    );
                    assert!(
                        rect.bottom() < console.top(),
                        "Page obscures console: {device:?} {zoom}: {rect:?} / {console:?}"
                    );
                    assert!(
                        ctx.screen_rect().contains_rect(rect),
                        "Page escapes viewport: {device:?} {zoom}: {rect:?} / {:?}",
                        ctx.screen_rect()
                    );
                }
            }
        }
    }
}

#[test]
fn empty_viewport_does_not_return_an_inverted_native_rectangle() {
    let ctx = egui::Context::default();
    let mut browser = BrowserView::default();
    for height in [1.0, 20.0, 50.0] {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(200.0, height),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let rect = browser.viewport_with_state(
                        ui,
                        "fixture",
                        PageState {
                            error: Some("Unavailable".into()),
                            ..Default::default()
                        },
                    );
                    assert!(
                        rect == egui::Rect::NOTHING || (rect.is_finite() && rect.is_positive())
                    );
                });
            },
        );
    }
}
