use super::*;
use crate::app::files::tests as file_tests;

#[test]
fn demo_budget_uses_real_admission_error_without_provider_or_filesystem_reads() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_files: true,
            demo_preview_budget: true,
            ..Default::default()
        },
    );
    for _ in 0..4 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    assert!(app.client.is_none());
    assert!(app.files.tabs[0].rx.is_none());
    assert_eq!(
        Lease::new(&ctx).pool.used.load(Ordering::Relaxed),
        MAX_BYTES
    );
    assert!(
        matches!(&app.files.tabs[0].content, Some(Err(error)) if error.contains("Close file tabs"))
    );
    app.files.close_tab(0);
    assert_eq!(Lease::new(&ctx).pool.used.load(Ordering::Relaxed), 0);
}

#[test]
fn bounded_error_fits_narrow_theme_zoom_and_keyboard_refresh_retries_once() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut app = file_tests::app(&ctx);
            app.prefs.theme = mode;
            app.prefs.hide_sidebar = true;
            let occupied = Lease::new(&ctx);
            occupied
                .admit(&Content::Text(String::with_capacity(MAX_BYTES)))
                .unwrap();
            let path = app.prefs.cwd.join("README.md");
            app.files
                .open_workspace_source(&ctx, &app.prefs.cwd.clone(), &path, None)
                .unwrap();
            file_tests::settle(&mut app.files);
            let mut time = 0.0;
            let mut draw = |app: &mut Pui, events| {
                time += 0.1;
                let mut native = eframe::Frame::_new_kittest();
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(480.0, 820.0),
                        )),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(app, ctx, &mut native),
                )
            };
            for _ in 0..50 {
                draw(&mut app, vec![]);
            }
            let out = draw(&mut app, vec![]);
            let error = out
                .shapes
                .iter()
                .find_map(|shape| {
                    if let egui::Shape::Text(text) = &shape.shape {
                        text.galley
                            .job
                            .text
                            .contains("64 MiB across chats")
                            .then_some((shape.clip_rect, text))
                    } else {
                        None
                    }
                })
                .unwrap();
            let rect = egui::Rect::from_min_size(error.1.pos, error.1.galley.size());
            assert!(
                error.0.contains_rect(rect),
                "{mode:?} {zoom}: {rect:?} {:?}",
                error.0
            );
            assert!(
                out.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(1)
            );
            assert!(
                egui::containers::panel::PanelState::load(
                    &ctx,
                    egui::Id::new("workspace-files-stack")
                )
                .is_some()
            );
            drop(occupied);
            let (id, rect) = ctx
                .data(|data| {
                    data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("file-preview-refresh"))
                })
                .unwrap();
            assert!(ctx.screen_rect().contains_rect(rect));
            ctx.memory_mut(|memory| memory.request_focus(id));
            draw(&mut app, vec![]);
            draw(
                &mut app,
                vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            file_tests::settle(&mut app.files);
            assert!(matches!(
                app.files.tabs[0].content,
                Some(Ok(Content::Text(_)))
            ));
            assert_eq!(app.files.tabs.len(), 1);
        }
    }
}
