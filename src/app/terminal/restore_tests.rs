use super::*;
use crate::app::files::tests::{app, click, frame, text};

#[test]
fn restored_terminal_stays_inert_until_opening_then_starts_without_another_button() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.prefs.theme = mode;
        let root = app.prefs.cwd.clone();
        app.terminals
            .restore_saved_layout(&ctx, &root, (true, false));
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        assert!(app.terminals.projects.is_empty());
        app.terminals.close(&ctx);
        app.terminals.toggle(&ctx, &root, true);
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.terminals.projects.len(), 1);
        assert!(
            app.terminals.projects[&root]
                .active()
                .unwrap()
                .demo
                .is_some()
        );
        app.terminals
            .projects
            .get_mut(&root)
            .unwrap()
            .active_mut()
            .unwrap()
            .demo
            .as_mut()
            .unwrap()
            .process(b"RETAINED");
        app.terminals
            .restore_saved_layout(&ctx, &root, (true, false));
        frame(&ctx, &mut app, vec![]);
        assert!(!app.terminals.restored_roots.contains(&root));
        assert!(
            app.terminals.projects[&root]
                .active()
                .unwrap()
                .demo
                .as_ref()
                .unwrap()
                .screen()
                .contents()
                .contains("RETAINED")
        );
    }
}

#[test]
fn restoring_one_project_keeps_unknown_projects_inert_until_explicit_start() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let root = app.prefs.cwd.clone();
    app.terminals
        .restore_saved_layout(&ctx, &root, (true, false));
    let other = root.join("src");
    app.prefs.cwd = other.clone();
    let output = frame(&ctx, &mut app, vec![]);
    assert!(!app.terminals.projects.contains_key(&other));
    assert!(!app.terminals.projects.contains_key(&root));
    text(&output, "Nothing was replayed");
    click(&ctx, &mut app, text(&output, "Start shell"));
    assert!(app.terminals.projects.contains_key(&other));
    app.prefs.cwd = root.clone();
    let output = frame(&ctx, &mut app, vec![]);
    text(&output, "Terminal restored");
    assert!(!app.terminals.projects.contains_key(&root));
}
