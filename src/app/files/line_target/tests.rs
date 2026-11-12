use super::*;
use crate::app::files::tests as file_tests;

#[test]
fn source_line_reveal_waits_for_worker_clamps_and_uses_no_external_privilege() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    let root = app.prefs.cwd.clone();
    let path = root.join("long.md");
    fs::write(
        &path,
        (1..=300)
            .map(|line| format!("line {line}\n"))
            .collect::<String>(),
    )
    .unwrap();
    app.files
        .open_workspace_source(&ctx, &root, &path, Some(200))
        .unwrap();
    assert!(!app.files.tabs[0].external);
    assert!(app.files.tabs[0].source);
    if let Some(rx) = app.files.tabs[0].rx.take() {
        app.files.tabs[0].content = Some(rx.recv_timeout(Duration::from_secs(5)).unwrap());
    }
    for _ in 0..100 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    let out = file_tests::frame(&ctx, &mut app, vec![]);
    let (clip, text) = out
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                text.galley
                    .job
                    .text
                    .starts_with("line 1\n")
                    .then_some((shape.clip_rect, text))
            } else {
                None
            }
        })
        .unwrap();
    assert!(
        clip.contains_rect(text.galley.rows[199].rect().translate(text.pos.to_vec2())),
        "requested line should actually be visible"
    );
    let target = ctx
        .data(|data| data.get_temp::<(usize, egui::Rect)>(egui::Id::new("quick-open-line")))
        .unwrap();
    assert_eq!(target.0, 200);
    assert!(app.files.tabs[0].line_target.is_none());
    assert_eq!(app.files.tabs[0].line_notice.as_deref(), Some("Line 200"));
    app.files
        .open_workspace_source(&ctx, &root, &path, Some(9999))
        .unwrap();
    for _ in 0..6 {
        file_tests::frame(&ctx, &mut app, vec![]);
    }
    assert!(
        app.files.tabs[0]
            .line_notice
            .as_deref()
            .unwrap()
            .contains("showing last line")
    );
    assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 300);
}
#[test]
fn traversal_and_outside_workspace_cannot_gain_explicit_open_privileges() {
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    let root = app.prefs.cwd.clone();
    for path in [PathBuf::from("/etc/passwd"), root.join("../outside.rs")] {
        assert!(
            app.files
                .open_workspace_source(&ctx, &root, &path, Some(1))
                .is_err()
        );
    }
    assert!(app.files.tabs.is_empty());
}
