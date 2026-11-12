use super::*;

fn show(ctx: &egui::Context, browser: &mut Browser, root: &Path) {
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        browser.show(ctx, root, &mut TableHeaders::default());
    });
}

#[test]
fn hidden_checkpoint_waits_until_open_and_preserves_source_tab_order() {
    let ctx = egui::Context::default();
    let app = tests::app(&ctx);
    let root = &app.prefs.cwd;
    let html = root.join("index.html");
    fs::write(&html, "<h1>Source tab</h1>").unwrap();
    let saved = SavedLayout {
        paths: vec![root.join("README.md"), html.clone()],
        selected: Some(html.clone()),
        source_paths: BTreeSet::from([html.clone()]),
        ..Default::default()
    };
    let mut browser = Browser::default();
    browser.restore_layout(root, &saved);
    show(&ctx, &mut browser, root);
    assert!(browser.tabs.is_empty());
    assert!(browser.directories.is_empty());
    browser.toggle(&ctx, root);
    assert!(browser.restored.is_none());
    assert_eq!(
        browser
            .tabs
            .iter()
            .map(|t| t.path.clone())
            .collect::<Vec<_>>(),
        saved.paths
    );
    assert_eq!(browser.selected, Some(html));
    assert!(browser.files_open());
    assert!(browser.tabs[1].source);
    assert!(browser.urls.is_empty());
}

#[test]
fn browser_url_selection_restores_without_gate_or_navigation_of_other_urls() {
    let ctx = egui::Context::default();
    let app = tests::app(&ctx);
    let root = &app.prefs.cwd;
    let saved = SavedLayout {
        open: true,
        browser_mode: true,
        urls: vec![
            "https://example.com/".into(),
            "http://localhost:3000/".into(),
        ],
        selected_url: Some("http://localhost:3000/".into()),
        ..Default::default()
    };
    let mut browser = Browser::default();
    browser.restore_layout(root, &saved);
    show(&ctx, &mut browser, root);
    assert!(browser.restored.is_none());
    assert!(browser.browser_open());
    assert_eq!(browser.urls, saved.urls);
    assert_eq!(browser.selected_url, saved.selected_url);
    assert_eq!(browser.address, "http://localhost:3000/");
    assert_eq!(
        browser.web_preview.as_ref().unwrap().url.as_deref(),
        Some("http://localhost:3000/")
    );
    assert!(browser.tabs.is_empty());
}

#[test]
fn workspace_change_discards_old_pending_reads() {
    let ctx = egui::Context::default();
    let app = tests::app(&ctx);
    let root = &app.prefs.cwd;
    let saved = SavedLayout {
        open: true,
        paths: vec![root.join("README.md")],
        selected: Some(root.join("README.md")),
        ..Default::default()
    };
    let other = root.join("src");
    let mut browser = Browser::default();
    browser.restore_layout(root, &saved);
    show(&ctx, &mut browser, &other);
    assert!(browser.restored.is_none());
    assert!(browser.tabs.is_empty());
    assert!(browser.selected.is_none());
    assert_eq!(browser.root, other);
}

#[test]
fn missing_retained_file_stays_an_error_not_a_restore_question() {
    let ctx = egui::Context::default();
    let app = tests::app(&ctx);
    let root = &app.prefs.cwd;
    let missing = root.join("missing.txt");
    let saved = SavedLayout {
        open: true,
        paths: vec![missing.clone()],
        selected: Some(missing.clone()),
        ..Default::default()
    };
    let mut browser = Browser::default();
    browser.restore_layout(root, &saved);
    show(&ctx, &mut browser, root);
    tests::settle(&mut browser);
    assert!(browser.restored.is_none());
    assert_eq!(browser.selected, Some(missing));
    assert!(matches!(browser.tabs[0].content, Some(Err(_))));
}
