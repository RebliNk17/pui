use super::*;

fn labels(shape: &egui::Shape, result: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => result.push(text.galley.job.text.clone()),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| labels(shape, result)),
        _ => {}
    }
}
#[test]
fn reviewed_plan_and_commands_are_readable_inline_without_starting_work() {
    for plan_mode in [false, true] {
        for theme in [theme::Theme::Light, theme::Theme::Dark] {
            for (width, zoom) in [(360.0, 1.0), (760.0, 1.0), (480.0, 1.4)] {
                let mut app = app();
                if plan_mode {
                    app.prefs.settings.permission = settings::PermissionMode::Plan;
                }
                theme.apply(&app.ctx);
                app.ctx.set_zoom_factor(zoom);
                app.control_request(&planned_request("review"));
                let ctx = app.ctx.clone();
                let mut text = Vec::new();
                for _ in 0..3 {
                    text.clear();
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 1100.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| app.agent_question(ui));
                        },
                    );
                    output
                        .shapes
                        .iter()
                        .for_each(|shape| labels(&shape.shape, &mut text));
                }
                assert!(
                    text.iter()
                        .any(|text| text.contains("Inspect keyboard focus")),
                    "{theme:?}/{width}/{zoom}: {text:?}"
                );
                assert!(text.iter().any(|text| text.contains("cargo test keyboard")));
                assert!(text.iter().any(|text| text
                    == if plan_mode {
                        "Approve plan · switch to Manual"
                    } else {
                        "Use workflow"
                    }));
                if plan_mode {
                    assert!(app.active_permission() == settings::PermissionMode::Plan);
                }
                assert!(
                    app.tasks.records.is_empty()
                        && app.tasks.processes.is_empty()
                        && app.tasks.job.is_none()
                );
                app.workflow_answer(
                    &json!({"type":"extension_ui_response","id":"review","cancelled":true}),
                );
                assert!(app.tasks.records.is_empty());
            }
        }
    }
}

#[test]
fn request_changes_editor_uses_inline_surface_and_preserves_read_only_scope() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for (width, zoom) in [(360.0, 1.0), (760.0, 1.0), (480.0, 1.4)] {
            let mut app = app();
            app.prefs.settings.permission = settings::PermissionMode::Plan;
            theme.apply(&app.ctx);
            app.ctx.set_zoom_factor(zoom);
            app.control_request(&planned_request("revision-ui"));
            app.workflow_answer(&json!({"type":"extension_ui_response","id":"revision-ui","value":"Request changes"}));
            app.state
                .dialogs
                .retain(|dialog| dialog["id"] != "revision-ui");
            let ctx = app.ctx.clone();
            let mut text = vec![];
            for _ in 0..3 {
                text.clear();
                let out = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 1000.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| app.agent_question(ui));
                    },
                );
                out.shapes
                    .iter()
                    .for_each(|shape| labels(&shape.shape, &mut text));
            }
            assert!(text.iter().any(|text| text == "Request plan changes"));
            assert!(text.iter().any(|text| text == "Send answer"));
            assert!(
                text.iter()
                    .any(|text| text.contains("does not approve execution"))
            );
            assert!(app.active_permission() == settings::PermissionMode::Plan);
            assert!(app.tasks.records.is_empty());
        }
    }
}
