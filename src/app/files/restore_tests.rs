use super::*;

#[test]
fn restored_viewer_loads_automatically_and_stacks_below_chat_in_narrow_windows() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let app = tests::app(&ctx);
        mode.apply(&ctx);
        let root = &app.prefs.cwd;
        let saved = SavedLayout {
            open: true,
            paths: vec![root.join("README.md")],
            selected: Some(root.join("README.md")),
            ..Default::default()
        };
        let mut browser = Browser::default();
        browser.restore_layout(root, &saved);
        assert!(
            browser.tabs.is_empty(),
            "Checkpoint hydration must not perform IO"
        );
        for width in [620.0, 1100.0] {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(width, 800.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    assert!(!browser.show(ctx, root, &mut TableHeaders::default()));
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.label("Chat retained");
                    });
                },
            );
            assert!(browser.restored.is_none());
            assert_eq!(browser.tabs.len(), 1);
            assert_eq!(
                browser.directories.len(),
                1,
                "Only the root directory loads initially"
            );
            assert!(browser.web_preview.is_none());
            let chat = tests::text(&output, "Chat retained");
            let tab = tests::text(&output, "README.md");
            if width < 700.0 {
                assert!(tab.y > chat.y);
            } else {
                assert!(tab.x > chat.x);
            }
        }
        browser.close();
        let closed = browser.saved_layout();
        assert!(!closed.open);
        assert_eq!(closed.paths.len(), 1);
        browser.restore_layout(root, &closed);
        assert!(browser.tabs.is_empty());
        browser.toggle(&ctx, root);
        assert_eq!(browser.tabs.len(), 1);
        assert!(browser.restored.is_none());
    }
}

#[test]
fn restoring_mixed_tabs_preserves_selected_pane_and_lazily_loads_folders() {
    let ctx = egui::Context::default();
    let app = tests::app(&ctx);
    let root = &app.prefs.cwd;
    let html = root.join("index.html");
    let text = root.join("README.md");
    fs::write(&html, "<h1>Example</h1>").unwrap();
    let saved = SavedLayout {
        open: true,
        browser_mode: false,
        paths: vec![text.clone(), html.clone()],
        urls: vec![html.display().to_string()],
        selected: Some(text.clone()),
        selected_url: Some(html.display().to_string()),
        directories: vec![root.join("src")],
        ..Default::default()
    };
    let mut browser = Browser::default();
    browser.restore_layout(root, &saved);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        browser.show(ctx, root, &mut TableHeaders::default());
    });
    assert!(
        browser.files_open(),
        "Loading a trailing HTML tab must not switch the selected pane"
    );
    assert_eq!(browser.selected, Some(text));
    browser.directories.get_mut(root).unwrap().listing = Some(list(root, root));
    let _ =
        ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut actions = Vec::new();
                browser.tree(ui, root, 0, &mut actions);
                assert!(actions.iter().any(
                    |action| matches!(action, Action::Load(path) if path == &root.join("src"))
                ));
            });
        });
}

#[cfg(unix)]
#[test]
fn saved_workspace_tabs_do_not_gain_external_open_privileges() {
    let ctx = egui::Context::default();
    let app = tests::app(&ctx);
    let root = &app.prefs.cwd;
    let path = root.join("src/nested/example.rs");
    let saved = SavedLayout {
        open: true,
        paths: vec![path.clone()],
        source_paths: BTreeSet::from([path.clone()]),
        selected: Some(path.clone()),
        ..Default::default()
    };
    // The old workspace folder was replaced by a symlink between launches.
    fs::rename(root.join("src"), root.join("old-src")).unwrap();
    std::os::unix::fs::symlink(root.join("old-src"), root.join("src")).unwrap();
    let mut browser = Browser::default();
    browser.restore_layout(root, &saved);
    browser.activate_restored_tabs(&ctx);
    let tab = &mut browser.tabs[0];
    assert!(!tab.external);
    assert!(tab.source);
    let content = tab
        .rx
        .take()
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    assert!(matches!(content, Err(error) if error.contains("Symlinked")));
}

#[test]
fn persisted_tabs_validate_urls_membership_and_retention_limits() {
    let saved: SavedLayout = serde_json::from_value(json!({
        "open": true,
        "paths": (0..100).map(|i| format!("/workspace/{i}.rs")).collect::<Vec<_>>(),
        "urls": ["https://example.com", "https://user:secret@example.com", "javascript:alert(1)", "/missing.html"],
        "selected": "/missing.rs",
        "selected_url": "javascript:alert(1)",
        "directories": (0..300).map(|i| format!("/workspace/folder-{i}")).collect::<Vec<_>>()
    })).unwrap();
    let saved = saved.bounded();
    assert_eq!(saved.paths.len(), MAX_TABS);
    assert_eq!(saved.directories.len(), MAX_DIRECTORIES - 1);
    assert_eq!(saved.urls, ["https://example.com"]);
    assert!(saved.selected.is_none());
    assert!(saved.selected_url.is_none());
}
