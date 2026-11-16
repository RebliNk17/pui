use super::*;
use crate::app::files::tests as file_tests;

fn loaded(app: &mut Pui) {
    let tab = app.files.tabs.last_mut().unwrap();
    if let Some(rx) = tab.rx.take() {
        tab.content = Some(rx.recv_timeout(Duration::from_secs(5)).unwrap());
    }
}
#[test]
fn html_quick_open_stays_source_without_browser_or_hidden_tabs_even_after_refresh() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    let root = app.prefs.cwd.clone();
    let path = root.join("document.html");
    fs::write(&path, "<!doctype html>\n<script>never_run()</script>\n").unwrap();
    app.files
        .open_workspace_source(&ctx, &root, &path, Some(2))
        .unwrap();
    loaded(&mut app);
    assert!(app.files.urls.is_empty());
    assert!(!app.files.browser_mode);
    assert!(app.files.tabs[0].source);
    for _ in 0..5 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    assert!(app.files.web_preview.is_none());
    assert_eq!(app.files.tabs[0].line_notice.as_deref(), Some("Line 2"));
    let out = file_tests::frame(&ctx, &mut app, vec![]);
    file_tests::text(&out, "document.html");
    app.files.refresh(&ctx);
    loaded(&mut app);
    assert!(!app.files.browser_mode && app.files.tabs[0].source && app.files.urls.is_empty());
    for _ in 0..3 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    assert!(app.files.web_preview.is_none());
    // Switching to Preview must not make this source-opened file vanish from
    // Files when another tab is selected. Preview remains an explicit action.
    app.files.tabs[0].source = false;
    assert!(app.files.file_tab(&app.files.tabs[0]));
    app.files.close_tab(0);
    assert!(app.files.selected.is_none());
}
#[cfg(unix)]
#[test]
fn cached_external_source_is_rechecked_and_replaced_symlink_is_never_read() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    let root = app.prefs.cwd.clone();
    let path = root.join("cached.rs");
    fs::write(&path, "safe").unwrap();
    app.files
        .open_target(&ctx, &root, path.to_str().unwrap())
        .unwrap();
    loaded(&mut app);
    assert!(app.files.tabs[0].external);
    let outside = root.with_extension("outside");
    fs::write(&outside, "PRIVATE_CONTENT_MUST_NOT_APPEAR").unwrap();
    // Rename fixture, never remove user files, then replace it with a symlink.
    fs::rename(&path, root.join("old.rs")).unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    app.files
        .open_workspace_source(&ctx, &root, &path, Some(1))
        .unwrap();
    loaded(&mut app);
    assert!(!app.files.tabs[0].external);
    let Some(Err(error)) = &app.files.tabs[0].content else {
        panic!("must reject symlink");
    };
    assert!(error.contains("Symlinked"));
}
#[test]
fn delimited_line_request_reveals_physical_source_line_not_csv_record_and_consumes_once() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    let root = app.prefs.cwd.clone();
    let path = root.join("many.csv");
    fs::write(
        &path,
        format!(
            "name,note\nfirst,\"one\ntwo\"\n{}",
            (1..=300)
                .map(|n| format!("row{n},body{n}\n"))
                .collect::<String>()
        ),
    )
    .unwrap();
    app.files
        .open_workspace_source(&ctx, &root, &path, Some(204))
        .unwrap();
    loaded(&mut app);
    let mut out = file_tests::frame(&ctx, &mut app, vec![]);
    for _ in 0..5 {
        out = file_tests::frame(&ctx, &mut app, vec![]);
    }
    let shown: Vec<_> = out
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                Some(t.galley.job.text.clone())
            } else {
                None
            }
        })
        .collect();
    assert!(
        shown.iter().any(|text| text.contains("row201,body201")),
        "{shown:?}"
    );
    assert!(app.files.tabs[0].line_target.is_none());
    assert_eq!(app.files.tabs[0].line_notice.as_deref(), Some("Line 204"));
    assert!(matches!(
        app.files.tabs[0].content,
        Some(Ok(Content::Table(_)))
    ));
}
