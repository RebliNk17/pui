use super::*;

#[test]
fn prompts_popovers_and_actions_follow_every_selected_palette() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let styles = theme::DarkStyle::ALL
        .into_iter()
        .map(|dark| {
            (
                theme::Theme::Dark,
                theme::Styles {
                    dark,
                    ..Default::default()
                },
            )
        })
        .chain(theme::LightStyle::ALL.into_iter().map(|light| {
            (
                theme::Theme::Light,
                theme::Styles {
                    light,
                    ..Default::default()
                },
            )
        }));
    for (mode, style) in styles {
        style.apply(&ctx, mode);
        let colors = theme::palette(&ctx);
        assert_eq!(background(&ctx), colors.surface);
        assert_eq!(subtle(&ctx), colors.bg);
        assert_eq!(border(&ctx), colors.border);
        for frame in [surface(&ctx), inline(&ctx), popup(&ctx)] {
            assert_eq!(frame.fill, colors.surface);
            assert_eq!(frame.stroke.color, colors.border);
        }
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                button(ui, "Continue", Button::Primary, true);
                button(ui, "Delete", Button::Danger, true);
                let disabled = button(ui, "Unavailable", Button::Primary, false);
                assert!(!disabled.enabled());
                assert!(!disabled.clicked());
            });
        });
        for color in [colors.accent, colors.red] {
            assert!(
                output
                    .shapes
                    .iter()
                    .any(|s| { matches!(&s.shape, egui::Shape::Rect(rect) if rect.fill == color) }),
                "Actions must use the active palette"
            );
        }
        for s in &output.shapes {
            if let egui::Shape::Text(text) = &s.shape {
                if ["Continue", "Delete"].contains(&text.galley.job.text.as_str()) {
                    assert_eq!(text.galley.job.sections[0].format.color, colors.bg);
                }
                if text.galley.job.text == "Unavailable" {
                    assert_ne!(text.galley.job.sections[0].format.color, colors.bg);
                }
            }
        }
    }
}

#[test]
fn menu_density_is_local_and_does_not_shrink_prompt_controls() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            style(ui);
            assert_eq!(ui.spacing().interact_size.y, 32.0);
            ui.scope(|ui| {
                theme::menu_controls(ui);
                let first = ui.button("First").rect;
                let second = ui.button("Second").rect;
                assert!(first.height() <= 24.0);
                assert!(second.top() - first.bottom() <= 2.0);
            });
            assert_eq!(ui.spacing().interact_size.y, 32.0);
            assert_eq!(ui.spacing().item_spacing.y, 8.0);
        });
    });
}
