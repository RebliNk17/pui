use super::*;
use crate::app::files::tests as file_tests;

#[test]
fn synthetic_queue_error_demo_does_not_spawn_or_saturate_real_workers() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_files: true,
            demo_preview_queue: true,
            ..Default::default()
        },
    );
    for _ in 0..4 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    assert!(app.client.is_none() && app.files.tabs[0]._load.is_none());
    assert!(matches!(&app.files.tabs[0].content, Some(Err(error)) if error == BUSY));
    let pool = pool(&ctx);
    let state = pool.0.lock().unwrap();
    assert_eq!(state.active, 0);
    assert!(state.pending.is_empty());
}

#[test]
fn queue_overflow_error_keeps_approval_draft_run_and_retry_needs_explicit_refresh() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let mut app = file_tests::app(&ctx);
        app.prefs.theme = mode;
        app.prefs.hide_sidebar = true;
        let root = app.prefs.cwd.clone();
        let blocked = super::tests::blocked(&ctx, &root);
        let mut tickets = vec![];
        for _ in 0..MAX_PENDING {
            let (_, ticket) = start(
                &ctx,
                root.clone(),
                root.join("README.md"),
                false,
                budget::Lease::new(&ctx),
            );
            tickets.push(ticket);
        }
        assert_eq!(blocked.pool.0.lock().unwrap().pending.len(), MAX_PENDING);
        app.state.busy = true;
        app.draft = "Keep draft".into();
        app.state.dialogs.push_back(json!({"id":"queue-permission","method":"confirm","title":"Allow bash?","message":"Run\n{\"command\":\"echo example\"}\n\nApprove this call only."}));
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        app.files
            .open_workspace_source(&ctx, &root, &root.join("README.md"), Some(2))
            .unwrap();
        file_tests::settle(&mut app.files);
        assert!(
            matches!(&app.files.tabs[0].content,Some(Err(error))if error.contains("2 active, 32 waiting"))
        );
        for _ in 0..4 {
            file_tests::frame(&ctx, &mut app, vec![]);
        }
        let out = file_tests::frame(&ctx, &mut app, vec![]);
        file_tests::text(&out, "File preview queue is full");
        assert!(app.state.busy && app.state.dialogs.len() == 1 && app.pending_dialog.is_none());
        assert_eq!(app.draft, "Keep draft");
        assert!(commands.try_recv().is_err());
        drop(tickets);
        assert!(blocked.pool.0.lock().unwrap().pending.is_empty());
        drop(blocked);
        // Error stays local and bounded at narrow width/zoom; no queue work,
        // recurring repaint or re-read is needed to keep it displayed.
        for zoom in [0.8, 1.4] {
            ctx.set_zoom_factor(zoom);
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
                            .contains("File preview queue is full")
                            .then_some((shape.clip_rect, text))
                    } else {
                        None
                    }
                })
                .unwrap();
            assert!(
                clip.contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size())),
                "{mode:?} {zoom}: {:?} clip {clip:?}",
                egui::Rect::from_min_size(text.pos, text.galley.size())
            );
            assert!(
                output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                    > Duration::from_secs(1)
            );
        }
        ctx.set_zoom_factor(1.0);
        for _ in 0..4 {
            file_tests::frame(&ctx, &mut app, vec![]);
        }
        assert!(
            matches!(app.files.tabs[0].content, Some(Err(_))),
            "Repaint must not retry silently"
        );
        let (_, rect) = ctx
            .data(|data| {
                data.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new("file-preview-refresh"))
            })
            .unwrap();
        file_tests::click(&ctx, &mut app, rect.center());
        file_tests::settle(&mut app.files);
        assert!(
            matches!(&app.files.tabs[0].content,Some(Ok(Content::Text(text)))if text.contains("Workspace"))
        );
        assert!(!app.files.tabs[0].external && app.files.tabs[0].source);
    }
}
