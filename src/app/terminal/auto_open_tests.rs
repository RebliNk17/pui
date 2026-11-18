use super::*;
use crate::app::files::tests::{app, click, frame};

#[test]
fn toolbar_open_activates_only_selected_restored_shell_without_second_click() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let ctx = egui::Context::default();
            ctx.set_zoom_factor(zoom);
            let mut app = app(&ctx);
            app.prefs.theme = mode;
            let root = app.prefs.cwd.clone();
            let saved = serde_json::from_value(json!({"projects":[
                {"root":root,"labels":[2,4,7],"selected":4},
                {"root":root.join("src"),"labels":[3],"selected":3}
            ]}))
            .unwrap();
            app.terminals.restore_tabs(&saved);
            app.terminals
                .restore_saved_layout(&ctx, &root, (false, false));
            app.draft = "keep this draft".into();
            let messages = app.state.messages.len();
            for _ in 0..3 {
                frame(&ctx, &mut app, vec![]);
            }
            assert!(
                app.terminals
                    .projects
                    .values()
                    .all(|project| project.tabs.iter().all(|tab| tab.restored))
            );
            let ids: Vec<_> = app.terminals.projects[&root]
                .tabs
                .iter()
                .map(|tab| tab.id)
                .collect();
            let button = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("terminal-toolbar-button")))
                .unwrap();
            click(&ctx, &mut app, button.center());
            for _ in 0..3 {
                frame(&ctx, &mut app, vec![]);
            }
            let project = &app.terminals.projects[&root];
            assert_eq!(project.selected, 1);
            assert_eq!(
                project.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>(),
                ids
            );
            assert!(project.tabs[1].demo.is_some());
            assert!(!project.tabs[1].restored);
            assert!(project.tabs[0].restored && project.tabs[2].restored);
            assert!(app.terminals.projects[&root.join("src")].tabs[0].restored);
            assert_eq!(app.terminals.total_tabs(), 4);
            assert_eq!(app.draft, "keep this draft");
            assert_eq!(app.state.messages.len(), messages);
            // Hide/reopen reuses the live screen and doesn't create another shell.
            app.terminals.close(&ctx);
            app.terminals.toggle(&ctx, &root, true);
            assert_eq!(app.terminals.total_tabs(), 4);
            assert_eq!(app.terminals.projects[&root].tabs[1].id, ids[1]);
        }
    }
}

#[test]
fn opening_empty_restored_or_unknown_project_launches_once_and_respects_capacity() {
    let ctx = egui::Context::default();
    let mut terminal = Terminals::default();
    let root = Path::new("/empty-restored");
    terminal.restore_tabs(
        &serde_json::from_value(json!({"projects":[{"root":root,"labels":[]}]})).unwrap(),
    );
    terminal.toggle(&ctx, root, true);
    assert_eq!(terminal.projects[root].tabs.len(), 1);
    assert!(terminal.projects[root].tabs[0].demo.is_some());
    let id = terminal.projects[root].tabs[0].id;
    for _ in 0..3 {
        terminal.workspace(&ctx, root, true);
    }
    assert_eq!(terminal.projects[root].tabs[0].id, id);
    terminal.close_tab(&ctx, root, id);
    terminal.workspace(&ctx, root, true);
    assert!(terminal.projects[root].tabs.is_empty()); // rendering doesn't undo close
    terminal.close(&ctx);
    terminal.toggle(&ctx, root, true);
    assert_eq!(terminal.projects[root].tabs.len(), 1);
    for project in 0..4 {
        let other = PathBuf::from(format!("/capacity-{project}"));
        terminal.close(&ctx);
        terminal.toggle(&ctx, &other, true);
        for _ in 1..tabs::MAX_TABS {
            terminal.start(&ctx, true);
        }
    }
    assert_eq!(terminal.total_tabs(), tabs::MAX_TOTAL_TABS);
    terminal.close(&ctx);
    terminal.toggle(&ctx, Path::new("/overflow"), true);
    assert!(!terminal.projects.contains_key(Path::new("/overflow")));
    assert_eq!(terminal.total_tabs(), tabs::MAX_TOTAL_TABS);
}

#[test]
fn terminal_header_has_no_reload_or_duplicate_new_shell_action() {
    let source = include_str!("../terminal.rs");
    assert!(!source.contains("Icon::Refresh"));
    assert!(!source.contains("Start a new terminal"));
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let root = app.prefs.cwd.clone();
    app.terminals.toggle(&ctx, &root, true);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let (_, add, enabled) = ctx
        .data(|data| {
            data.get_temp::<(egui::Id, egui::Rect, bool)>(egui::Id::new("terminal-tab-add"))
        })
        .unwrap();
    assert!(enabled);
    click(&ctx, &mut app, add.center());
    assert_eq!(app.terminals.projects[&root].tabs.len(), 2);
}
