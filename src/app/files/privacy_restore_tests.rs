//! Browser restore must hydrate presentation without executable navigation intent.
use super::*;

#[test]
fn restored_web_layout_frame_keeps_urls_stopped_until_explicit_resume() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let mut app = tests::app(&ctx);
        mode.apply(&ctx);
        let root = app.prefs.cwd.clone();
        let owner = app.review_comment_owner();
        let one = "https://restore-one.test/start";
        let two = "https://restore-two.test/start";
        let saved: SavedLayout = serde_json::from_value(json!({
            "open": true,
            "browser_mode": true,
            "selected_url": one,
            "urls": [one, two]
        }))
        .unwrap();
        let mut browser = Browser::default();
        browser.restore_layout(&root, &saved);
        // Match app pre-Files render binding order: restore, owner bind, Files.show.
        browser.bind_browser_owner(&root, &owner);
        for width in [620.0, 1100.0] {
            for _ in 0..3 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, 860.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        browser.show(ctx, &root, &mut TableHeaders::default());
                    },
                );
            }
            assert!(browser.restored.is_none());
            assert_eq!(browser.urls, [one, two]);
            assert!(browser.web.session_suspended(one));
            assert!(browser.web.session_suspended(two));
            assert!(browser.web.location(one).is_none());
            assert!(browser.web.location(two).is_none());
            let preview = browser
                .web_preview
                .as_ref()
                .expect("selected restored page has stopped placeholder");
            assert_eq!(preview.key, one);
            assert!(
                !preview.rect.is_positive(),
                "a stopped restored page must not provide a native drawable rectangle"
            );
            assert!(!browser.web.privacy_confirming());
        }
        browser.web.resume_session(one);
        assert!(!browser.web.session_suspended(one));
        assert!(browser.web.session_suspended(two));
        // Saving/reloading the stopped layout cannot carry a Resume or approval.
        let saved = browser.saved_layout();
        browser.restore_layout(&root, &saved);
        browser.bind_browser_owner(&root, &owner);
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1100.0, 860.0),
                )),
                ..Default::default()
            },
            |ctx| {
                browser.show(ctx, &root, &mut TableHeaders::default());
            },
        );
        assert!(browser.web.session_suspended(one));
        assert!(browser.web.session_suspended(two));
        assert!(!browser.web.privacy_confirming());
    }
}
