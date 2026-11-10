use super::*;

fn fixture() -> PathBuf {
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
fn settle(browser: &mut Browser) {
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
                egui::CentralPanel::default().show(ctx, |ui| browser.contents(ui));
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
    let source = text(&output, "Source");
    click(&ctx, &mut app, source);
    let output = frame(&ctx, &mut app, vec![]);
    text(&output, "# Workspace");
    assert!(app.files.tabs[0].source);
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

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn links_route_to_native_viewer_without_external_browser_commands() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        ctx.open_url(egui::OpenUrl::new_tab("https://example.com"));
        app.process_open_links(ctx);
    });
    assert!(app.files.open);
    assert_eq!(
        app.files.selected_url.as_deref(),
        Some("https://example.com")
    );
    // Do not build native webviews from the headless kittest Frame.
    app.files.close();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        ctx.open_url(egui::OpenUrl::new_tab("mailto:test@example.com"));
        app.process_open_links(ctx);
    });
    assert!(
        output.platform_output.commands.iter().any(
            |c| matches!(c, egui::OutputCommand::OpenUrl(url) if url.url.starts_with("mailto:"))
        )
    );
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
