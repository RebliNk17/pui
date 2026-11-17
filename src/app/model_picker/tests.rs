use super::*;

#[test]
fn short_model_results_reserve_a_taller_viewport_bounded_by_available_height() {
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        for zoom in [1.0, 1.5, 2.0] {
            for anchor_top in [180.0, 700.0] {
                let ctx = egui::Context::default();
                let mut app = app(&ctx);
                live(&mut app);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                app.model_anchor.as_mut().unwrap().0 = egui::Rect::from_min_size(
                    egui::pos2(20.0, anchor_top),
                    egui::vec2(130.0, 28.0),
                );
                app.state.models = vec![protocol::Model {
                    id: "fixture".into(),
                    name: "Only model".into(),
                    provider: "test".into(),
                    context_window: 1000,
                    input_cost: 1.0,
                    output_cost: 2.0,
                }];
                app.model_picker_open = true;
                let mut output = None;
                for _ in 0..6 {
                    output = Some(ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(700.0, 800.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| app.model_picker(ctx),
                    ));
                }
                let popup = ctx
                    .read_response(egui::Id::new("model-picker-dropdown"))
                    .unwrap()
                    .rect;
                assert!(
                    ctx.screen_rect().expand(1.0).contains_rect(popup),
                    "{popup:?}"
                );
                assert!(
                    popup.height() <= anchor_top,
                    "{popup:?} exceeds {anchor_top}"
                );
                if anchor_top > 600.0 {
                    let (_, row) = texts(&output.unwrap())
                        .into_iter()
                        .find(|(s, _)| s == "Only model")
                        .unwrap();
                    assert!(
                        // Name text is inset slightly below the first row edge.
                        popup.bottom() - row.top() >= 220.0,
                        "{popup:?} / {row:?}"
                    );
                }
            }
        }
    }
}

pub(super) fn app(ctx: &egui::Context) -> Pui {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.model_anchor = Some((
        egui::Rect::from_min_size(egui::pos2(20.0, 700.0), egui::vec2(130.0, 28.0)),
        egui::LayerId::background(),
    ));
    app.effort_anchor = Some((
        egui::Rect::from_min_size(egui::pos2(165.0, 700.0), egui::vec2(80.0, 28.0)),
        egui::LayerId::background(),
    ));
    app.model_picker_focus = false;
    app
}
pub(super) fn live(
    app: &mut Pui,
) -> (
    std::sync::mpsc::Sender<rpc::Event>,
    std::sync::mpsc::Receiver<Value>,
) {
    app.demo = false;
    app.connected = true;
    app.loading_chat = false;
    let (client, events, requests) = rpc::Client::stub_with_records();
    app.client = Some(client);
    (events, requests)
}
pub(super) fn render(
    ctx: &egui::Context,
    app: &mut Pui,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let rect = ctx.screen_rect();
    let screen = if rect.is_finite() {
        rect
    } else {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 800.0))
    };
    ctx.run(
        egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        },
        |ctx| app.model_picker(ctx),
    )
}
fn labels(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(text) => out.push((
            text.galley.text().into(),
            text.galley.rect.translate(text.pos.to_vec2()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                labels(shape, out);
            }
        }
        _ => {}
    }
}
pub(super) fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    let mut out = Vec::new();
    for shape in &output.shapes {
        labels(&shape.shape, &mut out);
    }
    out
}
pub(super) fn position(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
    texts(output)
        .iter()
        .find(|(s, _)| s == text)
        .unwrap_or_else(|| panic!("Missing {text:?}: {:?}", texts(output)))
        .1
        .center()
}
fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
pub(super) fn click(ctx: &egui::Context, app: &mut Pui, pos: egui::Pos2) {
    render(ctx, app, pointer(pos, true));
    render(ctx, app, pointer(pos, false));
}
pub(super) fn key(key: egui::Key) -> Vec<egui::Event> {
    vec![egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]
}
pub(super) fn settle(ctx: &egui::Context, app: &mut Pui) -> egui::FullOutput {
    for _ in 0..5 {
        render(ctx, app, vec![]);
    }
    render(ctx, app, vec![])
}

#[test]
fn busy_chat_selects_model_without_restarting_or_optimistically_changing_pi_state() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (events, requests) = live(&mut app);
    app.state.busy = true;
    app.state.run_observed = true;
    app.state.models = vec![protocol::Model {
        id: "gemini".into(),
        name: "Gemini model".into(),
        provider: "fixture".into(),
        context_window: 600000,
        input_cost: 2.0,
        output_cost: 10.0,
    }];
    app.state.model = "old".into();
    app.state.levels = vec!["low".into(), "high".into()];
    app.draft = "keep draft".into();
    app.model_picker_open = true;
    let output = settle(&ctx, &mut app);
    click(&ctx, &mut app, position(&output, "Gemini model"));
    assert!(!app.model_picker_open);
    assert_eq!(app.state.model, "old");
    assert_eq!(app.state.levels.len(), 2);
    let sent = requests.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(sent["type"], "set_model");
    assert_eq!(sent["provider"], "fixture");
    assert_eq!(sent["modelId"], "gemini");
    assert!(requests.try_recv().is_err(), "No abort or message");
    events.send(rpc::Event::Record(json!({"type":"response","command":"set_model","success":true,"data":{"id":"gemini","name":"Gemini model","provider":"fixture"}}))).unwrap();
    app.poll_rpc();
    assert_eq!(app.state.model, "gemini");
    assert!(app.state.busy);
    assert!(app.state.levels.is_empty());
    assert_eq!(app.draft, "keep draft");
}

#[test]
fn effort_changes_during_running_and_pending_requests_only_after_pi_event() {
    for dark in [false, true] {
        for pending in [false, true] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            let (events, requests) = live(&mut app);
            app.state.busy = true;
            app.state.pending = pending;
            app.state.levels = vec!["low".into(), "high".into()];
            app.state.thinking = "high".into();
            app.draft = "unsent".into();
            app.effort_picker_open = true;
            let output = settle(&ctx, &mut app);
            click(&ctx, &mut app, position(&output, "Low"));
            let sent = requests.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(sent["type"], "set_thinking_level");
            assert_eq!(sent["level"], "low");
            assert!(requests.try_recv().is_err());
            assert_eq!(app.state.thinking, "high");
            assert!(!app.effort_picker_open);
            events
                .send(rpc::Event::Record(
                    json!({"type":"thinking_level_changed","level":"low"}),
                ))
                .unwrap();
            app.poll_rpc();
            assert_eq!(app.state.thinking, "low");
            assert!(app.state.busy);
            assert_eq!(app.state.pending, pending);
            assert_eq!(app.draft, "unsent");
        }
    }
}

#[test]
fn unavailable_controls_and_readonly_ultracode_never_send_commands() {
    for blocked in ["disconnected", "loading", "compacting", "demo"] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let (_, requests) = live(&mut app);
        app.connected = blocked != "disconnected";
        app.loading_chat = blocked == "loading";
        app.state.compacting = blocked == "compacting";
        app.demo = blocked == "demo";
        app.effort_picker_open = true;
        app.state.levels = vec!["low".into(), "high".into()];
        app.state.thinking = "high".into();
        let output = settle(&ctx, &mut app);
        click(&ctx, &mut app, position(&output, "Low"));
        assert!(requests.try_recv().is_err(), "Disabled when {blocked}");
        assert_eq!(app.state.thinking, "high");
    }
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (_, requests) = live(&mut app);
    let id = app.ensure_agent();
    app.agents.nodes.get_mut(&id).unwrap().access = "read-only".into();
    app.ultracode_ready = true;
    app.effort_picker_open = true;
    let output = settle(&ctx, &mut app);
    let popup = ctx
        .read_response(egui::Id::new("effort-picker-dropdown"))
        .unwrap();
    let pos = egui::pos2(popup.rect.right() - 22.0, position(&output, "Ultracode").y);
    click(&ctx, &mut app, pos);
    assert!(requests.try_recv().is_err());
    assert!(!app.ultracode);
}

#[test]
fn compact_popovers_keep_prices_and_supported_efforts_separate_in_both_themes_and_zoom() {
    for dark in [false, true] {
        for width in [320.0, 700.0] {
            for zoom in [1.0, 1.5] {
                let ctx = egui::Context::default();
                let mut app = app(&ctx);
                ctx.set_visuals(if dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
                ctx.set_zoom_factor(zoom);
                app.model_picker_open = true;
                // Very long names/providers must not push prices or checks outside the menu.
                app.state.models[0].name = "A very long model name that must be truncated".into();
                app.state.models[0].provider = "a-very-long-provider-name".into();
                app.state.model = app.state.models[0].id.clone();
                app.state.provider = app.state.models[0].provider.clone();
                app.state.models[0].input_cost = 0.2;
                app.state.models[0].output_cost = 20.0;
                let mut output = None;
                for _ in 0..6 {
                    output = Some(ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 850.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| app.model_picker(ctx),
                    ));
                }
                let output = output.unwrap();
                let text = texts(&output);
                assert!(
                    text.iter().any(|(s, _)| s == "$0.2 / $20"),
                    "Selected model still has prices"
                );
                assert!(!text.iter().any(|(s, _)| s == "Thinking effort"
                    || s == "Ultracode"
                    || s.contains("next model request")
                    || s.contains("Prices per")));
                let popup = ctx
                    .read_response(egui::Id::new("model-picker-dropdown"))
                    .unwrap();
                assert!(popup.rect.width() <= 322.0, "Model width: {:?}", popup.rect);
                for (label, rect) in &text {
                    assert!(
                        rect.right() <= ctx.screen_rect().right() + 1.0 && rect.left() >= -1.0,
                        "{label:?} at {width}/{zoom}: {rect:?}"
                    );
                }
                app.model_picker_open = false;
                app.effort_picker_open = true;
                app.state.levels = vec!["off".into(), "high".into()];
                let output = settle(&ctx, &mut app);
                let text = texts(&output);
                assert!(text.iter().any(|(s, _)| s == "Off"));
                assert!(text.iter().any(|(s, _)| s == "High"));
                assert!(!text.iter().any(|(s, _)| s == "Medium"
                    || s == "Extra high"
                    || s.contains("Max parallel")
                    || s.contains("Search models")
                    || s.contains("current response")));
                assert!(
                    ctx.read_response(egui::Id::new("effort-picker-dropdown"))
                        .unwrap()
                        .rect
                        .width()
                        <= 242.0
                );
                app.ultracode = true;
                let output = settle(&ctx, &mut app);
                assert!(
                    texts(&output)
                        .iter()
                        .any(|(s, _)| s == "Max parallel workers"),
                    "{width}/{zoom}: {:?}",
                    texts(&output)
                );
            }
        }
    }
}

#[test]
fn effort_arrows_enter_and_escape_work_without_touching_the_draft() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (_, requests) = live(&mut app);
    app.state.levels = vec!["low".into(), "high".into(), "xhigh".into()];
    app.state.thinking = "high".into();
    app.state.busy = true;
    app.draft = "keep me".into();
    app.effort_picker_open = true;
    app.effort_picker_focus = true;
    settle(&ctx, &mut app);
    render(&ctx, &mut app, key(egui::Key::ArrowDown));
    render(&ctx, &mut app, key(egui::Key::Enter));
    let sent = requests.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(sent["level"], "xhigh");
    assert!(requests.try_recv().is_err());
    app.effort_picker_open = true;
    settle(&ctx, &mut app);
    render(&ctx, &mut app, key(egui::Key::Escape));
    assert!(!app.effort_picker_open);
    assert!(app.state.busy);
    assert_eq!(app.draft, "keep me");
    assert!(requests.try_recv().is_err());
    assert!(ctx.memory(|m| m.has_focus(egui::Id::new("composer-effort"))));
}

#[test]
fn model_search_provider_filter_and_rejected_selection_preserve_authoritative_state() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (_, requests) = live(&mut app);
    app.state.model = "old".into();
    app.state.levels = vec!["high".into()];
    app.model_picker_open = true;
    app.model_query = "not a real model".into();
    let output = settle(&ctx, &mut app);
    assert!(
        texts(&output)
            .iter()
            .any(|(s, _)| s == "No matching models.")
    );
    app.model_query.clear();
    app.model_provider = "missing-provider".into();
    let output = settle(&ctx, &mut app);
    assert!(
        texts(&output)
            .iter()
            .any(|(s, _)| s == "No matching models.")
    );
    assert!(requests.try_recv().is_err());
    app.state
        .apply(json!({"type":"response","command":"set_model","success":false,"error":"Rejected"}));
    assert_eq!(app.state.model, "old");
    assert_eq!(app.state.levels, ["high"]);
}

#[test]
fn provider_selection_keeps_model_picker_open_without_sending_a_model_change() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let (_, requests) = live(&mut app);
        mode.apply(&ctx);
        app.state.models = ["alpha", "beta"]
            .into_iter()
            .map(|provider| protocol::Model {
                id: format!("{provider}-model"),
                name: format!("{provider} model"),
                provider: provider.into(),
                context_window: 1000,
                input_cost: 1.0,
                output_cost: 2.0,
            })
            .collect();
        app.draft = "preserve draft".into();
        app.model_picker_open = true;
        let output = settle(&ctx, &mut app);
        click(&ctx, &mut app, position(&output, "All providers"));
        let output = settle(&ctx, &mut app);
        click(&ctx, &mut app, position(&output, "beta"));
        assert!(
            app.model_picker_open,
            "Provider choice must not close model picker"
        );
        assert_eq!(app.model_provider, "beta");
        let output = settle(&ctx, &mut app);
        assert!(texts(&output).iter().any(|(text, _)| text == "beta model"));
        assert!(!texts(&output).iter().any(|(text, _)| text == "alpha model"));
        assert_eq!(app.draft, "preserve draft");
        assert!(requests.try_recv().is_err());
        click(&ctx, &mut app, egui::pos2(650.0, 750.0));
        assert!(
            !app.model_picker_open,
            "Outside click must still dismiss the parent"
        );
    }
}

#[test]
fn provider_is_search_trailing_with_centered_chevron_and_price_aligned_heading() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            for width in [280.0, 700.0] {
                let ctx = egui::Context::default();
                let mut app = app(&ctx);
                live(&mut app);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                app.model_anchor.as_mut().unwrap().0 =
                    egui::Rect::from_min_size(egui::pos2(10.0, 620.0), egui::vec2(130.0, 28.0));
                app.state.models = (0..16)
                    .map(|i| protocol::Model {
                        id: format!("model-{i}"),
                        name: format!("Model {i}"),
                        provider: if i % 2 == 0 {
                            "alpha"
                        } else {
                            "a-very-long-provider-name-for-truncation"
                        }
                        .into(),
                        context_window: 1000,
                        input_cost: 2.0,
                        output_cost: 10.0,
                    })
                    .collect();
                app.model_picker_open = true;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 760.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| app.model_picker(ctx),
                );
                drop(output);
                for provider in ["", "a-very-long-provider-name-for-truncation"] {
                    app.model_provider = provider.into();
                    let output = settle(&ctx, &mut app);
                    let rect = |name| {
                        ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new(name)))
                            .unwrap()
                    };
                    let search = rect("model-search-row");
                    let control = rect("model-provider-control");
                    let input = rect("model-search-input");
                    let chevron = ctx
                        .data(|d| d.get_temp::<egui::Pos2>(egui::Id::new("model-provider-chevron")))
                        .unwrap();
                    assert!(
                        search.expand(1.0).contains_rect(control),
                        "{search:?} {control:?}"
                    );
                    assert!(
                        input.right() <= control.left(),
                        "Search/provider must not overlap"
                    );
                    assert!(
                        (chevron.y - control.center().y).abs() < 0.6,
                        "Chevron must center in the provider hit target: {chevron:?} {control:?}"
                    );
                    assert!(
                        (control.right() - chevron.x - 9.0).abs() < 1.0,
                        "Chevron must retain trailing inset"
                    );
                    let heading = rect("model-price-heading");
                    let price = ctx
                        .data(|d| {
                            d.get_temp::<egui::Rect>(egui::Id::new(("model-price-column", 0usize)))
                        })
                        .unwrap();
                    assert!(
                        (heading.right() - price.right()).abs() < 0.6,
                        "Heading must align with prices, not checkmark: {heading:?} {price:?}"
                    );
                    let painted = texts(&output)
                        .into_iter()
                        .find(|(s, _)| s == "$2 / $10")
                        .unwrap()
                        .1;
                    assert!((heading.right() - painted.right()).abs() < 0.6);
                    let popup = ctx
                        .read_response(egui::Id::new("model-picker-dropdown"))
                        .unwrap()
                        .rect;
                    assert!(
                        ctx.screen_rect().expand(1.0).contains_rect(popup),
                        "{popup:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn composer_controls_open_exclusively_and_have_independent_names() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
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
    let model = ctx.read_response(egui::Id::new("composer-model")).unwrap();
    let effort = ctx.read_response(egui::Id::new("composer-effort")).unwrap();
    assert!(model.rect.right() <= effort.rect.left());
    for pressed in [true, false] {
        frame(&mut app, pointer(effort.rect.center(), pressed));
    }
    assert!(app.effort_picker_open);
    assert!(!app.model_picker_open);
    for pressed in [true, false] {
        frame(&mut app, pointer(model.rect.center(), pressed));
    }
    assert!(app.model_picker_open);
    assert!(!app.effort_picker_open);
    app.close_model_popovers();
    assert!(!app.model_picker_open && !app.effort_picker_open);
    let mut state = State {
        model: "test".into(),
        model_name: "Friendly model".into(),
        provider: "one".into(),
        thinking: "high".into(),
        ..Default::default()
    };
    let model = |provider: &str| protocol::Model {
        id: "test".into(),
        name: "Friendly model".into(),
        provider: provider.into(),
        context_window: 1000,
        input_cost: 1.0,
        output_cost: 2.0,
    };
    state.models = vec![model("one")];
    assert_eq!(model_label(&state), "Friendly model");
    assert_eq!(selected_label(&state), "Friendly model · High");
    state.models.push(model("two"));
    assert_eq!(model_label(&state), "Friendly model · one");
}
