use super::*;

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn integer_css_dimensions_reject_invalid_overflow_and_excessive_area() {
    for (width, height) in [
        ("240", "240"),
        ("3840", "2160"),
        ("2160", "3840"),
        (" 1200 ", "800"),
    ] {
        assert!(dimensions(width, height).is_ok(), "{width} × {height}");
    }
    for (width, height) in [
        ("", "800"),
        ("239", "800"),
        ("3841", "800"),
        ("+390", "844"),
        ("-1", "844"),
        ("390.0", "844"),
        ("1e3", "844"),
        ("３９０", "844"),
        ("390px", "844"),
        ("4294967296", "844"),
        ("99999999999999999999", "844"),
        ("3840", "3840"),
        ("390", "0"),
        ("390", "NaN"),
    ] {
        assert!(dimensions(width, height).is_err(), "{width} × {height}");
    }
    for width in [0, MIN_SIDE, 1440, MAX_SIDE, u32::MAX] {
        for height in [0, MIN_SIDE, 900, MAX_SIDE, u32::MAX] {
            let (width, height) = bounded_dimensions(width, height);
            assert!((MIN_SIDE..=MAX_SIDE).contains(&width));
            assert!((MIN_SIDE..=MAX_SIDE).contains(&height));
            assert!(width.checked_mul(height).unwrap() <= MAX_AREA);
        }
    }
}

#[test]
fn explicit_custom_config_uses_same_bounds_and_invalid_config_leaves_options_unchanged() {
    let mut browser = BrowserView::default();
    for device in ["responsive", "desktop", "tablet", "mobile"] {
        browser.configure("fixture", device, true).unwrap();
    }
    browser
        .configure("fixture", "custom:1200x800", false)
        .unwrap();
    assert_eq!(browser.options["fixture"].dimensions(), Some((1200, 800)));
    browser
        .configure("fixture", "custom:3840x2160", true)
        .unwrap();
    assert_eq!(browser.options["fixture"].dimensions(), Some((3840, 2160)));
    let original = browser.options["fixture"].clone();
    for device in [
        "custom:3840x3840",
        "custom:239x844",
        "custom:4294967296x800",
        "custom:390X844",
        "custom:390.0x844",
        "custom:",
        "other",
    ] {
        assert!(
            browser.configure("fixture", device, false).is_err(),
            "{device}"
        );
        assert_eq!(browser.options["fixture"], original);
        assert!(browser.configure("new", device, false).is_err());
        assert!(!browser.options.contains_key("new"));
    }
}

#[test]
fn drafts_are_atomic_preserve_preferences_and_do_not_change_options_until_applied() {
    let mut options = Options {
        device: Device::Mobile,
        rotate: true,
        fit: false,
        zoom: 1.5,
        console: true,
        ..Default::default()
    };
    let original = options.clone();
    let mut editor = Editor::new("fixture", &options);
    assert_eq!((&*editor.width, &*editor.height), ("844", "390"));
    editor.width = "1200".into();
    editor.height = "invalid".into();
    assert!(!editor.apply(&mut options, true));
    assert_eq!(options, original);
    editor.height = "800".into();
    assert!(!editor.apply(&mut options, false));
    assert_eq!(options, original);
    assert!(editor.apply(&mut options, true));
    assert_eq!(options.device, Device::Custom);
    assert_eq!(options.dimensions(), Some((1200, 800)));
    assert!(!options.rotate);
    assert!(!options.fit);
    assert_eq!(options.zoom, 1.5);
    assert!(options.console);
    assert!(
        !options.mobile(),
        "Custom size must not imply a mobile user agent"
    );
}

struct Form {
    ctx: egui::Context,
    options: Options,
    editor: Option<Editor>,
    width: f32,
    enabled: bool,
    available: bool,
}
impl Form {
    fn new(mode: theme::Theme, width: f32, zoom: f32) -> Self {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        mode.apply(&ctx);
        ctx.set_zoom_factor(zoom);
        let options = Options::default();
        let editor = Some(Editor::new("fixture", &options));
        Self {
            ctx,
            options,
            editor,
            width,
            enabled: true,
            available: true,
        }
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(self.width, 540.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_max_width(ui.available_width().min(270.0));
                    ui.add_enabled_ui(self.enabled, |ui| {
                        settings(
                            ui,
                            "fixture",
                            &mut self.options,
                            &mut self.editor,
                            self.available,
                        );
                    });
                });
            },
        )
    }

    fn control(&self, label: &str) -> (egui::Id, egui::Rect) {
        self.ctx
            .data(|data| data.get_temp(egui::Id::new(("custom-viewport", label))))
            .unwrap()
    }

    fn click(&mut self, label: &str) {
        let pos = self.control(label).1.center();
        for pressed in [true, false] {
            self.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
    }
}

#[test]
fn form_fits_light_dark_narrow_zoom_and_validation_errors() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [180.0, 240.0, 420.0] {
            for zoom in [0.8, 1.0, 1.6] {
                for invalid in [false, true] {
                    let mut form = Form::new(mode, width, zoom);
                    if invalid {
                        form.editor.as_mut().unwrap().width = "4294967296".into();
                    }
                    for _ in 0..3 {
                        form.frame(vec![]);
                    }
                    for label in ["Width", "Height", "Apply", "Cancel"] {
                        let rect = form.control(label).1;
                        assert!(rect.is_positive());
                        assert!(
                            form.ctx.screen_rect().contains_rect(rect),
                            "{mode:?} {width} {zoom} {label}: {rect:?} / {:?}",
                            form.ctx.screen_rect()
                        );
                    }
                    let apply = form.ctx.read_response(form.control("Apply").0).unwrap();
                    assert_eq!(apply.enabled(), !invalid);
                }
            }
        }
    }
}

#[test]
fn typing_does_not_resize_and_plain_enter_or_apply_commit_once() {
    for by_click in [false, true] {
        let mut form = Form::new(theme::Theme::Light, 350.0, 1.0);
        for _ in 0..3 {
            form.frame(vec![]);
        }
        let original = form.options.clone();
        // Exercise actual TextEdit events, including the size limit on pasted input.
        let width_id = form.control("Width").0;
        form.ctx.memory_mut(|memory| memory.request_focus(width_id));
        form.editor.as_mut().unwrap().width.clear();
        form.frame(vec![egui::Event::Text("1200".into())]);
        form.editor.as_mut().unwrap().height = "800".into();
        form.frame(vec![]);
        assert_eq!(form.options, original);
        if by_click {
            form.click("Apply");
        } else {
            form.frame(vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
        }
        assert!(form.editor.is_none());
        assert_eq!(form.options.dimensions(), Some((1200, 800)));
        let applied = form.options.clone();
        form.frame(vec![]);
        assert_eq!(form.options, applied);
    }
    let mut form = Form::new(theme::Theme::Dark, 350.0, 1.0);
    form.frame(vec![]);
    form.editor.as_mut().unwrap().width.clear();
    form.frame(vec![egui::Event::Text(
        "123456789012345678901234567890".into(),
    )]);
    assert!(form.editor.as_ref().unwrap().width.chars().count() <= FIELD_LIMIT);
    assert_eq!(form.options.device, Device::Responsive);
}

#[test]
fn escape_cancel_invalid_and_disabled_paths_never_apply() {
    for (enabled, available, invalid, cancel) in [
        (true, true, false, "Escape"),
        (true, true, false, "Cancel"),
        (true, true, true, "Enter"),
        (false, true, false, "Enter"),
        (true, false, false, "Enter"),
        (true, false, false, "Apply"),
    ] {
        let mut form = Form::new(theme::Theme::Dark, 350.0, 1.0);
        form.enabled = enabled;
        form.available = available;
        form.editor.as_mut().unwrap().width = if invalid { "bad" } else { "1200" }.into();
        let original = form.options.clone();
        for _ in 0..3 {
            form.frame(vec![]);
        }
        if matches!(cancel, "Cancel" | "Apply") {
            form.click(cancel);
        } else {
            form.frame(vec![key(
                if cancel == "Escape" {
                    egui::Key::Escape
                } else {
                    egui::Key::Enter
                },
                egui::Modifiers::NONE,
            )]);
        }
        assert_eq!(form.options, original);
        assert_eq!(form.editor.is_none(), matches!(cancel, "Escape" | "Cancel"));
    }
}

#[test]
fn editor_consumes_send_stop_approval_and_denial_keys_without_modified_enter_apply() {
    for (key_value, modifiers) in [
        (egui::Key::Enter, egui::Modifiers::COMMAND),
        (egui::Key::Enter, egui::Modifiers::SHIFT),
        (egui::Key::Backspace, egui::Modifiers::COMMAND),
        (egui::Key::Escape, egui::Modifiers::NONE),
    ] {
        let mut form = Form::new(theme::Theme::Dark, 350.0, 1.0);
        form.frame(vec![]);
        let original = form.options.clone();
        form.frame(vec![key(key_value, modifiers)]);
        assert_eq!(form.options, original);
        assert!(
            !form.ctx.input(|input| input.key_pressed(key_value)),
            "{key_value:?} leaked"
        );
    }
}

fn toolbar_frame(ctx: &egui::Context, browser: &mut BrowserView, events: Vec<egui::Event>) {
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(350.0, 620.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::TopBottomPanel::top("toolbar")
                .exact_height(42.0)
                .show(ctx, |ui| browser.toolbar(ui, "fixture"));
            egui::CentralPanel::default().show(ctx, |ui| {
                browser.viewport(ui, "fixture");
            });
        },
    );
}

fn toolbar_click(ctx: &egui::Context, browser: &mut BrowserView, pos: egui::Pos2) {
    for pressed in [true, false] {
        toolbar_frame(
            ctx,
            browser,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn actual_popup_discards_on_outside_click_reopen_tab_switch_and_close() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut browser = BrowserView::default();
    for _ in 0..3 {
        toolbar_frame(&ctx, &mut browser, vec![]);
    }
    let size = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("browser-control", "Auto"))))
        .unwrap();
    toolbar_click(&ctx, &mut browser, size.center());
    toolbar_frame(&ctx, &mut browser, vec![]);
    let custom = ctx
        .data(|data| {
            data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new(("custom-viewport", "Open")))
        })
        .unwrap();
    toolbar_click(&ctx, &mut browser, custom.1.center());
    toolbar_frame(&ctx, &mut browser, vec![]);
    assert!(browser.viewport_editor.is_some());
    browser.viewport_editor.as_mut().unwrap().width = "1200".into();
    toolbar_click(&ctx, &mut browser, egui::pos2(330.0, 590.0));
    toolbar_frame(&ctx, &mut browser, vec![]);
    assert!(browser.viewport_editor.is_none());
    assert_eq!(browser.options["fixture"].device, Device::Responsive);
    toolbar_click(&ctx, &mut browser, size.center());
    for _ in 0..3 {
        toolbar_frame(&ctx, &mut browser, vec![]);
    }
    let custom = ctx
        .data(|data| {
            data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new(("custom-viewport", "Open")))
        })
        .unwrap();
    toolbar_click(&ctx, &mut browser, custom.1.center());
    toolbar_frame(&ctx, &mut browser, vec![]);
    assert_eq!(browser.viewport_editor.as_ref().unwrap().width, "390");
    browser.close("fixture");
    assert!(browser.viewport_editor.is_none());
    browser.viewport_editor = Some(Editor::new("other", &Options::default()));
    toolbar_frame(&ctx, &mut browser, vec![]);
    assert!(browser.viewport_editor.is_none());
    browser.viewport_editor = Some(Editor::new("fixture", &Options::default()));
    browser.clear();
    assert!(browser.viewport_editor.is_none());
}

#[test]
fn popup_and_controls_stay_in_screen_at_narrow_widths_and_zoom() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [180.0, 280.0, 420.0] {
            for zoom in [0.8, 1.0, 1.6] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let mut browser = BrowserView::default();
                let draw = |browser: &mut BrowserView, events| {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(width, 600.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| {
                            egui::TopBottomPanel::top("toolbar")
                                .exact_height(42.0)
                                .show(ctx, |ui| browser.toolbar(ui, "fixture"));
                        },
                    );
                };
                for _ in 0..3 {
                    draw(&mut browser, vec![]);
                }
                for label in ["Auto", "Show console", "100%"] {
                    let rect = ctx
                        .data(|data| {
                            data.get_temp::<egui::Rect>(egui::Id::new(("browser-control", label)))
                        })
                        .unwrap();
                    assert!(
                        ctx.screen_rect().contains_rect(rect),
                        "{mode:?} {width} {zoom} {label}: {rect:?}"
                    );
                }
                let click = |browser: &mut BrowserView, pos| {
                    for pressed in [true, false] {
                        draw(
                            browser,
                            vec![
                                egui::Event::PointerMoved(pos),
                                egui::Event::PointerButton {
                                    pos,
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                        );
                    }
                };
                let size = ctx
                    .data(|data| {
                        data.get_temp::<egui::Rect>(egui::Id::new(("browser-control", "Auto")))
                    })
                    .unwrap();
                click(&mut browser, size.center());
                for _ in 0..3 {
                    draw(&mut browser, vec![]);
                }
                let custom = ctx
                    .data(|data| {
                        data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new((
                            "custom-viewport",
                            "Open",
                        )))
                    })
                    .unwrap();
                assert!(
                    ctx.screen_rect().contains_rect(custom.1),
                    "{width} {zoom}: {:?}",
                    custom.1
                );
                click(&mut browser, custom.1.center());
                for _ in 0..3 {
                    draw(&mut browser, vec![]);
                }
                assert!(browser.viewport_editor.is_some());
                for label in ["Width", "Height", "Apply", "Cancel"] {
                    let (id, rect) = ctx
                        .data(|data| {
                            data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new((
                                "custom-viewport",
                                label,
                            )))
                        })
                        .unwrap();
                    assert!(
                        ctx.screen_rect().contains_rect(rect),
                        "{width} {zoom} {label}: {rect:?}"
                    );
                    assert!(
                        ctx.read_response(id).unwrap().interact_rect.is_positive(),
                        "Clipped {label}"
                    );
                }
            }
        }
    }
}

#[test]
fn cancel_focus_enter_discards_instead_of_applying_and_cancel_wins_simultaneous_keys() {
    let mut form = Form::new(theme::Theme::Dark, 350.0, 1.0);
    for _ in 0..3 {
        form.frame(vec![]);
    }
    let cancel = form.control("Cancel").0;
    form.ctx.memory_mut(|memory| memory.request_focus(cancel));
    form.frame(vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    assert!(form.editor.is_none());
    assert_eq!(form.options.device, Device::Responsive);
    form.editor = Some(Editor::new("fixture", &form.options));
    form.frame(vec![
        key(egui::Key::Enter, egui::Modifiers::NONE),
        key(egui::Key::Escape, egui::Modifiers::NONE),
    ]);
    assert!(form.editor.is_none());
    assert_eq!(form.options.device, Device::Responsive);
}

#[test]
fn focused_action_space_consumes_activation_text_and_disabled_apply_stays_disabled() {
    for (label, available) in [("Apply", true), ("Cancel", true), ("Apply", false)] {
        let mut form = Form::new(theme::Theme::Dark, 350.0, 1.0);
        form.available = available;
        for _ in 0..3 {
            form.frame(vec![]);
        }
        let id = form.control(label).0;
        form.ctx.memory_mut(|memory| memory.request_focus(id));
        form.frame(vec![
            key(egui::Key::Space, egui::Modifiers::NONE),
            egui::Event::Text(" ".into()),
        ]);
        assert!(
            !form.ctx.input(|input| input.key_pressed(egui::Key::Space)),
            "{label} {available} focus={:?}",
            form.ctx.memory(|memory| memory.focused())
        );
        assert!(!form.ctx.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Text(text) if text == " "))
        }));
        assert_eq!(
            form.options.device == Device::Custom,
            label == "Apply" && available
        );
        assert_eq!(form.editor.is_none(), available);
    }
}

#[test]
fn maximum_applied_label_fits_near_compact_breakpoint_in_both_orientations() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [280.0, 300.0, 320.0, 340.0, 360.0] {
            for rotate in [false, true] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(1.6);
                let mut browser = BrowserView::default();
                browser.options.insert(
                    "fixture".into(),
                    Options {
                        device: Device::Custom,
                        width: 3840,
                        height: 2160,
                        rotate,
                        ..Default::default()
                    },
                );
                for _ in 0..3 {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(width, 600.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::TopBottomPanel::top("toolbar")
                                .exact_height(42.0)
                                .show(ctx, |ui| browser.toolbar(ui, "fixture"));
                        },
                    );
                }
                let label = if rotate {
                    "2160 × 3840"
                } else {
                    "3840 × 2160"
                };
                let rect = |label| {
                    ctx.data(|data| {
                        data.get_temp::<egui::Rect>(egui::Id::new(("browser-control", label)))
                    })
                    .unwrap()
                };
                for label in [label, "100%", "Show console"] {
                    assert!(
                        ctx.screen_rect().contains_rect(rect(label)),
                        "{width} {label}: {:?}",
                        rect(label)
                    );
                }
                assert!(
                    rect(label).right() < rect("100%").left(),
                    "{width}: custom label overlaps zoom"
                );
            }
        }
    }
}

#[test]
fn maximum_custom_geometry_excludes_chat_status_console_at_narrow_zoom() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [180.0, 320.0, 650.0] {
            for zoom in [0.8, 1.0, 1.6] {
                for fit in [false, true] {
                    let ctx = egui::Context::default();
                    theme::install(&ctx);
                    mode.apply(&ctx);
                    ctx.set_zoom_factor(zoom);
                    let mut browser = BrowserView::default();
                    browser.options.insert(
                        "fixture".into(),
                        Options {
                            device: Device::Custom,
                            width: 3840,
                            height: 2160,
                            console: true,
                            fit,
                            ..Default::default()
                        },
                    );
                    let mut native = egui::Rect::NOTHING;
                    let mut pane = egui::Rect::NOTHING;
                    let mut status_bottom = 0.0_f32;
                    for _ in 0..3 {
                        let output = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    Vec2::new(width + 240.0, 700.0),
                                )),
                                ..Default::default()
                            },
                            |ctx| {
                                egui::SidePanel::left("chat")
                                    .exact_width(220.0)
                                    .show(ctx, |ui| {
                                        ui.label("Chat remains visible");
                                    });
                                egui::CentralPanel::default().show(ctx, |ui| {
                                    pane = ui.clip_rect();
                                    native = browser.viewport_with_state(
                                        ui,
                                        "fixture",
                                        PageState {
                                            error: Some("Page returned HTTP 503.".into()),
                                            ..Default::default()
                                        },
                                    );
                                });
                            },
                        );
                        for shape in output.shapes {
                            if let egui::Shape::Text(text) = shape.shape {
                                if text.galley.job.text.contains("Certificate checks") {
                                    status_bottom = text.pos.y + text.galley.size().y;
                                }
                            }
                        }
                    }
                    let console = ctx
                        .data(|data| {
                            data.get_temp::<egui::Rect>(egui::Id::new((
                                "browser-control",
                                "Hide console",
                            )))
                        })
                        .unwrap();
                    assert!(
                        native.is_finite() && native.is_positive(),
                        "{width} {zoom} {fit}: {native:?}"
                    );
                    assert!(pane.contains_rect(native), "{pane:?} / {native:?}");
                    assert!(native.top() >= status_bottom);
                    assert!(native.bottom() < console.top());
                    assert!(native.left() >= 220.0);
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn full_app_preset_popup_owns_modified_keys_and_space_without_answering_approval() {
    use crate::app::files::tests::{app, click, frame};
    for label in ["Open", "Desktop · 1440 × 900"] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.state.busy = true;
        app.draft = "Keep draft".into();
        let root = app.prefs.cwd.clone();
        app.files
            .open_target(&ctx, &root, "http://localhost:3000/fixture")
            .unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let size = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("browser-control", "Auto"))))
            .unwrap();
        click(&ctx, &mut app, size.center());
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let (id, _) = ctx
            .data(|data| {
                data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new(("custom-viewport", label)))
            })
            .unwrap();
        ctx.memory_mut(|memory| memory.request_focus(id));
        app.state.dialogs.push_back(json!({"id":"viewport-approval", "method":"confirm", "title":"Allow bash?", "message":"Run fixture\n\n{}\n\nApprove this call only."}));
        for event in [
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
        ] {
            frame(&ctx, &mut app, vec![event]);
            assert_eq!(app.state.dialogs.len(), 1);
            assert!(egui::Popup::is_any_open(&ctx));
            assert_eq!(ctx.memory(|memory| memory.focused()), Some(id));
        }
        frame(
            &ctx,
            &mut app,
            vec![
                key(egui::Key::Space, egui::Modifiers::NONE),
                egui::Event::Text(" ".into()),
            ],
        );
        assert!(!ctx.input(|input| input.key_pressed(egui::Key::Space)));
        assert!(!ctx.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Text(text) if text == " "))
        }));
        assert_eq!(app.state.dialogs.len(), 1);
        // Reopen if Space selected a preset, then Escape must dismiss only popup.
        if !egui::Popup::is_any_open(&ctx) {
            let size = ctx
                .data(|data| {
                    data.get_temp::<egui::Rect>(egui::Id::new(("browser-control", "Auto")))
                })
                .unwrap();
            // Label changes one frame after selecting the preset.
            frame(&ctx, &mut app, vec![]);
            let size = ctx
                .data(|data| {
                    data.get_temp::<egui::Rect>(egui::Id::new(("browser-control", "1440 × 900")))
                })
                .unwrap_or(size);
            click(&ctx, &mut app, size.center());
        }
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(!egui::Popup::is_any_open(&ctx));
        assert_eq!(app.state.dialogs.len(), 1);
        assert_eq!(app.draft, "Keep draft");
        assert!(app.state.busy);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn full_app_custom_editor_keys_preserve_chat_work_draft_and_pending_approval() {
    use crate::app::files::tests::{app, click, frame, text};
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.prefs.theme = mode;
        app.state.busy = true;
        app.draft = "Do not submit, stop, or replace this".into();
        let messages = app.state.messages.len();
        let root = app.prefs.cwd.clone();
        app.files
            .open_target(&ctx, &root, "http://localhost:3000/custom-fixture")
            .unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let pos = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("browser-control", "Auto"))))
            .unwrap()
            .center();
        click(&ctx, &mut app, pos);
        frame(&ctx, &mut app, vec![]);
        let pos = ctx
            .data(|data| {
                data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new(("custom-viewport", "Open")))
            })
            .unwrap()
            .1
            .center();
        click(&ctx, &mut app, pos);
        frame(&ctx, &mut app, vec![]);
        // Even if navigation held focus before opening Custom, a fast Enter
        // belongs to this editor, not the address field rendered before it.
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert_eq!(
            app.files.navigation_target().as_deref(),
            Some("http://localhost:3000/custom-fixture")
        );
        let output = frame(&ctx, &mut app, vec![]);
        // The kittest frame has no native window: truthful unavailability keeps
        // Apply disabled and the draft open instead of claiming a CEF resize.
        text(&output, "Browser unavailable");
        app.state.dialogs.push_back(json!({"id":"viewport-approval", "method":"confirm", "title":"Allow bash?", "message":"Run fixture\n\n{}\n\nApprove this call only."}));
        for event in [
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
        ] {
            frame(&ctx, &mut app, vec![event]);
            assert_eq!(app.state.dialogs.len(), 1);
        }
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert_eq!(
            app.state.dialogs.len(),
            1,
            "Escape must not deny the agent prompt"
        );
        assert!(app.state.busy, "Escape must not stop the agent");
        assert_eq!(app.draft, "Do not submit, stop, or replace this");
        assert_eq!(app.state.messages.len(), messages);
        assert_eq!(
            app.files.navigation_target().as_deref(),
            Some("http://localhost:3000/custom-fixture")
        );
        let output = frame(&ctx, &mut app, vec![]);
        text(&output, "Auto");
    }
}
