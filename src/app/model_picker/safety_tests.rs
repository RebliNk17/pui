use super::tests::{app, key, live, position, render, settle, texts};
use super::*;

#[test]
fn full_app_escape_dismisses_effort_before_stop_and_approval_shortcuts() {
    for approval in [false, true] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let (_, requests) = live(&mut app);
        app.state.busy = true;
        app.state.run_observed = true;
        app.state.levels = vec!["low".into(), "high".into()];
        app.draft = "do not send".into();
        if approval {
            app.state.dialogs.push_back(json!({"type":"extension_ui_request","id":"permission","method":"confirm","title":"Allow bash?","message":"Inspect workspace\n{\"command\":\"pwd\"}\n\nApprove this call only."}));
        }
        let mut frame = eframe::Frame::_new_kittest();
        let run = |app: &mut Pui, frame: &mut eframe::Frame, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 900.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(app, ctx, frame),
            )
        };
        for _ in 0..4 {
            run(&mut app, &mut frame, vec![]);
        }
        // Drain startup/read-only metadata requests; user actions below must not prompt or abort.
        while requests.try_recv().is_ok() {}
        app.effort_picker_open = true;
        app.effort_picker_focus = true;
        for _ in 0..4 {
            run(&mut app, &mut frame, vec![]);
        }
        if approval {
            let mut enter = key(egui::Key::Enter);
            if let egui::Event::Key { modifiers, .. } = &mut enter[0] {
                *modifiers = egui::Modifiers::COMMAND;
            }
            run(&mut app, &mut frame, enter);
            assert_eq!(app.state.dialogs.len(), 1);
        }
        run(&mut app, &mut frame, key(egui::Key::Escape));
        assert!(!app.effort_picker_open);
        assert!(app.state.busy);
        assert_eq!(app.draft, "do not send");
        assert_eq!(app.state.dialogs.len(), usize::from(approval));
        for record in requests.try_iter() {
            assert!(
                !matches!(
                    record["type"].as_str(),
                    Some("abort" | "prompt" | "extension_ui_response")
                ),
                "Unexpected action: {record}"
            );
        }
    }
}

#[test]
fn effort_selection_rejection_does_not_change_current_label_and_can_be_retried() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (_, requests) = live(&mut app);
    app.state.levels = vec!["low".into(), "high".into()];
    app.state.thinking = "high".into();
    app.state.busy = true;
    app.effort_picker_open = true;
    let output = settle(&ctx, &mut app);
    super::tests::click(&ctx, &mut app, position(&output, "Low"));
    assert_eq!(
        requests.recv_timeout(Duration::from_secs(1)).unwrap()["level"],
        "low"
    );
    app.state.apply(json!({"type":"response","command":"set_thinking_level","success":false,"error":"Not available"}));
    assert_eq!(app.state.thinking, "high");
    assert!(app.state.busy);
    app.effort_picker_open = true;
    let output = settle(&ctx, &mut app);
    assert!(texts(&output).iter().any(|(s, _)| s == "Low"));
}

#[test]
fn outside_click_closes_effort_without_a_message_and_layout_change_closes_both() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (_, requests) = live(&mut app);
    app.effort_picker_open = true;
    settle(&ctx, &mut app);
    super::tests::click(&ctx, &mut app, egui::pos2(8.0, 8.0));
    assert!(!app.effort_picker_open);
    assert!(requests.try_recv().is_err());
    app.effort_picker_open = true;
    app.model_picker_open = true;
    app.leave_session_layout();
    assert!(!app.effort_picker_open && !app.model_picker_open);
}

#[test]
fn keyboard_open_does_not_select_and_arrows_step_once_through_middle_efforts() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let (_, requests) = live(&mut app);
        app.state.levels = vec!["low".into(), "medium".into(), "high".into(), "xhigh".into()];
        app.state.thinking = "low".into();
        let frame = |app: &mut Pui, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700.0, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.add_space(600.0);
                        ui.horizontal(|ui| {
                            app.model_button(ui);
                            app.effort_button(ui);
                        });
                    });
                    app.model_picker(ctx);
                },
            )
        };
        for _ in 0..4 {
            frame(&mut app, vec![]);
        }
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("composer-effort")));
        frame(&mut app, vec![]);
        frame(&mut app, key(activation));
        assert!(
            app.effort_picker_open,
            "{activation:?} should open without selecting"
        );
        assert!(requests.try_recv().is_err());
        // No settling frames: focus filters must transfer with each consecutive arrow.
        frame(&mut app, key(egui::Key::ArrowDown));
        frame(&mut app, key(egui::Key::ArrowDown));
        frame(&mut app, key(egui::Key::Enter));
        assert_eq!(
            requests.recv_timeout(Duration::from_secs(1)).unwrap()["level"],
            "high"
        );
        assert!(requests.try_recv().is_err());
    }
}

#[test]
fn ultracode_row_label_and_switch_share_the_effort_row_alignment() {
    for dark in [false, true] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        live(&mut app);
        ctx.set_visuals(if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        app.effort_picker_open = true;
        app.state.levels = vec!["low".into(), "high".into()];
        let output = settle(&ctx, &mut app);
        let text = texts(&output);
        let label = text.iter().find(|(s, _)| s == "Ultracode").unwrap().1;
        let low = text.iter().find(|(s, _)| s == "Low").unwrap().1;
        assert!(
            (label.left() - low.left()).abs() < 0.1,
            "Label inset must match effort options"
        );
        fn switches(shape: &egui::Shape, out: &mut Vec<egui::Rect>) {
            match shape {
                egui::Shape::Rect(r)
                    if (r.rect.width() - 30.0).abs() < 0.1
                        && (r.rect.height() - 18.0).abs() < 0.1 =>
                {
                    out.push(r.rect)
                }
                egui::Shape::Vec(shapes) => {
                    for s in shapes {
                        switches(s, out);
                    }
                }
                _ => {}
            }
        }
        let mut tracks = Vec::new();
        for shape in &output.shapes {
            switches(&shape.shape, &mut tracks);
        }
        let track = tracks.first().unwrap();
        assert!(
            (label.center().y - track.center().y).abs() < 0.6,
            "Label and switch must be centered: {label:?}/{track:?}"
        );
        let popup = ctx
            .read_response(egui::Id::new("effort-picker-dropdown"))
            .unwrap()
            .rect;
        assert!(
            (popup.right() - track.right() - 13.0).abs() < 2.0,
            "Shared right inset"
        );
    }
}

#[test]
fn keyboard_search_navigates_and_selects_a_model_from_the_full_catalog() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (_, requests) = live(&mut app);
    app.state.busy = true;
    app.state.models = (0..30)
        .map(|i| protocol::Model {
            id: format!("model-{i}"),
            name: format!("Model {i}"),
            provider: "fixture".into(),
            context_window: 600000,
            input_cost: 2.0,
            output_cost: 10.0,
        })
        .collect();
    app.state.model = "model-29".into();
    app.state.provider = "fixture".into();
    app.model_picker_open = true;
    app.model_picker_focus = true;
    settle(&ctx, &mut app);
    render(&ctx, &mut app, key(egui::Key::ArrowDown));
    settle(&ctx, &mut app);
    render(&ctx, &mut app, key(egui::Key::Enter));
    let sent = requests.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(sent["type"], "set_model");
    assert_eq!(sent["modelId"], "model-29");
    assert!(requests.try_recv().is_err());
}

#[test]
fn ultracode_toggle_waits_for_adapter_and_worker_limit_is_conditional() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (_, requests) = live(&mut app);
    app.effort_picker_open = true;
    app.ultracode_ready = true;
    let output = settle(&ctx, &mut app);
    let popup = ctx
        .read_response(egui::Id::new("effort-picker-dropdown"))
        .unwrap();
    let pos = egui::pos2(popup.rect.right() - 22.0, position(&output, "Ultracode").y);
    super::tests::click(&ctx, &mut app, pos);
    assert_eq!(
        requests.recv_timeout(Duration::from_secs(1)).unwrap()["message"],
        "/pui-ultracode on"
    );
    assert!(!app.ultracode, "No optimistic mode change");
    assert!(
        !texts(&settle(&ctx, &mut app))
            .iter()
            .any(|(s, _)| s.contains("Max parallel"))
    );
    app.ultracode = true;
    assert!(
        texts(&settle(&ctx, &mut app))
            .iter()
            .any(|(s, _)| s == "Max parallel workers")
    );
    app.state.busy = true;
    let output = settle(&ctx, &mut app);
    let pos = egui::pos2(popup.rect.right() - 22.0, position(&output, "Ultracode").y);
    super::tests::click(&ctx, &mut app, pos);
    assert!(requests.try_recv().is_err());
    render(&ctx, &mut app, key(egui::Key::Escape));
    assert!(app.state.busy);
}
