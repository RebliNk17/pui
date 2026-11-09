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
        let heading = text(&output, "Files");
        let expand = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::LineSegment { points, .. }
                    if (points[0].x - points[1].x).abs() == 12.0
                        && (points[0].y - points[1].y).abs() == 12.0
                        && (points[0].y.min(points[1].y) + 6.0 - heading.y).abs() < 3.0 =>
                {
                    Some((points[0] + points[1].to_vec2()) / 2.0)
                }
                _ => None,
            })
            .expect("Expand files control");
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
