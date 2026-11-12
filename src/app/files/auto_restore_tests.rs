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

fn mixed_hidden_layout(root: &Path) -> SavedLayout {
    let html = root.join("retained.html");
    let source = root.join("retained-source.html");
    fs::write(&html, "<h1>Browser</h1>").unwrap();
    fs::write(&source, "<h1>Source</h1>").unwrap();
    SavedLayout {
        paths: vec![root.join("README.md"), html.clone(), source.clone()],
        external_paths: BTreeSet::from([html.clone()]),
        source_paths: BTreeSet::from([source.clone()]),
        urls: vec![
            html.display().to_string(),
            "https://example.com/retained".into(),
        ],
        selected: Some(source),
        selected_url: Some(html.display().to_string()),
        directories: vec![root.join("src")],
        ..Default::default()
    }
}

fn assert_retained_prefix(browser: &Browser, saved: &SavedLayout) {
    assert!(browser.restored.is_none());
    assert!(browser.is_open());
    assert_eq!(
        browser
            .tabs
            .iter()
            .take(saved.paths.len())
            .map(|tab| &tab.path)
            .collect::<Vec<_>>(),
        saved.paths.iter().collect::<Vec<_>>()
    );
    for tab in browser.tabs.iter().take(saved.paths.len()) {
        assert_eq!(tab.external, saved.external_paths.contains(&tab.path));
        assert_eq!(tab.source, saved.source_paths.contains(&tab.path));
    }
    assert_eq!(
        browser.expanded_dirs,
        saved.directories.iter().cloned().collect()
    );
    assert!(
        browser.web_preview.is_none(),
        "Activation must not navigate retained URLs"
    );
}

#[test]
fn quick_open_materializes_mixed_hidden_tabs_before_selecting_new_source() {
    let ctx = egui::Context::default();
    let app = tests::app(&ctx);
    let root = &app.prefs.cwd;
    let saved = mixed_hidden_layout(root);
    let path = root.join("src/nested/example.rs");
    let mut browser = Browser::default();
    browser.restore_layout(root, &saved);
    browser
        .open_workspace_source(&ctx, root, &path, Some(2))
        .unwrap();
    assert_retained_prefix(&browser, &saved);
    assert_eq!(browser.tabs.len(), saved.paths.len() + 1);
    assert_eq!(browser.urls, saved.urls);
    assert_eq!(browser.selected, Some(path));
    assert!(browser.files_open());
    let tab = browser.tabs.last().unwrap();
    assert!(!tab.external && tab.source);
    assert_eq!(tab.line_target, Some(2));
    tests::settle(&mut browser);
    assert!(
        browser
            .tabs
            .iter()
            .all(|tab| matches!(tab.content, Some(Ok(_))))
    );
}

#[test]
fn explicit_file_open_materializes_mixed_hidden_tabs_before_selecting_new_target() {
    let ctx = egui::Context::default();
    let app = tests::app(&ctx);
    let root = &app.prefs.cwd;
    let saved = mixed_hidden_layout(root);
    let path = root.join("src/nested/example.rs");
    let mut browser = Browser::default();
    browser.restore_layout(root, &saved);
    browser
        .open_target(&ctx, root, path.to_str().unwrap())
        .unwrap();
    assert_retained_prefix(&browser, &saved);
    assert_eq!(browser.tabs.len(), saved.paths.len() + 1);
    assert_eq!(browser.urls, saved.urls);
    assert_eq!(browser.selected, Some(path));
    assert!(browser.files_open());
    assert!(browser.tabs.last().unwrap().external);
}

#[cfg(target_os = "macos")]
#[test]
fn explicit_url_open_materializes_mixed_hidden_tabs_without_navigating_retained_urls() {
    let ctx = egui::Context::default();
    let app = tests::app(&ctx);
    let root = &app.prefs.cwd;
    let saved = mixed_hidden_layout(root);
    let mut browser = Browser::default();
    browser.restore_layout(root, &saved);
    let target = "https://example.com/new";
    browser.open_target(&ctx, root, target).unwrap();
    assert_retained_prefix(&browser, &saved);
    assert_eq!(browser.tabs.len(), saved.paths.len());
    assert_eq!(browser.urls[..saved.urls.len()], saved.urls);
    assert_eq!(browser.urls.last().map(String::as_str), Some(target));
    assert_eq!(browser.selected_url.as_deref(), Some(target));
    assert_eq!(browser.address, target);
    assert!(browser.browser_open());
}

#[test]
fn hidden_activation_keeps_tab_cap_order_and_bounded_privilege_membership() {
    let ctx = egui::Context::default();
    let app = tests::app(&ctx);
    let root = &app.prefs.cwd;
    let paths = (0..MAX_TABS)
        .map(|i| root.join(format!("retained-{i}.rs")))
        .collect::<Vec<_>>();
    for path in &paths {
        fs::write(path, "retained").unwrap();
    }
    let target = root.join("src/nested/example.rs");
    let saved = SavedLayout {
        paths: paths.clone(),
        selected: paths.last().cloned(),
        external_paths: BTreeSet::from([paths[1].clone(), target.clone()]),
        source_paths: BTreeSet::from([paths[2].clone(), target.clone()]),
        ..Default::default()
    };
    let mut browser = Browser::default();
    browser.restore_layout(root, &saved);
    browser
        .open_target(&ctx, root, target.to_str().unwrap())
        .unwrap();
    assert_eq!(browser.tabs.len(), MAX_TABS);
    assert_eq!(
        browser.tabs[..MAX_TABS - 1]
            .iter()
            .map(|tab| &tab.path)
            .collect::<Vec<_>>(),
        paths[1..].iter().collect::<Vec<_>>()
    );
    assert!(browser.tabs[0].external);
    assert!(browser.tabs[1].source);
    let new = browser.tabs.last().unwrap();
    assert_eq!(new.path, target);
    assert!(new.external);
    assert!(
        !new.source,
        "Nonmember saved flags cannot attach to a new tab"
    );
    let checkpoint = browser.saved_layout();
    assert!(
        checkpoint
            .external_paths
            .iter()
            .all(|path| checkpoint.paths.contains(path))
    );
    assert!(
        checkpoint
            .source_paths
            .iter()
            .all(|path| checkpoint.paths.contains(path))
    );
}

#[cfg(target_os = "macos")]
#[test]
fn hidden_url_activation_retains_capped_order_and_bounded_file_privileges() {
    let ctx = egui::Context::default();
    let app = tests::app(&ctx);
    let root = &app.prefs.cwd;
    let mut saved = mixed_hidden_layout(root);
    saved.urls = (0..MAX_TABS)
        .map(|i| format!("https://example.com/{i}"))
        .collect();
    saved.selected_url = saved.urls.last().cloned();
    saved.external_paths.insert(root.join("nonmember.rs"));
    saved.source_paths.insert(root.join("nonmember.rs"));
    let bounded = saved.clone().bounded();
    let mut browser = Browser::default();
    browser.restore_layout(root, &saved);
    browser
        .open_target(&ctx, root, "https://example.com/new")
        .unwrap();
    assert_retained_prefix(&browser, &bounded);
    assert_eq!(browser.urls.len(), MAX_TABS);
    assert_eq!(browser.urls[..MAX_TABS - 1], saved.urls[1..]);
    let checkpoint = browser.saved_layout();
    assert_eq!(checkpoint.external_paths, bounded.external_paths);
    assert_eq!(checkpoint.source_paths, bounded.source_paths);
}

#[cfg(unix)]
#[test]
fn quick_open_downgrades_hidden_external_tab_before_workspace_validation() {
    let ctx = egui::Context::default();
    let app = tests::app(&ctx);
    let root = &app.prefs.cwd;
    let path = root.join("src/nested/example.rs");
    fs::rename(root.join("src"), root.join("old-src")).unwrap();
    std::os::unix::fs::symlink(root.join("old-src"), root.join("src")).unwrap();
    let saved = SavedLayout {
        paths: vec![root.join("README.md"), path.clone()],
        external_paths: BTreeSet::from([path.clone()]),
        selected: Some(path.clone()),
        ..Default::default()
    };
    let mut browser = Browser::default();
    browser.restore_layout(root, &saved);
    browser
        .open_workspace_source(&ctx, root, &path, Some(1))
        .unwrap();
    assert_eq!(browser.tabs.len(), 2);
    let tab = browser
        .tabs
        .iter_mut()
        .find(|tab| tab.path == path)
        .unwrap();
    assert!(!tab.external && tab.source);
    let result = tab
        .rx
        .take()
        .unwrap()
        .recv_timeout(Duration::from_secs(4))
        .unwrap();
    assert!(matches!(result, Err(error) if error.contains("Symlinked")));
}

#[test]
fn rejected_explicit_target_does_not_materialize_or_discard_hidden_checkpoint() {
    let ctx = egui::Context::default();
    let app = tests::app(&ctx);
    let root = &app.prefs.cwd;
    let saved = mixed_hidden_layout(root);
    let mut browser = Browser::default();
    browser.restore_layout(root, &saved);
    assert!(
        browser
            .open_target(&ctx, root, "javascript:alert(1)")
            .is_err()
    );
    assert_eq!(browser.restored.as_ref().unwrap().paths, saved.paths);
    assert!(browser.tabs.is_empty());
    assert!(!browser.is_open());
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
