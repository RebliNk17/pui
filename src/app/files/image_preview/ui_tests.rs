use super::*;
use crate::app::files::tests as file_tests;

#[test]
fn real_oversized_header_is_visible_readonly_bounded_and_retry_is_explicit() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            let mut app = file_tests::app(&ctx);
            app.prefs.theme = mode;
            app.prefs.hide_sidebar = true;
            ctx.set_zoom_factor(zoom);
            let root = app.prefs.cwd.clone();
            let path = root.join("too-large.png");
            let bytes = super::tests::dimensions(super::tests::png(), 8192, 8192);
            fs::write(&path, &bytes).unwrap();
            app.state.busy = true;
            app.draft = "Keep draft".into();
            app.state.dialogs.push_back(json!({"id":"image-preflight-permission","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."}));
            let (client, _, commands) = rpc::Client::stub_with_records();
            app.client = Some(client);
            let messages = app.state.messages.len();
            app.files
                .open_workspace_source(&ctx, &root, &path, None)
                .unwrap();
            file_tests::settle(&mut app.files);
            assert!(
                matches!(&app.files.tabs[0].content,Some(Err(error))if error.contains("32 MiB decoded-pixel"))
            );
            let mut time = 0.0;
            let mut draw = || {
                time += 0.1;
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(480.0, 820.0),
                        )),
                        time: Some(time),
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut eframe::Frame::_new_kittest()),
                )
            };
            for _ in 0..50 {
                draw();
                crate::media::wait_decodes(&ctx);
            }
            let output = draw();
            let (clip, text) = output
                .shapes
                .iter()
                .find_map(|shape| {
                    if let egui::Shape::Text(text) = &shape.shape {
                        text.galley
                            .job
                            .text
                            .contains("32 MiB decoded-pixel")
                            .then_some((shape.clip_rect, text))
                    } else {
                        None
                    }
                })
                .unwrap();
            assert!(clip.contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size())));
            assert!(
                output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                    > Duration::from_secs(1)
            );
            assert_eq!(app.draft, "Keep draft");
            assert_eq!(app.state.messages.len(), messages);
            assert!(app.state.busy && app.state.dialogs.len() == 1 && app.pending_dialog.is_none());
            assert!(commands.try_recv().is_err());
            assert!(app.files.tabs[0].texture.is_none());
            assert_eq!(fs::read(&path).unwrap(), bytes);
            // Only changing the test fixture followed by an explicit UI Refresh
            // retries; neither repaints nor budget availability do so silently.
            fs::write(&path, super::tests::png()).unwrap();
            for _ in 0..4 {
                file_tests::frame(&ctx, &mut app, vec![]);
            }
            assert!(matches!(app.files.tabs[0].content, Some(Err(_))));
            let (_, rect) = ctx
                .data(|data| {
                    data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("file-preview-refresh"))
                })
                .unwrap();
            file_tests::click(&ctx, &mut app, rect.center());
            file_tests::settle(&mut app.files);
            assert!(
                matches!(&app.files.tabs[0].content,Some(Ok(Content::Image(image)))if image.size==[2,2])
            );
            assert!(!app.files.tabs[0].external && app.files.tabs[0].source);
        }
    }
}
