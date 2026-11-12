use super::*;
use crate::app::files::tests as file_tests;

fn open(app: &mut Pui, path: &Path) {
    let ctx = app.ctx.clone();
    app.files
        .open_target(&ctx, &app.prefs.cwd.clone(), path.to_str().unwrap())
        .unwrap();
    let tab = app
        .files
        .tabs
        .iter_mut()
        .find(|tab| tab.path == path)
        .unwrap();
    if let Some(rx) = tab.rx.take() {
        tab.content = Some(rx.recv_timeout(Duration::from_secs(5)).unwrap());
    }
    for _ in 0..4 {
        file_tests::frame(&ctx, app, vec![]);
    }
}
fn header(app: &Pui) -> bool {
    let tab = app
        .files
        .tabs
        .iter()
        .find(|tab| Some(&tab.path) == app.files.selected.as_ref())
        .unwrap();
    let Some(Ok(Content::Table(preview))) = &tab.content else {
        panic!("expected table");
    };
    preview.header()
}
fn toggle(app: &mut Pui) {
    let ctx = app.ctx.clone();
    let out = file_tests::frame(&ctx, app, vec![]);
    let pos = file_tests::text(&out, "First record is header");
    file_tests::click(&ctx, app, pos);
    file_tests::frame(&ctx, app, vec![]);
}
#[test]
fn real_checkbox_remembers_each_file_through_refresh_close_reopen_and_default_reset() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    let root = app.prefs.cwd.clone();
    let csv = root.join("headerless.csv");
    let tsv = root.join("independent.tsv");
    let source = "Alpha,שלום\nBeta,=1+2\n";
    fs::write(&csv, source).unwrap();
    fs::write(&tsv, "Name\tNote\nOne\tTwo\n").unwrap();
    app.draft = "Kept draft".into();
    app.state.busy = true;
    let count = app.state.messages.len();
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    open(&mut app, &csv);
    assert!(header(&app));
    toggle(&mut app);
    assert!(!header(&app));
    assert!(!app.prefs.table_headers.header(&root, &csv));
    app.files.refresh(&ctx);
    open(&mut app, &csv);
    assert!(!header(&app));
    open(&mut app, &tsv);
    assert!(header(&app));
    app.files.close();
    app.files.open = true;
    open(&mut app, &csv);
    assert!(!header(&app));
    let index = app
        .files
        .tabs
        .iter()
        .position(|tab| tab.path == csv)
        .unwrap();
    app.files.close_tab(index);
    open(&mut app, &csv);
    assert!(!header(&app));
    toggle(&mut app);
    assert!(header(&app));
    assert!(app.prefs.table_headers.0.is_empty());
    assert_eq!(fs::read_to_string(&csv).unwrap(), source);
    assert_eq!(app.draft, "Kept draft");
    assert_eq!(app.state.messages.len(), count);
    assert!(app.state.busy);
    assert!(commands.try_recv().is_err());
}
#[cfg(unix)]
#[test]
fn remembered_external_symlink_replacement_still_fails_on_worker_reread() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    let root = app.prefs.cwd.clone();
    let file = root.join("safe.csv");
    fs::write(&file, "first,second\nthird,fourth\n").unwrap();
    open(&mut app, &file);
    toggle(&mut app);
    assert!(!header(&app));
    fs::rename(&file, root.join("original.csv")).unwrap();
    let outside = root.with_extension("external-csv");
    fs::write(&outside, "PRIVATE,DO_NOT_READ\n").unwrap();
    std::os::unix::fs::symlink(&outside, &file).unwrap();
    app.files.refresh(&ctx);
    open(&mut app, &file);
    let tab = app.files.tabs.iter().find(|tab| tab.path == file).unwrap();
    assert!(matches!(&tab.content, Some(Err(error)) if error.contains("Symlinked")));
    assert!(!app.prefs.table_headers.header(&root, &file));
    let stored = serde_json::to_string(&app.prefs.table_headers).unwrap();
    assert!(!stored.contains("PRIVATE"));
}

#[test]
fn metadata_survives_unloaded_restore_without_reads_or_widened_privileges() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    let root = app.prefs.cwd.clone();
    let path = root.join("restore.csv");
    fs::write(&path, "first,second\nthird,fourth\n").unwrap();
    open(&mut app, &path);
    toggle(&mut app);
    let saved = app.files.saved_layout();
    let encoded = serde_json::to_string(&app.prefs.table_headers).unwrap();
    let mut tables: TableHeaders = serde_json::from_str(&encoded).unwrap();
    let mut browser = Browser::default();
    browser.restore_layout(&root, &saved);
    assert!(browser.tabs.is_empty() && browser.directories.is_empty());
    for _ in 0..3 {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1000.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| {
                browser.show(ctx, &root, &mut tables);
            },
        );
    }
    assert!(browser.tabs.is_empty() && browser.directories.is_empty());
    assert!(!tables.header(&root, &path));
    browser.activate_restored_tabs(&ctx);
    let tab = &mut browser.tabs[0];
    tab.content = Some(
        tab.rx
            .take()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap(),
    );
    assert!(tab.external == saved.external_paths.contains(&path));
    for _ in 0..3 {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1000.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| {
                browser.show(ctx, &root, &mut tables);
            },
        );
    }
    let Some(Ok(Content::Table(preview))) = &browser.tabs[0].content else {
        panic!();
    };
    assert!(!preview.header());
}
#[test]
fn headerless_demo_uses_real_preference_without_writing_or_hiding_first_record() {
    let ctx = egui::Context::default();
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(ctx.clone()),
        Options {
            demo: true,
            demo_files: true,
            demo_table_grid: true,
            demo_table_headerless: true,
            ..Default::default()
        },
    );
    for _ in 0..5 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    let out = file_tests::frame(&ctx, &mut app, vec![]);
    assert!(!header(&app));
    assert_eq!(app.prefs.table_headers.0.len(), 1);
    // This focused-column demo may scroll column 1 offscreen in the narrow
    // tool dock; the virtualized first-record reachability test covers it.
    file_tests::text(&out, "Column 2");
    assert!(app.client.is_none() && app.files.web_preview.is_none());
}

#[test]
fn same_external_file_in_other_workspace_does_not_inherit_header_choice() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    let root = app.prefs.cwd.clone();
    let path = root.join("shared.csv");
    fs::write(&path, "A,B\nC,D\n").unwrap();
    open(&mut app, &path);
    toggle(&mut app);
    assert!(!header(&app));
    let other = root.join("other-workspace");
    fs::create_dir(&other).unwrap();
    app.prefs.cwd = other;
    open(&mut app, &path);
    assert!(header(&app));
    assert_eq!(app.prefs.table_headers.0.len(), 1);
    app.prefs.cwd = root;
    open(&mut app, &path);
    assert!(!header(&app));
}
#[test]
fn narrow_theme_zoom_toggle_keeps_first_record_visible_and_settles_with_metadata() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.0, 1.4] {
            let ctx = egui::Context::default();
            let mut preview = delimited::Preview::new("Alpha,שלום\nBeta,=1+2\n".into(), b',');
            let mut tables = TableHeaders::default();
            let root = Path::new("/workspace");
            let path = root.join("headerless.csv");
            theme::install(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut draw = |events| {
                let time = ctx.input(|i| i.time) + 0.016;
                ctx.run(
                    egui::RawInput {
                        time: Some(time),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(390.0, 560.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let old = tables.header(root, &path);
                            preview.set_header(old);
                            preview.show(ui, &path);
                            if preview.header() != old {
                                tables.set(root, &path, preview.header());
                            }
                        });
                    },
                )
            };
            let mut out = draw(vec![]);
            for _ in 0..4 {
                out = draw(vec![]);
            }
            let pos = file_tests::text(&out, "First record is header");
            for pressed in [true, false] {
                draw(vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
            }
            for _ in 0..100 {
                out = draw(vec![]);
            }
            file_tests::text(&out, "Alpha");
            file_tests::text(&out, "Column 1");
            assert!(
                out.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::from_secs(1)
            );
            assert!(!tables.header(root, &path));
        }
    }
}
