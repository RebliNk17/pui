use super::*;

#[test]
fn mirror_checkpoint_keeps_only_local_draft_not_remote_approval_or_run_state() {
    let mut state = State {
        busy: true,
        ..Default::default()
    };
    state
        .dialogs
        .push_back(json!({"method":"confirm", "title":"Allow dangerous action?", "id":"remote"}));
    let checkpoint = Checkpoint::capture(
        Some("/fixture/session".into()),
        "/fixture".into(),
        &state,
        "Local mirror draft",
        false,
    );
    assert!(checkpoint.notice.is_empty());
    let mut restored = State::default();
    let mut draft = String::new();
    checkpoint.apply(&mut restored, &mut draft);
    assert_eq!(draft, "Local mirror draft");
    assert!(restored.dialogs.is_empty());
    assert!(!restored.busy);
    assert!(!restored.pending);
}

#[test]
fn recovery_controls_render_in_both_themes_at_narrow_width_and_zoom() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        app.prefs.color_styles.apply(&ctx, theme);
        for zoom in [1.0, 1.5] {
            ctx.set_zoom_factor(zoom);
            app.state.restart_notice = "Interrupted while waiting for input or approval.".into();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(260.0, 500.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.restart_recovery(ui));
                },
            );
            assert!(!output.shapes.is_empty());
            assert!(
                !app.state.restart_notice.is_empty(),
                "Rendering must not dismiss or continue"
            );
            assert!(app.state.dialogs.is_empty());
            assert!(!app.state.pending);
        }
    }
}
