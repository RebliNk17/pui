use super::*;
use tests::{app, settle, text};

fn draw(ctx: &egui::Context, app: &mut Pui, width: f32) -> egui::FullOutput {
    draw_events(ctx, app, width, vec![])
}

fn draw_events(
    ctx: &egui::Context,
    app: &mut Pui,
    width: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 960.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    )
}

#[cfg(target_os = "macos")]
#[test]
fn inline_questions_and_permissions_keep_html_and_web_previews_visible() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for (width, zoom) in [(1480.0, 1.0), (860.0, 1.25)] {
            for local_html in [true, false] {
                let ctx = egui::Context::default();
                let mut app = app(&ctx);
                app.prefs.theme = mode;
                app.prefs.hide_sidebar = true;
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                app.state.busy = true;
                app.draft = "Keep this draft".into();
                let root = app.prefs.cwd.clone();
                let target = if local_html {
                    let path = root.join("report.html");
                    fs::write(&path, "<h1>Question reference</h1>").unwrap();
                    path.display().to_string()
                } else {
                    "https://example.com/reference".into()
                };
                app.files.open_target(&ctx, &root, &target).unwrap();
                settle(&mut app.files);
                for _ in 0..3 {
                    draw(&ctx, &mut app, width);
                }
                let selected = app.files.navigation_target();
                let dialogs = [
                    questions::demo_dialog(),
                    json!({"id":"preview-permission","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo safe\"}\n\nApprove this call only."}),
                ];
                for dialog in dialogs {
                    let id = protocol::string(&dialog, "id").to_owned();
                    let heading = if questions::Request::parse(&dialog).is_some() {
                        "How should users start a task?"
                    } else {
                        "Allow bash?"
                    };
                    app.state.dialogs.push_back(dialog);
                    for _ in 0..3 {
                        let output = draw(&ctx, &mut app, width);
                        text(&output, heading);
                        let preview = app.files.web_preview.as_ref().unwrap();
                        assert_eq!(preview.key, target);
                        assert!(preview.rect.is_positive());
                        assert!(app.web_preview_allowed(false, false));
                        assert!(!app.files.web_obscured(&ctx));
                        assert_eq!(app.files.navigation_target(), selected);
                        assert_eq!(app.state.dialogs.len(), 1);
                        assert!(app.state.busy);
                        assert_eq!(app.draft, "Keep this draft");
                    }
                    // Mirrored answers awaiting acknowledgment must not blank
                    // the reference either, and must remain disabled, not sent.
                    app.pending_dialog = Some(id.clone());
                    draw(&ctx, &mut app, width);
                    assert!(app.web_preview_allowed(false, false));
                    assert_eq!(app.state.dialogs.len(), 1);
                    app.pending_dialog = None;
                    let output = draw(&ctx, &mut app, width);
                    let cancel = text(
                        &output,
                        if heading == "Allow bash?" {
                            "Deny"
                        } else {
                            "Cancel"
                        },
                    );
                    for pressed in [true, false] {
                        draw_events(
                            &ctx,
                            &mut app,
                            width,
                            vec![
                                egui::Event::PointerMoved(cancel),
                                egui::Event::PointerButton {
                                    pos: cancel,
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                        );
                    }
                    draw(&ctx, &mut app, width);
                    assert!(app.state.dialogs.is_empty());
                    assert!(app.web_preview_allowed(false, false));
                    assert_eq!(app.files.navigation_target(), selected);
                    assert_eq!(app.draft, "Keep this draft");
                }
            }
        }
    }
}

#[test]
fn inline_questions_do_not_bypass_workspace_overlay_guards() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.dialogs.push_back(questions::demo_dialog());
    assert!(app.web_preview_allowed(false, false));
    assert!(!app.web_preview_allowed(true, false));
    assert!(!app.web_preview_allowed(false, true));
    app.settings_open = true;
    assert!(!app.web_preview_allowed(false, false));
    app.settings_open = false;
    app.kanban.open = true;
    assert!(!app.web_preview_allowed(false, false));
    app.kanban.open = false;
    app.automation.open = true;
    assert!(!app.web_preview_allowed(false, false));
    app.automation.open = false;
    app.search_open = true;
    assert!(!app.web_preview_allowed(false, false));
    app.search_open = false;
    assert!(app.web_preview_allowed(false, false));
}
