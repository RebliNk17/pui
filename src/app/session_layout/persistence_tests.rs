use super::*;
use crate::app::files::tests::app;

#[test]
fn layout_checkpoint_restores_geometry_and_visibility_without_io_or_shells() {
    let ctx = egui::Context::default();
    let mut source = app(&ctx);
    source.state.session_file = Some("/fixture/layout-restore".into());
    let cwd = source.prefs.cwd.clone();
    source.files.toggle_browser(&ctx, &cwd);
    source.inspector_panel = true;
    source.changes.set_expanded(true);
    source.terminals.set_layout(&ctx, (true, false));
    ctx.data_mut(|data| {
        data.insert_persisted(
            egui::Id::new("workspace-files-dock"),
            PanelState {
                rect: egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(510.0, 400.0)),
            },
        )
    });
    source.save_workspace_layouts();
    let value = serde_json::to_value(&source.prefs.workspace_layouts).unwrap();
    assert!(!value.to_string().contains("prompt"));
    let mut target = app(&ctx);
    target.state.session_file = Some("/fixture/layout-restore".into());
    target.prefs.workspace_layouts = serde_json::from_value(value).unwrap();
    target.restore_workspace_layout();
    assert!(target.files.browser_open());
    assert!(target.inspector_panel);
    assert!(target.changes.expanded());
    assert!(target.terminals.is_open());
    assert!(!target.terminals.running());
    assert_eq!(
        PanelState::load(&ctx, egui::Id::new("workspace-files-dock"))
            .unwrap()
            .rect
            .width(),
        510.0
    );
    assert!(
        target.prefs.workspace_layouts.is_empty(),
        "restore consumes startup checkpoint once"
    );
    target.files.close();
    target.inspector_panel = false;
    target.restore_workspace_layout();
    assert!(!target.files.is_open());
}

#[test]
fn layout_checkpoint_keeps_unopened_sessions_and_bounds_count() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.session_file = Some("/fixture/current".into());
    app.prefs.workspace_layouts = (0..100)
        .map(|index| Saved {
            key: format!("/fixture/saved-{index}"),
            ..Default::default()
        })
        .collect();
    app.save_workspace_layouts();
    assert_eq!(app.prefs.workspace_layouts.len(), 64);
    assert!(
        app.prefs
            .workspace_layouts
            .iter()
            .any(|s| s.key == "/fixture/current")
    );
    assert!(
        app.prefs
            .workspace_layouts
            .iter()
            .any(|s| s.key == "/fixture/saved-0")
    );
}

#[test]
fn app_save_and_startup_restore_layout_without_restarting_execution() {
    #[derive(Default)]
    struct Storage(std::collections::HashMap<String, String>);
    impl eframe::Storage for Storage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.into(), value);
        }
        fn flush(&mut self) {}
    }
    let ctx = egui::Context::default();
    let mut source = app(&ctx);
    let root = source.prefs.cwd.clone();
    let path = root.join("layout-session.jsonl");
    std::fs::write(
        &path,
        format!("{}\n", json!({"type":"session", "cwd":root})),
    )
    .unwrap();
    source.demo = false;
    source.new_chat = false;
    source.state = State {
        session_file: Some(path.display().to_string()),
        ..Default::default()
    };
    source.files.toggle_browser(&ctx, &root);
    source.terminals.set_layout(&ctx, (true, false));
    source.closed_projects.insert(root.clone());
    source.prefs.hide_sidebar = true;
    let mut storage = Storage::default();
    eframe::App::save(&mut source, &mut storage);
    let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    cc.storage = Some(&storage);
    let restored = Pui::new(&cc, Options::default());
    assert_eq!(restored.pending_session.as_ref(), Some(&path));
    assert!(restored.files.browser_open());
    assert!(restored.terminals.is_open());
    assert!(!restored.terminals.running());
    assert!(restored.closed_projects.contains(&root));
    assert!(restored.prefs.hide_sidebar);
    assert!(restored.state.messages.is_empty());
    assert!(!restored.state.pending);
    assert!(restored.state.dialogs.is_empty());
}

#[test]
fn invalid_panel_geometry_is_ignored_without_touching_global_sidebar_width() {
    let ctx = egui::Context::default();
    let app = app(&ctx);
    ctx.data_mut(|data| {
        data.insert_persisted(
            egui::Id::new("sessions-v4"),
            PanelState {
                rect: egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(300.0, 800.0)),
            },
        )
    });
    for width in [f32::NAN, f32::INFINITY, -100.0, 100_000.0] {
        app.set_layout_panels(&[Some(PanelState {
            rect: egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width, 400.0)),
        })]);
        assert!(PanelState::load(&ctx, egui::Id::new("workspace-files-dock")).is_none());
        assert_eq!(
            PanelState::load(&ctx, egui::Id::new("sessions-v4"))
                .unwrap()
                .rect
                .width(),
            300.0
        );
    }
}
