use super::*;
fn frame(ctx: &egui::Context, app: &mut Pui, width: f32, events: Vec<egui::Event>) {
    let mut native = eframe::Frame::_new_kittest();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 850.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    );
}
#[test]
fn compact_rail_actions_fit_viewport_in_both_themes_and_zoom() {
    for dark in [false, true] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            ctx.set_zoom_factor(zoom);
            let mut app = super::tests::app(&ctx);
            assert!(app.start_side_chat("kept draft"));
            app.prefs.hide_sidebar = true;
            for _ in 0..4 {
                frame(&ctx, &mut app, 420.0, vec![]);
            }
            let mut rects = Vec::new();
            for control in [
                "side-chat-switch",
                "side-chat-preview-toggle",
                "side-chat-dismiss",
            ] {
                let id = ctx
                    .data(|data| data.get_temp::<egui::Id>(egui::Id::new(control)))
                    .unwrap();
                let response = ctx.read_response(id).unwrap();
                assert!(response.enabled());
                assert!(
                    ctx.screen_rect().contains_rect(response.interact_rect),
                    "{control}: {:?}",
                    response.interact_rect
                );
                rects.push(response.interact_rect);
            }
            for pair in rects.windows(2) {
                assert!(!pair[0].intersects(pair[1]));
            }
            assert_eq!(app.draft, "kept draft");
        }
    }
}
#[test]
fn short_narrow_window_yields_preview_space_and_resize_restores_preference() {
    for dark in [false, true] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            ctx.set_zoom_factor(zoom);
            let mut app = super::tests::app(&ctx);
            app.prefs.hide_sidebar = true;
            assert!(app.start_side_chat("keep short-window draft"));
            app.prefs.hide_sidebar = true;
            for height in [280.0, 1200.0] {
                ctx.data_mut(|data| {
                    data.remove::<u64>(egui::Id::new("side-chat-preview-visible"));
                });
                let mut native = eframe::Frame::_new_kittest();
                for _ in 0..3 {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(420.0, height),
                            )),
                            ..Default::default()
                        },
                        |ctx| eframe::App::update(&mut app, ctx, &mut native),
                    );
                }
                let visible = ctx
                    .data(|data| data.get_temp::<u64>(egui::Id::new("side-chat-preview-visible")))
                    .is_some_and(|pass| pass.checked_add(1) == Some(ctx.cumulative_pass_nr()));
                assert_eq!(visible, height > 280.0);
                assert!(app.side_chat.preview_open);
                assert_eq!(app.draft, "keep short-window draft");
                assert!(ctx.read_response(egui::Id::new("composer")).is_some());
            }
        }
    }
}
#[test]
fn workspace_navigation_hides_preview_without_losing_preference_or_draft() {
    let ctx = egui::Context::default();
    let mut app = super::tests::app(&ctx);
    assert!(app.start_side_chat("kept side draft"));
    for tool in 0..3 {
        ctx.data_mut(|data| {
            data.remove::<u64>(egui::Id::new("side-chat-preview-visible"));
        });
        match tool {
            0 => {
                app.files.toggle(&ctx, &app.prefs.cwd);
            }
            1 => app.inspector_panel = true,
            _ => app.tasks.open = true,
        }
        frame(&ctx, &mut app, 1600.0, vec![]);
        assert!(
            ctx.data(|data| data.get_temp::<u64>(egui::Id::new("side-chat-preview-visible")))
                .is_none()
        );
        assert!(app.side_chat.preview_open);
        assert_eq!(app.draft, "kept side draft");
        match tool {
            0 => {
                app.files.toggle(&ctx, &app.prefs.cwd);
            }
            1 => app.inspector_panel = false,
            _ => app.tasks.open = false,
        }
        frame(&ctx, &mut app, 1600.0, vec![]);
        assert!(
            ctx.data(|data| data.get_temp::<u64>(egui::Id::new("side-chat-preview-visible")))
                .is_some()
        );
    }
}
#[test]
fn preview_hide_reopen_and_wide_narrow_bounds_preserve_unread_and_draft() {
    for width in [420.0, 1600.0] {
        for dark in [false, true] {
            let ctx = egui::Context::default();
            if dark {
                ctx.set_visuals(egui::Visuals::dark());
            }
            let mut app = super::tests::app(&ctx);
            app.prefs.hide_sidebar = true;
            let main = app.agents.active.clone();
            let path = app.prefs.cwd.join("preview-unread-fixture.jsonl");
            app.state.session_file = Some(path.to_string_lossy().into());
            app.attention_event(&json!({"type":"response","command":"prompt","success":false,"id":"preview-unread"}));
            assert!(app.start_side_chat("unchanged side draft"));
            assert_eq!(app.attention.unread(), 1);
            for _ in 0..3 {
                frame(&ctx, &mut app, width, vec![]);
            }
            let rect = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("side-chat-preview-rect")))
                .unwrap();
            assert!(rect.width() > 0.0 && rect.height() > 0.0);
            if width < 900.0 {
                assert!(rect.height() <= 156.0, "{rect:?}");
            } else {
                assert!(rect.width() <= 300.0, "{rect:?}");
            }
            assert_eq!(app.attention.unread(), 1);
            for expected in [false, true] {
                let id = ctx
                    .data(|data| {
                        data.get_temp::<egui::Id>(egui::Id::new("side-chat-preview-toggle"))
                    })
                    .unwrap();
                ctx.memory_mut(|m| m.request_focus(id));
                frame(
                    &ctx,
                    &mut app,
                    width,
                    vec![egui::Event::Key {
                        key: egui::Key::Enter,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
                assert_eq!(app.side_chat.preview_open, expected);
                frame(&ctx, &mut app, width, vec![]);
                assert_eq!(app.draft, "unchanged side draft");
                assert_eq!(app.attention.unread(), 1);
                assert!(app.agents.parked.contains_key(&main));
            }
        }
    }
}
