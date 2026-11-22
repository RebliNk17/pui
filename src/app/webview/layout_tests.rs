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
                            ..Default::default()
                        },
                    );
                    let mut rect = egui::Rect::NOTHING;
                    let mut console = egui::Rect::NOTHING;
                    let mut status_bottom = f32::INFINITY;
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
                                    || text.galley.job.text.contains("Page returned HTTP")
                                {
                                    status_bottom = status_bottom.min(text.pos.y);
                                }
                            }
                        }
                    }
                    assert!(
                        rect.is_finite() && rect.is_positive(),
                        "{device:?} {zoom}: {rect:?}"
                    );
                    assert!(
                        status_bottom < f32::INFINITY && rect.bottom() <= status_bottom,
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
fn fixed_viewports_scale_down_automatically_without_reflow_or_zoom_changes() {
    for (device, width, height) in [
        ("responsive", 320.0, 500.0),
        ("desktop", 320.0, 500.0),
        ("tablet", 700.0, 260.0),
        ("mobile", 320.0, 500.0),
        ("custom:3840x2160", 180.0, 100.0),
    ] {
        for rotate in [false, true] {
            let ctx = egui::Context::default();
            let mut browser = BrowserView::default();
            browser.configure("fixture", device, false).unwrap();
            let options = browser.options.get_mut("fixture").unwrap();
            options.rotate = rotate;
            options.zoom = 1.5;
            let expected = options.clone();
            let mut pane = egui::Rect::NOTHING;
            let mut native = egui::Rect::NOTHING;
            for _ in 0..3 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, height),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            pane = ui.available_rect_before_wrap();
                            native =
                                browser.viewport_with_state(ui, "fixture", PageState::default());
                        });
                    },
                );
            }
            assert!(native.is_positive() && pane.contains_rect(native));
            if let Some((w, h)) = expected.dimensions() {
                assert!(
                    (native.width() / native.height() - w as f32 / h as f32).abs() < 0.01,
                    "{device} {rotate}: aspect ratio changed: {native:?}"
                );
                assert!(native.width() <= w as f32 && native.height() <= h as f32);
            } else {
                assert_eq!(native, pane);
            }
            assert_eq!(
                browser.options["fixture"], expected,
                "Scaling must not change CSS viewport or page zoom"
            );
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
