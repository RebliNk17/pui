use super::*;

pub(super) fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!("pui-files-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("src/nested")).unwrap();
    fs::create_dir(root.join(".git")).unwrap();
    fs::write(root.join("README.md"), "# Workspace\nHello, 世界\n").unwrap();
    fs::write(root.join(".gitignore"), "target/\n").unwrap();
    fs::write(root.join("src/nested/example.rs"), "fn example() {}\n").unwrap();
    root
}

pub(crate) fn app(ctx: &egui::Context) -> Pui {
    Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            cwd: Some(fixture()),
            ..Default::default()
        },
    )
}
pub(crate) fn frame(
    ctx: &egui::Context,
    app: &mut Pui,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut native = eframe::Frame::_new_kittest();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1134.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut native),
    )
}
pub(crate) fn click(ctx: &egui::Context, app: &mut Pui, pos: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            ctx,
            app,
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
pub(crate) fn text(output: &egui::FullOutput, needle: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text.contains(needle) => {
                Some(t.pos + t.galley.size() / 2.0)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing text: {needle}"))
}
pub(crate) fn folder(output: &egui::FullOutput) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Path(p) if p.closed && p.points.len() == 6 => {
                Some(egui::Rect::from_points(&p.points).center())
            }
            _ => None,
        })
        .expect("Folder button")
}
pub(super) fn settle(browser: &mut Browser) {
    for directory in browser.directories.values_mut() {
        if let Some(rx) = directory.rx.take() {
            directory.listing = Some(rx.recv_timeout(Duration::from_secs(5)).unwrap());
        }
    }
    for tab in &mut browser.tabs {
        if let Some(rx) = tab.rx.take() {
            tab.content = Some(rx.recv_timeout(Duration::from_secs(5)).unwrap());
        }
    }
}

#[test]
fn docked_tree_is_right_of_preview_and_reads_only_clicked_files_while_busy() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.prefs.theme = theme;
        app.state.busy = true;
        app.draft = "Keep this draft".into();
        let messages = app.state.messages.len();
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![]);
        click(&ctx, &mut app, folder(&output));
        assert!(app.files.is_open());
        settle(&mut app.files);
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![]);
        assert!(text(&output, "Open files appear here").x < text(&output, "README.md").x);
        assert!(app.files.tabs.is_empty());
        assert_eq!(
            app.files.directories.len(),
            1,
            "Collapsed folders must not be scanned"
        );
        click(&ctx, &mut app, text(&output, "README.md"));
        settle(&mut app.files);
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![]);
        text(&output, "Hello, 世界");
        assert_eq!(app.files.tabs.len(), 1);
        assert!(app.state.busy);
        assert_eq!(app.draft, "Keep this draft");
        assert_eq!(app.state.messages.len(), messages);
        assert!(app.chat_folder_rx.is_none());
        assert!(app.demo);
        // Click the expand control to occupy the workspace without a floating window.
        let expand = ctx
            .data(|data| {
                data.get_temp::<egui::Rect>(egui::Id::new(("browser-control", "Expand viewer")))
            })
            .expect("Expand viewer control")
            .center();
        click(&ctx, &mut app, expand);
        assert!(app.files.expanded);
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![]);
        text(&output, "Hello, 世界");
        assert!(!output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text.contains("Design preview · example conversation"))));
        app.files.close_tab(0);
        assert!(app.files.selected.is_none());
        assert!(app.files.tabs.is_empty());
    }
}

#[test]
fn restored_viewer_loads_without_a_restore_gate_across_redraws() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for (width, expanded, zoom) in [
            (480.0, false, 1.0),
            (1134.0, false, 1.0),
            (1134.0, true, 1.0),
            (480.0, false, 1.4),
            (1134.0, true, 0.8),
        ] {
            let ctx = egui::Context::default();
            ctx.set_zoom_factor(zoom);
            let mut app = app(&ctx);
            app.prefs.theme = mode;
            app.prefs.hide_sidebar = true;
            app.files.toggle(&ctx, &app.prefs.cwd);
            app.files.expanded = expanded;
            app.files.select(&ctx, app.prefs.cwd.join("README.md"));
            let saved = app.files.saved_layout();
            app.files.restore_layout(&app.prefs.cwd, &saved);
            for _ in 0..40 {
                let mut native = eframe::Frame::_new_kittest();
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut native),
                );
                assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Text(text) if ["Workspace restored", "Restore tabs"].contains(&text.galley.job.text.as_str()))));
                let pos = text(&output, "README.md");
                assert!(
                    ctx.screen_rect().contains(pos),
                    "Tab must remain visible: {pos:?}"
                );
                assert!(app.files.restored.is_none());
                assert_eq!(app.files.tabs.len(), 1);
                assert!(!app.terminals.running());
                assert!(app.state.dialogs.is_empty());
            }
        }
    }
}

#[test]
fn file_explorer_omits_address_and_error_rows_but_keeps_tabs_and_refresh() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [360.0, 760.0] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            let root = fixture();
            let mut browser = Browser::default();
            browser.toggle(&ctx, &root);
            browser.select(&ctx, root.join("README.md"));
            settle(&mut browser);
            browser.address_error = Some("Invalid file path or URL.".into());
            for _ in 0..2 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .show(ctx, |ui| browser.contents(ui, &mut TableHeaders::default()));
                    },
                );
                text(&output, "Hello, 世界");
                assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Text(text) if text.galley.job.text.contains("Invalid file path or URL.")
                        || text.galley.job.text.contains(&root.display().to_string())
                )));
                assert!(
                    ctx.read_response(egui::Id::new("viewer-navigation"))
                        .is_none()
                );
                assert!(
                    ctx.read_response(egui::Id::new("viewer-address-error"))
                        .is_none()
                );
                for tip in [
                    "Refresh files and selected preview",
                    "Expand viewer",
                    "Close viewer",
                ] {
                    let rect = ctx
                        .data(|data| {
                            data.get_temp::<egui::Rect>(egui::Id::new(("browser-control", tip)))
                        })
                        .unwrap();
                    assert!(rect.left() >= 0.0 && rect.right() <= width);
                }
                assert!(
                    ctx.data(|data| {
                        data.get_temp::<egui::Rect>(egui::Id::new((
                            "browser-control",
                            "Open another file",
                        )))
                    })
                    .is_none()
                );
            }
            assert_eq!(
                browser.selected.as_deref(),
                Some(root.join("README.md").as_path())
            );
            assert_eq!(browser.tabs.len(), 1);
        }
    }
}

#[test]
fn globe_and_folder_buttons_open_separate_panes_without_navigation() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let _ = frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    let globe = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("browser-toolbar-button")))
        .unwrap()
        .center();
    let folder = folder(&output);
    assert_ne!(globe, folder);
    click(&ctx, &mut app, globe);
    assert!(app.files.browser_open());
    assert!(
        app.files.urls.is_empty(),
        "Opening the browser must not navigate or start Chromium"
    );
    assert!(app.files.web_preview.is_none());
    let output = frame(&ctx, &mut app, vec![]);
    text(&output, "Open a page");
    click(&ctx, &mut app, folder);
    assert!(app.files.files_open());
    let output = frame(&ctx, &mut app, vec![]);
    text(&output, "Open files appear here");
    click(&ctx, &mut app, globe);
    assert!(app.files.browser_open());
    click(&ctx, &mut app, globe);
    assert!(!app.files.is_open());
}

#[cfg(target_os = "macos")]
#[test]
fn browser_chrome_has_one_address_row_and_switches_tabs_in_narrow_panels() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [360.0, 760.0] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            theme.apply(&ctx);
            let root = fixture();
            let mut browser = Browser::default();
            browser
                .open_target(&ctx, &root, "http://localhost:3000")
                .unwrap();
            browser
                .open_target(&ctx, &root, "https://example.com/a/very/long/path")
                .unwrap();
            let draw = |ctx: &egui::Context, browser: &mut Browser| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| browser.contents(ui, &mut TableHeaders::default()));
            };
            let mut output = egui::FullOutput::default();
            for _ in 0..2 {
                output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| draw(ctx, &mut browser),
                );
            }
            let first = text(&output, "localhost");
            let second = text(&output, "example.com");
            assert!((first.y - second.y).abs() < 1.0);
            let preview = browser.web_preview.as_ref().unwrap();
            assert!(
                preview.rect.top() < 165.0,
                "chrome grew too tall at {width}: {:?}",
                preview.rect
            );
            for tip in [
                "Back",
                "Forward",
                "Reload page",
                "Inspect an element",
                "Open Chrome DevTools",
                "Expand viewer",
            ] {
                let rect = ctx
                    .data(|data| {
                        data.get_temp::<egui::Rect>(egui::Id::new(("browser-control", tip)))
                    })
                    .unwrap();
                assert!(
                    rect.left() >= 0.0 && rect.right() <= width,
                    "{tip} outside {width}: {rect:?}"
                );
            }
            for pressed in [true, false] {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, 700.0),
                        )),
                        events: vec![
                            egui::Event::PointerMoved(first),
                            egui::Event::PointerButton {
                                pos: first,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                        ..Default::default()
                    },
                    |ctx| draw(ctx, &mut browser),
                );
            }
            assert_eq!(
                browser.selected_url.as_deref(),
                Some("http://localhost:3000")
            );
            assert_eq!(browser.address, "http://localhost:3000");
        }
    }
}

#[test]
fn folder_expansion_and_search_find_nested_files_without_reading_content() {
    let ctx = egui::Context::default();
    let root = fixture();
    let mut browser = Browser::default();
    browser.toggle(&ctx, &root);
    settle(&mut browser);
    browser.expand(&ctx, root.join("src"));
    settle(&mut browser);
    assert!(browser.expanded_dirs.contains(&root.join("src")));
    browser.expand(&ctx, root.join("src"));
    assert!(!browser.expanded_dirs.contains(&root.join("src")));
    let search = search_paths(&root, "example", &AtomicBool::new(false)).unwrap();
    assert_eq!(search.entries.len(), 1);
    assert_eq!(search.entries[0].path, root.join("src/nested/example.rs"));
    assert!(browser.tabs.is_empty());
    browser.query = "example".into();
    browser.search_files(&ctx);
    browser.query.clear();
    browser.search_files(&ctx);
    assert!(browser.search_rx.is_none());
    assert!(browser.search.is_none());
}

#[test]
fn workspace_switch_discards_old_tabs_tree_search_and_receivers() {
    let ctx = egui::Context::default();
    let mut browser = Browser::default();
    let first = fixture();
    browser.toggle(&ctx, &first);
    browser.select(&ctx, first.join("README.md"));
    browser.query = "readme".into();
    browser.search_files(&ctx);
    let second = fixture();
    browser.workspace(&ctx, &second);
    assert_eq!(browser.root, second);
    assert!(browser.tabs.is_empty());
    assert!(browser.selected.is_none());
    assert!(browser.query.is_empty());
    assert!(browser.search_rx.is_none());
    assert_eq!(browser.directories.len(), 1);
    assert!(browser.directories.contains_key(&second));
}

#[test]
fn folders_first_hidden_files_visible_and_explicit_utf8_reads_are_read_only() {
    let root = fixture();
    let listing = list(&root, &root).unwrap();
    let names: Vec<_> = listing
        .entries
        .iter()
        .map(|e| e.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["src", ".gitignore", "README.md"]);
    assert!(!listing.truncated);
    let path = root.join("README.md");
    let original = fs::read(&path).unwrap();
    assert_eq!(read_preview(&root, &path).unwrap().as_bytes(), original);
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn explicit_open_supports_tmp_reports_and_deduplicates_tabs() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let outside = fixture().join("README.md");
    let request = json!({"type":"extension_ui_request","method":"input","title":"Pui open","id":"open-1","placeholder":json!({"version":1,"target":outside}).to_string()});
    let messages = app.state.messages.len();
    app.draft = "Keep this draft".into();
    assert!(app.open_request(&request));
    settle(&mut app.files);
    assert!(app.files.open);
    assert_eq!(app.files.selected.as_deref(), Some(outside.as_path()));
    assert!(
        matches!(&app.files.tabs[0].content, Some(Ok(Content::Text(text))) if text.contains("Hello, 世界"))
    );
    assert!(app.open_request(&request));
    assert_eq!(app.files.tabs.len(), 1);
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    text(&output, "Hello, 世界");
    assert_eq!(app.draft, "Keep this draft");
    assert_eq!(app.state.messages.len(), messages);
    assert!(!app.files.tabs[0].source);
    let source = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("file-preview-source-toggle-rect")))
        .unwrap();
    click(&ctx, &mut app, source.center());
    let output = frame(&ctx, &mut app, vec![]);
    text(&output, "# Workspace");
    assert!(app.files.tabs[0].source);
}

#[test]
fn local_html_uses_browser_tabs_and_retains_file_selection_and_source() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.prefs.theme = theme;
        let root = app.prefs.cwd.clone();
        let html = root.join("preview.HTML");
        fs::write(
            &html,
            "<button onclick=\"this.textContent='Clicked'\">Click</button>",
        )
        .unwrap();
        app.files.open_target(&ctx, &root, "README.md").unwrap();
        let file = app.files.navigation_target();
        app.files
            .open_target(&ctx, &root, &html.display().to_string())
            .unwrap();
        settle(&mut app.files);
        assert!(app.files.browser_open());
        assert!(!app.files.files_open());
        assert_eq!(app.files.urls, [html.display().to_string()]);
        frame(&ctx, &mut app, vec![]);
        let output = frame(&ctx, &mut app, vec![]);
        assert!(
            app.files
                .web_preview
                .as_ref()
                .unwrap()
                .html
                .as_ref()
                .unwrap()
                .contains("onclick")
        );
        assert!(app.files.web_preview.as_ref().unwrap().url.is_none());
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(t) if t.galley.job.text.contains("Search files"))));
        let preview = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("browser-control", "Preview"))))
            .unwrap();
        click(&ctx, &mut app, preview.center());
        let output = frame(&ctx, &mut app, vec![]);
        text(&output, "onclick");
        assert!(app.files.web_preview.is_none());
        app.files.toggle(&ctx, &root);
        assert_eq!(app.files.navigation_target(), file);
        app.files.toggle_browser(&ctx, &root);
        app.files.close_active_browser_tab();
        assert!(app.files.urls.is_empty());
        assert_eq!(app.files.tabs.len(), 1);
    }
}

#[test]
fn model_picker_only_obscures_the_browser_when_it_overlaps_the_page() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for (width, zoom) in [(1600.0, 1.0), (1000.0, 1.25)] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.prefs.theme = theme;
            theme.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            app.model_picker_open = true;
            app.model_anchor = Some((
                egui::Rect::from_min_size(egui::pos2(24.0, 720.0), egui::vec2(150.0, 28.0)),
                egui::LayerId::background(),
            ));
            app.files.web_preview = Some(WebPreview {
                rect: egui::Rect::from_min_max(egui::pos2(600.0, 180.0), egui::pos2(width, 800.0)),
                key: "report.html".into(),
                url: None,
                html: Some("<h1>Report</h1>".into()),
            });
            let input = || egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 850.0),
                )),
                ..Default::default()
            };
            for _ in 0..8 {
                let _ = ctx.run(input(), |ctx| {
                    app.model_picker(ctx);
                    assert!(!app.files.web_obscured(ctx));
                });
            }
            assert!(app.model_picker_open);
            let popup = ctx
                .read_response(egui::Id::new("model-picker-dropdown"))
                .unwrap();
            app.files.web_preview.as_mut().unwrap().rect = popup.rect;
            let _ = ctx.run(input(), |ctx| {
                app.model_picker(ctx);
                assert!(app.files.web_obscured(ctx), "Popup must cover native view");
            });
            app.model_picker_open = false;
            // egui retains last-frame areas for one frame after dismissal.
            for _ in 0..2 {
                let _ = ctx.run(input(), |ctx| app.model_picker(ctx));
            }
            assert!(!app.files.web_obscured(&ctx));
        }
    }
}

#[test]
fn browser_popup_overlap_includes_nested_menus_and_layer_transforms() {
    let ctx = egui::Context::default();
    let mut browser = Browser::default();
    browser.web_preview = Some(WebPreview {
        rect: egui::Rect::from_min_max(egui::pos2(600.0, 180.0), egui::pos2(1000.0, 800.0)),
        key: "https://example.com".into(),
        url: Some("https://example.com".into()),
        html: None,
    });
    let popup = egui::Id::new("browser-overlapping-menu");
    let layer = egui::LayerId::new(egui::Order::Foreground, popup);
    for _ in 0..3 {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 850.0),
                )),
                ..Default::default()
            },
            |ctx| {
                // A provider submenu can overlap even when its parent picker does not.
                egui::Popup::new(
                    popup,
                    ctx.clone(),
                    egui::pos2(580.0, 300.0),
                    egui::LayerId::background(),
                )
                .open_memory(Some(egui::SetOpenCommand::Bool(true)))
                .width(140.0)
                .show(|ui| {
                    ui.label("Provider submenu");
                });
                assert!(egui::Popup::is_any_open(ctx));
                assert!(browser.web_obscured(ctx));
                ctx.set_transform_layer(
                    layer,
                    egui::emath::TSTransform::from_translation(egui::vec2(-400.0, 0.0)),
                );
                assert!(!browser.web_obscured(ctx));
                ctx.set_transform_layer(layer, egui::emath::TSTransform::IDENTITY);
            },
        );
    }
    browser.web_preview = None;
    assert!(!browser.web_obscured(&ctx));
}

#[test]
fn session_switch_restores_html_tabs_expansion_and_clears_stale_native_rect() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let root = app.prefs.cwd.clone();
    let html = root.join("report.html");
    fs::write(&html, "<h1>Session A</h1>").unwrap();
    app.state.session_file = Some("/fixture/browser-a".into());
    let a = app.ensure_agent();
    app.files
        .open_target(&ctx, &root, &html.display().to_string())
        .unwrap();
    app.files.expanded = true;
    settle(&mut app.files);
    frame(&ctx, &mut app, vec![]);
    assert!(app.files.web_preview.is_some());
    app.park_tree();
    assert!(app.files.web_preview.is_none());
    app.state.session_file = Some("/fixture/browser-b".into());
    let b = app.ensure_agent();
    app.enter_session_layout();
    assert!(!app.files.is_open());
    app.files.open_target(&ctx, &root, "README.md").unwrap();
    app.view_agent(&a);
    assert!(app.files.browser_open());
    assert!(app.files.expanded);
    assert_eq!(
        app.files.selected_url.as_deref(),
        Some(html.to_str().unwrap())
    );
    assert!(
        app.files.web_preview.is_none(),
        "Never reuse the previous session's native-view rect"
    );
    frame(&ctx, &mut app, vec![]);
    assert!(app.files.web_preview.is_some());
    app.view_agent(&b);
    assert!(app.files.files_open());
    assert!(!app.files.expanded);
    assert!(app.files.urls.is_empty());
    assert!(app.files.web_preview.is_none());
}

#[test]
fn explicit_open_blocks_sensitive_aliases_and_handles_missing_or_binary_files() {
    let root = fixture();
    let outside = fixture();
    fs::write(outside.join(".env"), "secret").unwrap();
    assert!(read_content(&root, &outside.join(".env"), true).is_err());
    assert!(read_content(&root, &outside.join("missing.md"), true).is_err());
    fs::write(outside.join("binary.bin"), [0, 1, 2]).unwrap();
    assert!(read_content(&root, &outside.join("binary.bin"), true).is_err());
    fs::write(outside.join("large.txt"), "a".repeat(MAX_TEXT + 1)).unwrap();
    assert!(read_content(&root, &outside.join("large.txt"), true).is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.join(".env"), outside.join("alias.md")).unwrap();
        assert!(read_content(&root, &outside.join("alias.md"), true).is_err());
        fs::create_dir(outside.join(".ssh")).unwrap();
        fs::write(outside.join(".ssh/config"), "private").unwrap();
        std::os::unix::fs::symlink(outside.join(".ssh"), outside.join("alias-dir")).unwrap();
        assert!(read_content(&root, &outside.join("alias-dir/config"), true).is_err());
    }
    let mut browser = Browser::default();
    let ctx = egui::Context::default();
    for target in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "../README.md",
        ".env",
        "https://user:pass@example.com",
        "data:text/html,hi",
    ] {
        assert!(
            browser.open_target(&ctx, &root, target).is_err(),
            "{target}"
        );
    }
}

#[test]
fn focused_thinking_disclosure_settles_in_full_app_update() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state = State {
        messages: vec![Message::from_json(&json!({"role":"assistant","content":[
            {"type":"thinking","thinking":"Reasoning fixture"},
            {"type":"text","text":"Finished response"}
        ]}))],
        ..Default::default()
    };
    app.loading_chat = false;
    app.new_chat = false;
    app.focus_composer = false;
    app.transcript.expand_activity = true;
    app.prefs.hide_inspector = true;
    app.prefs.hide_sidebar = true;
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    click(&ctx, &mut app, text(&output, "Thinking"));
    for _ in 0..6 {
        frame(&ctx, &mut app, vec![]);
    }
    let output = frame(&ctx, &mut app, vec![]);
    assert!(
        output.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::ZERO,
        "settled focused disclosure must not drive an immediate app repaint loop"
    );
}

#[test]
fn displayed_image_transfers_pixels_once_and_keeps_its_texture() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let path = app.prefs.cwd.join("preview.png");
    fs::write(&path, include_bytes!("../../../assets/icon.png")).unwrap();
    app.files
        .open_target(&ctx, &app.prefs.cwd.clone(), &path.display().to_string())
        .unwrap();
    settle(&mut app.files);
    assert!(matches!(
        app.files.tabs[0].content,
        Some(Ok(Content::Image(_)))
    ));
    frame(&ctx, &mut app, vec![]);
    assert!(matches!(
        app.files.tabs[0].content,
        Some(Ok(Content::UploadedImage))
    ));
    let id = app.files.tabs[0].texture.as_ref().unwrap().id();
    frame(&ctx, &mut app, vec![]);
    assert_eq!(app.files.tabs[0].texture.as_ref().unwrap().id(), id);
}

#[test]
fn images_decode_off_thread_and_unknown_text_formats_still_open() {
    let root = fixture();
    fs::write(
        root.join("image.png"),
        include_bytes!("../../../assets/icon.png"),
    )
    .unwrap();
    assert!(matches!(
        read_content(&root, &root.join("image.png"), false),
        Ok(Content::Image(_))
    ));
    fs::write(root.join("notes.custom"), "hello").unwrap();
    assert!(
        matches!(read_content(&root, &root.join("notes.custom"), false), Ok(Content::Text(text)) if text == "hello")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn local_preview_links_route_to_native_viewer_without_external_commands() {
    for url in [
        "http://localhost:3000",
        "http://127.0.0.1:8000",
        "http://[::1]:8080",
    ] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.state.busy = true;
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            ctx.open_url(egui::OpenUrl::new_tab(url));
            app.process_open_links(ctx);
            app.agent_close_guard(ctx);
        });
        assert!(app.files.browser_open());
        assert_eq!(app.files.selected_url.as_deref(), Some(url));
        assert!(!app.agents.close_prompt);
        assert!(
            !output
                .platform_output
                .commands
                .iter()
                .any(|c| matches!(c, egui::OutputCommand::OpenUrl(_)))
        );
    }
}

#[test]
fn remote_links_and_mailto_use_os_handler_without_changing_the_viewer() {
    for url in [
        "https://github.com/example/browser-service/pull/40",
        "https://example.com",
        "mailto:test@example.com",
    ] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.state.busy = true;
        app.files
            .open_target(&ctx, &app.prefs.cwd, "README.md")
            .unwrap();
        let selected = app.files.selected.clone();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            ctx.open_url(egui::OpenUrl::new_tab(url));
            app.process_open_links(ctx);
            app.agent_close_guard(ctx);
        });
        assert!(app.files.files_open());
        assert_eq!(app.files.selected, selected);
        assert!(app.files.urls.is_empty());
        assert!(!app.agents.close_prompt);
        assert!(
            output
                .platform_output
                .commands
                .iter()
                .any(|c| matches!(c, egui::OutputCommand::OpenUrl(open) if open.url == url))
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn explicit_remote_open_still_uses_pui_browser() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let request = json!({"type":"extension_ui_request","method":"input","title":"Pui open","id":"open-remote","placeholder":json!({"version":1,"target":"https://example.com"}).to_string()});
    assert!(app.open_request(&request));
    assert!(app.files.browser_open());
    assert_eq!(
        app.files.selected_url.as_deref(),
        Some("https://example.com")
    );
}

#[test]
fn browser_escape_is_scoped_to_the_focused_pane_and_never_aborts_chat() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.focus_composer = false;
    app.state.busy = true;
    app.draft = "Keep this draft".into();
    let (client, _, commands) = rpc::Client::stub_with_commands();
    app.client = Some(client);
    app.files.toggle_browser(&ctx, &app.prefs.cwd);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    let pane = app.files.pane_rect.unwrap();
    click(&ctx, &mut app, pane.center());
    assert!(app.files.focused(&ctx));
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    // Flush the stub's FIFO so the absence of abort isn't a thread race.
    app.send(json!({"type":"focus-test-barrier"}));
    loop {
        let command = commands.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_ne!(command["type"], "abort", "Browser Escape must not stop Pi");
        assert_ne!(command["type"], "clear_queue");
        if command["type"] == "focus-test-barrier" {
            break;
        }
    }
    assert!(app.state.busy);
    assert_eq!(app.draft, "Keep this draft");

    // Clicking back into the chat restores its Escape shortcut, even with
    // the browser still visible beside it.
    click(
        &ctx,
        &mut app,
        egui::pos2(pane.left() - 80.0, pane.center().y),
    );
    assert!(!app.files.focused(&ctx));
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    loop {
        let command = commands.recv_timeout(Duration::from_secs(2)).unwrap();
        if command["type"] == "abort" {
            break;
        }
    }
}

#[test]
fn background_open_does_not_steal_foreground_workspace() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.agents.background = true;
    let request = json!({"type":"extension_ui_request","method":"input","title":"Pui open","id":"open-1","placeholder":json!({"version":1,"target":"README.md"}).to_string()});
    assert!(app.open_request(&request));
    assert!(!app.files.open);
    assert!(app.state.dialogs.is_empty());
    assert_eq!(
        app.preview_opens,
        [app.prefs.cwd.join("README.md").display().to_string()]
    );
    app.open_queued_previews();
    assert!(!app.files.open); // background processing cannot drain the intent
    assert_eq!(app.preview_opens.len(), 1);
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn file_and_browser_panes_preserve_their_own_selection() {
    let ctx = egui::Context::default();
    let root = fixture();
    let mut browser = Browser::default();
    browser.open_target(&ctx, &root, "README.md").unwrap();
    browser
        .open_target(&ctx, &root, "https://example.com")
        .unwrap();
    assert!(browser.browser_open());
    assert_eq!(
        browser.selected.as_deref(),
        Some(root.join("README.md").as_path())
    );
    browser.toggle(&ctx, &root);
    assert!(browser.files_open());
    assert!(browser.address.ends_with("README.md"));
    assert_eq!(browser.selected_url.as_deref(), Some("https://example.com"));
    browser.toggle_browser(&ctx, &root);
    assert!(browser.browser_open());
    assert_eq!(browser.address, "https://example.com");
    browser.toggle(&ctx, &root);
    browser.close_tab(0);
    assert!(browser.files_open());
    assert!(browser.selected.is_none());
    assert_eq!(browser.selected_url.as_deref(), Some("https://example.com"));
}

#[test]
fn blocks_sensitive_binary_oversized_missing_and_symlinked_files() {
    let root = fixture();
    fs::write(root.join("large.txt"), "a".repeat(MAX_TEXT + 1)).unwrap();
    fs::write(root.join("binary"), [0, 1, 2]).unwrap();
    fs::write(root.join(".env.local"), "PRIVATE=value").unwrap();
    fs::write(root.join("credentials.json"), "{}").unwrap();
    for file in [
        "large.txt",
        "binary",
        ".env.local",
        "credentials.json",
        "missing",
        "src",
    ] {
        assert!(read_preview(&root, &root.join(file)).is_err(), "{file}");
    }
    let outside = fixture();
    assert!(read_preview(&root, &outside.join("README.md")).is_err());
    assert!(read_preview(&root, &root.join("src/../README.md")).is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("README.md"), root.join("internal-link")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("external")).unwrap();
        assert!(read_preview(&root, &root.join("internal-link")).is_err());
        assert!(list(&root, &root.join("external")).is_err());
        let listing = list(&root, &root).unwrap();
        assert!(
            listing
                .entries
                .iter()
                .filter(|e| e.path.ends_with("internal-link") || e.path.ends_with("external"))
                .all(|e| e.blocked.is_some())
        );
    }
    assert!(
        search_paths(&root, ".env", &AtomicBool::new(false))
            .unwrap()
            .entries
            .is_empty()
    );
}
