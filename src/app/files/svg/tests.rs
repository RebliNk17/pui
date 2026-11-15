use super::*;
#[test]
fn svg_source_preview_error_and_budget_survive_without_cef_or_privilege_changes() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            crate::theme::Styles::default().apply(&ctx, theme);
            ctx.set_zoom_factor(zoom);
            let source = "<svg width=\"16\" height=\"8\"><text>unsupported</text></svg>".to_owned();
            let mut preview = Preview {
                retained: source.capacity(),
                source: source.clone(),
                pixels: None,
                error: Some("Unsupported SVG; Source is available.".into()),
                controls: [None; 3],
                keyboard_pass: None,
            };
            let lease = budget::Lease::new(&ctx);
            lease.reserve(preview.retained_bytes()).unwrap();
            let mut texture = None;
            let mut line = Some(1);
            let mut notice = None;
            for source_mode in [false, true] {
                for _ in 0..3 {
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(360.0, 650.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                preview.show(
                                    ui,
                                    Path::new("untrusted.svg"),
                                    source_mode,
                                    &mut texture,
                                    &mut line,
                                    &mut notice,
                                )
                            });
                        },
                    );
                    assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if if source_mode{text.galley.job.text==source}else{text.galley.job.text.contains("Unsupported SVG")})));
                }
            }
            assert_eq!(preview.source, source);
            assert!(texture.is_none());
        }
    }
}
#[test]
fn svg_source_and_preview_settle_without_reraster_or_idle_repaint() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let source = "<svg width='16' height='8'/>".to_owned();
    let mut preview = Preview {
        retained: source.capacity(),
        source,
        pixels: None,
        error: Some("Unsupported fixture".into()),
        controls: [None; 3],
        keyboard_pass: None,
    };
    let mut texture = None;
    let mut line = None;
    let mut notice = None;
    for source in [false, true] {
        for pass in 0..20 {
            let output = ctx.run(
                egui::RawInput {
                    time: Some(pass as f64 * 0.1 + if source { 3.0 } else { 0.0 }),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        preview.show(
                            ui,
                            Path::new("static.svg"),
                            source,
                            &mut texture,
                            &mut line,
                            &mut notice,
                        )
                    });
                },
            );
            if pass == 19 {
                assert_eq!(
                    output.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
                    Duration::MAX
                );
            }
        }
    }
}

#[test]
fn svg_pixels_transfer_once_premultiplied_and_keep_source_cpu_gpu_budget() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let source = "<svg width=\"2\" height=\"1\"/>".to_owned();
    let image = egui::ColorImage::from_rgba_premultiplied([2, 1], &[128, 0, 0, 128, 0, 0, 0, 0]);
    let retained = source.capacity() + image.pixels.capacity() * 8;
    let mut preview = Preview {
        source: source.clone(),
        pixels: Some(image),
        error: None,
        retained,
        controls: [None; 3],
        keyboard_pass: None,
    };
    let lease = budget::Lease::new(&ctx);
    lease.reserve(retained).unwrap();
    let other = budget::Lease::new(&ctx);
    let mut texture = None;
    let mut line = None;
    let mut notice = None;
    for _ in 0..3 {
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                preview.show(
                    ui,
                    Path::new("static.svg"),
                    false,
                    &mut texture,
                    &mut line,
                    &mut notice,
                )
            });
        });
    }
    assert!(preview.pixels.is_none());
    assert!(texture.is_some());
    assert_eq!(preview.source, source);
    assert_eq!(preview.retained_bytes(), retained);
    other.reserve(64 * 1024 * 1024 - retained).unwrap();
    assert!(other.reserve(64 * 1024 * 1024).is_err());
    drop(lease);
    other.reserve(64 * 1024 * 1024).unwrap();
}
