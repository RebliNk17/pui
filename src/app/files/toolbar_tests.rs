use super::*;

fn control(ctx: &egui::Context, label: &str) -> egui::Rect {
    ctx.data(|data| data.get_temp(egui::Id::new(("browser-control", label))))
        .unwrap_or_else(|| panic!("Missing {label}"))
}

fn draw(
    ctx: &egui::Context,
    browser: &mut Browser,
    width: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    browser.begin_frame();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(width, 700.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| browser.contents(ui, &mut TableHeaders::default()));
        },
    )
}

fn click(ctx: &egui::Context, browser: &mut Browser, width: f32, pos: egui::Pos2) {
    for pressed in [true, false] {
        draw(
            ctx,
            browser,
            width,
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

fn html_browser(ctx: &egui::Context) -> Browser {
    let path = PathBuf::from("/tmp/compact-preview-fixture.html");
    let mut browser = Browser::default();
    browser.open = true;
    browser.browser_mode = true;
    browser.selected_url = Some(path.display().to_string());
    browser.address = path.display().to_string();
    browser.urls = vec![path.display().to_string()];
    browser.tabs = vec![OpenFile {
        path,
        content: Some(Ok(Content::Text(
            "<!doctype html><h1>Exact source fixture</h1>".into(),
        ))),
        rx: None,
        texture: None,
        source: false,
        external: true,
        line_target: None,
        line_notice: None,
        _budget: budget::Lease::new(ctx),
        _load: None,
    }];
    browser
}

#[test]
fn compact_html_eye_toggle_hides_native_preview_and_controls_then_restores() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [280.0, 340.0, 720.0] {
            for zoom in [0.8, 1.0, 1.6] {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let mut browser = html_browser(&ctx);
                for _ in 0..3 {
                    draw(&ctx, &mut browser, width, vec![]);
                }
                let eye = control(&ctx, "Preview");
                let console = control(&ctx, "Show console");
                let size = control(&ctx, "Responsive");
                assert_eq!(eye.size(), Vec2::splat(28.0));
                for rect in [eye, console, size, control(&ctx, "Go to address")] {
                    assert!(
                        ctx.screen_rect().contains_rect(rect),
                        "{width} {zoom}: {rect:?}"
                    );
                }
                assert!((eye.center().y - console.center().y).abs() < 1.0);
                assert!(eye.right() < size.left() && size.right() < console.left());
                let preview = browser.web_preview.as_ref().unwrap().rect;
                assert!(
                    preview.left().abs() < 1.0
                        && (preview.right() - ctx.screen_rect().right()).abs() < 1.0,
                    "HTML preview should be edge-to-edge: {preview:?}"
                );
                let field = ctx
                    .data(|data| {
                        data.get_temp::<egui::Rect>(egui::Id::new("browser-address-field"))
                    })
                    .unwrap();
                let go = control(&ctx, "Go to address");
                assert_eq!(field.height(), 28.0);
                assert!(field.contains_rect(go));
                let editor = ctx
                    .data(|data| {
                        data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new(
                            "browser-address-editor",
                        ))
                    })
                    .unwrap()
                    .1;
                assert!(
                    editor.right() <= go.left(),
                    "Address text overlaps Go: {editor:?} / {go:?}"
                );
                click(&ctx, &mut browser, width, eye.center());
                let output = draw(&ctx, &mut browser, width, vec![]);
                assert!(browser.tabs[0].source);
                assert!(browser.web_preview.is_none());
                let texts: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Text(t) => Some(t.galley.job.text.as_str()),
                        _ => None,
                    })
                    .collect();
                assert!(texts.contains(&"<!doctype html><h1>Exact source fixture</h1>"));
                assert!(!texts.iter().any(|t| t.contains("Isolated preview")
                    || t.contains("Responsive")
                    || *t == "100%"
                    || *t == "Fit"));
                click(&ctx, &mut browser, width, control(&ctx, "Preview").center());
                draw(&ctx, &mut browser, width, vec![]);
                assert!(!browser.tabs[0].source);
                assert!(browser.web_preview.is_some());
            }
        }
    }
}

#[test]
fn html_file_with_explorer_keeps_eye_and_overflow_settings_inside_preview_column() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.6] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut browser = html_browser(&ctx);
            browser.browser_mode = false;
            browser.tree_open = true;
            browser.selected = Some(browser.tabs[0].path.clone());
            browser.tabs[0].source = true;
            browser.directories.insert(
                PathBuf::new(),
                Directory {
                    listing: Some(Ok(Listing {
                        entries: vec![],
                        truncated: false,
                    })),
                    rx: None,
                },
            );
            for _ in 0..3 {
                draw(&ctx, &mut browser, 280.0, vec![]);
            }
            click(&ctx, &mut browser, 280.0, control(&ctx, "Preview").center());
            for _ in 0..3 {
                draw(&ctx, &mut browser, 280.0, vec![]);
            }
            assert!(!browser.tabs[0].source);
            let preview = browser.web_preview.as_ref().unwrap().rect;
            let eye = control(&ctx, "Preview");
            let settings = control(&ctx, "Preview settings");
            assert!(
                eye.left() >= preview.left() && settings.right() <= preview.right(),
                "{zoom}: {eye:?} / {settings:?} / {preview:?}"
            );
            assert!(eye.right() < settings.left());
            click(&ctx, &mut browser, 280.0, settings.center());
            let output = draw(&ctx, &mut browser, 280.0, vec![]);
            let contains = |needle: &str| {
                output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(t) if t.galley.job.text.contains(needle)))
            };
            assert!(contains("Page zoom") && contains("Show console"));
            assert!(!contains("Fit viewport"));
            assert!(egui::Popup::is_any_open(&ctx));
        }
    }
}

#[test]
fn eye_toggle_keyboard_preserves_chat_draft_busy_state_and_message_count() {
    let ctx = egui::Context::default();
    let mut app = tests::app(&ctx);
    app.files = html_browser(&ctx);
    app.files.root = app.prefs.cwd.clone();
    app.prefs.hide_sidebar = true;
    app.state.busy = true;
    app.draft = "Keep this draft".into();
    let messages = app.state.messages.len();
    for _ in 0..3 {
        tests::frame(&ctx, &mut app, vec![]);
    }
    let id = ctx
        .data(|data| data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("file-preview-toggle")))
        .unwrap()
        .0;
    ctx.memory_mut(|memory| memory.request_focus(id));
    tests::frame(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(app.files.tabs[0].source);
    assert_eq!(app.draft, "Keep this draft");
    assert_eq!(app.state.messages.len(), messages);
    assert!(app.state.busy);
}
