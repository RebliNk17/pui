use super::*;

fn render(
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    width: f32,
) -> (egui::FullOutput, Vec<Response>, Vec<bool>) {
    let mut responses = vec![];
    let mut bodies = vec![false, false];
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(800.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(Rect::from_min_size(
                        ui.cursor().min,
                        Vec2::new(width, 500.0),
                    )),
                    |ui| {
                        ui.set_width(width);
                        for (i, body) in bodies.iter_mut().enumerate() {
                            responses.push(
                                Section::new("A very long unbroken title".repeat(20))
                                    .id_salt(i)
                                    .summary("Summary ".repeat(60))
                                    .icon(Icon::Folder)
                                    .status("Passed", theme::palette(ctx).green)
                                    .show(ui, |ui| {
                                        *body = true;
                                        ui.add(egui::Label::new("Body content").wrap());
                                    }),
                            );
                        }
                        assert!(
                            ui.min_rect().width() <= width + 1.0,
                            "Section expanded its parent: {:?}",
                            ui.min_rect()
                        );
                    },
                );
            });
        },
    );
    (output, responses, bodies)
}
fn click(ctx: &egui::Context, pos: egui::Pos2) {
    for pressed in [true, false] {
        render(
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            320.0,
        );
    }
}
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
#[test]
fn entire_header_toggles_with_pointer_and_keyboard_and_state_is_independent() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let (_, rows, bodies) = render(&ctx, vec![], 320.0);
    assert_eq!(bodies, vec![false, false]);
    assert!(rows[0].rect.height() >= 44.0);
    click(&ctx, rows[0].rect.left_center() + egui::vec2(90.0, 0.0));
    assert_eq!(render(&ctx, vec![], 320.0).2, vec![true, false]);
    assert_eq!(
        render(&ctx, vec![key(egui::Key::ArrowLeft)], 320.0).2,
        vec![false, false]
    );
    assert_eq!(
        render(&ctx, vec![key(egui::Key::ArrowRight)], 320.0).2,
        vec![true, false]
    );
    assert_eq!(
        render(&ctx, vec![key(egui::Key::Space)], 320.0).2,
        vec![false, false]
    );
    assert_eq!(
        render(&ctx, vec![key(egui::Key::Enter)], 320.0).2,
        vec![true, false]
    );
}
#[test]
fn long_headers_are_bounded_and_stable_in_narrow_and_wide_themes() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        theme.apply(&ctx);
        for width in [200.0, 280.0, 736.0] {
            for _ in 0..6 {
                let (out, rows, _) = render(&ctx, vec![], width);
                assert!(rows.iter().all(|r| r.rect.width() <= width + 1.0));
                for shape in out.shapes {
                    if let egui::Shape::Text(text) = shape.shape {
                        assert!(
                            text.galley.size().x <= width,
                            "text exceeded section: {}",
                            text.galley.job.text
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn failed_sections_can_default_open_and_hidden_content_is_not_executed() {
    let ctx = egui::Context::default();
    let mut body_calls = 0;
    for _ in 0..3 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                Section::new("Failed check")
                    .default_open(true)
                    .show(ui, |_| {
                        body_calls += 1;
                    });
                Section::new("Raw response")
                    .show(ui, |_| panic!("closed contents must not execute"));
            });
        });
    }
    assert_eq!(body_calls, 3);
}
