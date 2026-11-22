// Extra native interaction and bounded timeout lifecycle regressions.

#[test]
fn page_bidi_controls_and_control_bytes_are_visible_not_native_label_spoofing() {
    assert_eq!(
        visible_data("title\u{202e}Close\u{2066}\n\0"),
        "title\\u{202E}Close\\u{2066}\\u{000A}\\u{0000}"
    );
    assert_eq!(visible_data("שלום · public"), "שלום · public");
}

#[test]
fn inspection_reserves_native_page_space_without_covering_console_or_chat() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [240.0, 420.0, 700.0] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            let mut browser = BrowserView {
                inspection: Some(Editor::new("fixture", 0)),
                ..Default::default()
            };
            browser.options.entry("fixture".into()).or_default().console = true;
            for _ in 0..3 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width + 240.0, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::SidePanel::left("chat")
                            .exact_width(220.0)
                            .show(ctx, |ui| {
                                ui.label("Chat");
                            });
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let rect =
                                browser.viewport_with_state(ui, "fixture", PageState::default());
                            let (_, close) = ctx
                                .data(|d| {
                                    d.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new((
                                        "inspection-control",
                                        "Close",
                                    )))
                                })
                                .unwrap();
                            let console = ctx
                                .data(|d| {
                                    d.get_temp::<egui::Rect>(egui::Id::new((
                                        "browser-control",
                                        "Hide console",
                                    )))
                                })
                                .unwrap();
                            assert!(rect.is_finite() && rect.is_positive());
                            assert!(rect.top() >= close.bottom(), "native page covers inspector");
                            assert!(rect.left() >= 220.0);
                            assert!(rect.bottom() < console.top());
                        });
                    },
                );
            }
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn full_app_inspector_keys_preserve_agent_draft_and_pending_approval_without_cef() {
    use crate::app::files::tests::{app, click, frame, text};
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.prefs.theme = mode;
        app.state.busy = true;
        app.draft = "Keep inspector draft".into();
        let messages = app.state.messages.len();
        let root = app.prefs.cwd.clone();
        app.files
            .open_target(&ctx, &root, "http://localhost:3000/inspection-fixture")
            .unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let pos = ctx
            .data(|d| {
                d.get_temp::<egui::Rect>(egui::Id::new(("browser-control", "Inspect an element")))
            })
            .unwrap()
            .center();
        click(&ctx, &mut app, pos);
        let output = frame(&ctx, &mut app, vec![]);
        text(&output, "Element inspection");
        text(&output, "Inspection unavailable");
        app.state.dialogs.push_back(json!({"id":"inspection-approval", "method":"confirm", "title":"Allow bash?", "message":"Run fixture\n\n{}\n\nApprove this call only."}));
        for event in [
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
            key(egui::Key::Escape, egui::Modifiers::NONE),
        ] {
            frame(&ctx, &mut app, vec![event.clone()]);
            assert_eq!(
                app.state.dialogs.len(),
                1,
                "inspector key leaked: {event:?}"
            );
            assert!(app.state.busy);
            assert_eq!(app.draft, "Keep inspector draft");
            assert_eq!(app.state.messages.len(), messages);
        }
        assert_eq!(
            app.files.navigation_target().as_deref(),
            Some("http://localhost:3000/inspection-fixture")
        );
    }
}

#[test]
fn explicit_buttons_keyboard_and_disabled_panel_never_leak_activation() {
    let mut panel = Panel::new(theme::Theme::Light, 360.0, 1.0);
    panel.frame(vec![], false);
    panel.frame(vec![], false);
    assert_eq!(panel.click("Inspect").inspect.as_deref(), Some("h1"));
    let inspect = panel.control("Inspect").0;
    panel.ctx.memory_mut(|m| m.request_focus(inspect));
    panel.frame(vec![], false);
    let (decision, remaining) =
        panel.frame(vec![key(egui::Key::Enter, egui::Modifiers::NONE)], false);
    assert_eq!(decision.inspect.as_deref(), Some("h1"));
    assert!(!remaining.iter().any(|event| matches!(
        event,
        egui::Event::Key {
            key: egui::Key::Enter,
            ..
        }
    )));
    panel.enabled = false;
    panel.frame(vec![], false);
    assert!(panel.click("Inspect").inspect.is_none());
    panel.enabled = true;
    panel.frame(vec![], false);
    assert!(panel.click("Close").close);
}

#[cfg(target_os = "macos")]
#[test]
fn whole_operation_timeout_and_cancel_do_not_expire_a_later_query() {
    let mut state = State::default();
    let id = start(&mut state);
    let operation = state.operation;
    state.timeout(operation);
    assert!(!state.pending());
    assert!(state.message.as_ref().unwrap().contains("timed out"));
    receive(&mut state, id, true, b"{}");
    assert!(state.facts.is_none());
    start(&mut state);
    state.timeout(operation);
    assert!(state.pending());
    state.cancel(None);
    assert!(!state.pending());
    state.timeout(state.operation);
    assert!(state.message.is_none());
}
