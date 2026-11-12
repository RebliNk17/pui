use super::*;
use crate::app::files::tests::{app, frame};

fn session(path: &str, cwd: &std::path::Path) -> Session {
    Session {
        path: path.into(),
        cwd: cwd.into(),
        project: cwd.into(),
        title: path.into(),
        created: None,
        modified: std::time::SystemTime::now(),
    }
}

fn panel(ctx: &egui::Context, name: &str, width: f32) {
    ctx.data_mut(|data| {
        data.insert_persisted(
            egui::Id::new(name),
            PanelState {
                rect: egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width, 300.0)),
            },
        );
    });
}

#[test]
fn parked_chats_restore_their_own_panes_and_keep_sidebar_width_shared() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.prefs.theme = theme;
        app.state.session_file = Some("/fixture/layout-a".into());
        let a = app.ensure_agent();
        let root = app.prefs.cwd.clone();
        let target = root.join("README.md").display().to_string();
        app.files.open_target(&ctx, &root, &target).unwrap();
        app.files.toggle_browser(&ctx, &root);
        app.prefs.hide_inspector = true;
        app.prefs.hide_sidebar = true;
        app.terminals.set_layout(&ctx, (true, true));
        panel(&ctx, "workspace-files-dock", 630.0);
        panel(&ctx, "project-terminal", 510.0);
        panel(&ctx, "sessions-v4", 280.0);
        app.park_tree();
        assert!(!app.files.is_open());
        assert!(!app.terminals.is_open());

        app.state.session_file = Some("/fixture/layout-b".into());
        let b = app.ensure_agent();
        app.enter_session_layout();
        assert!(!app.files.is_open());
        assert!(!app.prefs.hide_inspector);
        assert!(!app.prefs.hide_sidebar);
        assert!(PanelState::load(&ctx, egui::Id::new("workspace-files-dock")).is_none());
        app.files.toggle(&ctx, &root);
        app.inspector_panel = true;
        panel(&ctx, "workspace-files-dock", 420.0);
        panel(&ctx, "sessions-v4", 330.0);

        app.view_agent(&a);
        assert!(app.files.browser_open());
        assert_eq!(app.files.navigation_target(), None);
        assert!(app.prefs.hide_inspector);
        assert!(app.prefs.hide_sidebar);
        assert_eq!(app.terminals.layout(), (true, true));
        assert_eq!(
            PanelState::load(&ctx, egui::Id::new("workspace-files-dock"))
                .unwrap()
                .size()
                .x,
            630.0
        );
        assert_eq!(
            PanelState::load(&ctx, egui::Id::new("sessions-v4"))
                .unwrap()
                .size()
                .x,
            330.0
        );
        app.files.toggle(&ctx, &root);
        assert_eq!(
            app.files.navigation_target().as_deref(),
            Some(target.as_str())
        );

        app.view_agent(&b);
        assert!(app.files.files_open());
        assert!(app.inspector_panel);
        assert!(!app.prefs.hide_inspector);
        assert!(!app.prefs.hide_sidebar);
        assert!(!app.terminals.is_open());
        assert_eq!(
            PanelState::load(&ctx, egui::Id::new("workspace-files-dock"))
                .unwrap()
                .size()
                .x,
            420.0
        );
    }
}

#[test]
fn same_directory_rpc_switch_and_superseded_connection_isolate_layouts() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state = State {
        session_file: Some("/fixture/layout-a".into()),
        ..Default::default()
    };
    app.agents = Default::default();
    app.client = Some(rpc::Client::stub());
    app.connected = true;
    let root = app.prefs.cwd.clone();
    app.files.toggle_browser(&ctx, &root);
    app.open_chat(session("/fixture/layout-b", &root));
    assert!(!app.files.is_open());
    app.files.toggle(&ctx, &root);
    // Supersede the pending switch, without waiting for RPC completion.
    app.open_chat(session("/fixture/layout-a", &root));
    assert!(app.files.browser_open());
    app.open_chat(session("/fixture/layout-b", &root));
    assert!(app.files.files_open());
}

#[test]
fn deleted_chat_and_agent_layouts_release_browsers_without_affecting_other_chats() {
    let mut layouts = SessionLayouts::default();
    for key in [
        "/fixture/deleted",
        "/fixture/kept",
        "agent:deleted",
        "agent:kept",
    ] {
        layouts.saved.insert(key.into(), Layout::default());
    }
    layouts.forget_path(std::path::Path::new("/fixture/deleted"));
    layouts.forget_agent("deleted");
    assert_eq!(layouts.saved.len(), 2);
    assert!(layouts.saved.contains_key("/fixture/kept"));
    assert!(layouts.saved.contains_key("agent:kept"));
}

#[test]
fn new_conversation_does_not_inherit_previous_browser() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.demo = false;
    app.client = Some(rpc::Client::stub());
    app.state.session_file = Some("/fixture/layout-a".into());
    let a = app.ensure_agent();
    let root = app.prefs.cwd.clone();
    app.files.toggle_browser(&ctx, &root);
    app.start_chat_in(root);
    assert!(!app.files.is_open());
    app.view_agent(&a);
    assert!(app.files.browser_open());
}

#[test]
fn background_chat_work_does_not_swap_visible_layout() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.session_file = Some("/fixture/layout-a".into());
    app.ensure_agent();
    let root = app.prefs.cwd.clone();
    app.files.toggle_browser(&ctx, &root);
    app.park_tree();
    app.state.session_file = Some("/fixture/layout-b".into());
    app.ensure_agent();
    app.files.toggle(&ctx, &root);
    app.with_live_chat(std::path::Path::new("/fixture/layout-a"), |app| {
        app.state.notice = Some("Background result".into());
    });
    assert!(app.files.files_open());
    // Rendering the active session cannot select a hidden session's browser.
    frame(&ctx, &mut app, vec![]);
    assert!(app.files.files_open());
}
