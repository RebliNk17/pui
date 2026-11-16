use super::*;
use crate::app::files::tests::app;

fn metadata(root: &std::path::Path, labels: &[u64], selected: u64) -> terminal::SavedTabs {
    serde_json::from_value(json!({"projects":[{"root":root,"labels":labels,"selected":selected}]}))
        .unwrap()
}
fn labels(saved: &terminal::SavedTabs, root: &std::path::Path) -> Vec<u64> {
    let value = serde_json::to_value(saved).unwrap();
    value["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["root"] == json!(root))
        .unwrap()["labels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|label| label.as_u64().unwrap())
        .collect()
}

#[test]
fn checkpoint_restores_all_hidden_projects_once_and_legacy_layout_tuple_still_works() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let root = app.prefs.cwd.clone();
    let hidden = root.join("src");
    app.state.session_file = Some("/fixture/tabs-a".into());
    app.prefs.workspace_layouts = serde_json::from_value(json!([
        {"key":"/fixture/tabs-a","terminal":[true,false],"terminal_tabs":{
            "projects":[{"root":root,"labels":[2,5,8],"selected":5},
                        {"root":hidden,"labels":[3,7],"selected":7}]}},
        {"key":"/fixture/tabs-b","terminal":[false,true],"terminal_tabs":{
            "projects":[{"root":root,"labels":[99],"selected":99}]}},
        {"key":"/fixture/legacy","terminal":[true,true]}
    ]))
    .unwrap();
    app.restore_workspace_layout();
    assert_eq!(app.terminals.total_tabs(), 5);
    assert!(!app.terminals.running());
    assert!(app.terminals.is_open());
    assert_eq!(labels(&app.terminals.saved_tabs(), &root), vec![2, 5, 8]);
    assert_eq!(labels(&app.terminals.saved_tabs(), &hidden), vec![3, 7]);
    assert!(
        app.prefs
            .workspace_layouts
            .iter()
            .all(|layout| layout.terminal_tabs.is_none())
    );
    app.leave_session_layout();
    app.state.session_file = Some("/fixture/tabs-b".into());
    app.enter_session_layout();
    assert!(!app.terminals.is_open());
    assert_eq!(labels(&app.terminals.saved_tabs(), &root), vec![2, 5, 8]);
    app.leave_session_layout();
    app.state.session_file = Some("/fixture/tabs-a".into());
    app.enter_session_layout();
    assert!(app.terminals.is_open());
    assert_eq!(app.terminals.total_tabs(), 5);
    app.terminals.shutdown();
    app.terminals.close(&ctx);
    app.restore_workspace_layout();
    assert_eq!(
        app.terminals.total_tabs(),
        0,
        "consumed presentation must not respawn"
    );
    app.state.session_file = Some("/fixture/legacy".into());
    app.restore_workspace_layout();
    assert_eq!(app.terminals.layout(), (true, true));
    assert!(!app.terminals.running());
}

#[test]
fn save_places_one_window_snapshot_in_current_layout_and_preserves_unopened_metadata() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let root = app.prefs.cwd.clone();
    let hidden = root.join("src");
    app.state.session_file = Some("/fixture/current-tabs".into());
    app.terminals.restore_tabs(&metadata(&root, &[], 0));
    app.prefs.workspace_layouts = vec![
        Saved {
            key: "/fixture/unopened-tabs".into(),
            terminal_tabs: Some(metadata(&hidden, &[4, 8], 8)),
            ..Default::default()
        },
        Saved {
            key: "/fixture/stale-tabs".into(),
            terminal_tabs: Some(metadata(&root, &[1, 2, 3], 2)),
            ..Default::default()
        },
    ];
    app.save_workspace_layouts();
    assert_eq!(
        app.prefs
            .workspace_layouts
            .iter()
            .filter(|layout| layout.terminal_tabs.is_some())
            .count(),
        1
    );
    let current = app
        .prefs
        .workspace_layouts
        .iter()
        .find(|layout| layout.key == "/fixture/current-tabs")
        .unwrap();
    let tabs = current.terminal_tabs.as_ref().unwrap();
    assert!(
        labels(tabs, &root).is_empty(),
        "stale metadata cannot undo final-close"
    );
    assert_eq!(labels(tabs, &hidden), vec![4, 8]);
    app.save_workspace_layouts();
    let current = app
        .prefs
        .workspace_layouts
        .iter()
        .find(|layout| layout.key == "/fixture/current-tabs")
        .unwrap();
    assert_eq!(
        labels(current.terminal_tabs.as_ref().unwrap(), &hidden),
        vec![4, 8]
    );
}

#[test]
fn storage_save_reopen_preserves_multiple_placeholders_without_output_or_execution_replay() {
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
    let path = root.join("terminal-layout-session.jsonl");
    std::fs::write(&path, format!("{}\n", json!({"type":"session","cwd":root}))).unwrap();
    source.terminals.toggle(&ctx, &root, true);
    source.terminals.demo_tabs(&ctx);
    source.terminals.close(&ctx);
    source
        .terminals
        .restore_tabs(&metadata(&root.join("src"), &[6, 9], 9));
    source.demo = false;
    source.new_chat = false;
    source.state = State {
        session_file: Some(path.display().to_string()),
        ..Default::default()
    };
    let mut storage = Storage::default();
    eframe::App::save(&mut source, &mut storage);
    let saved = serde_json::to_value(&source.prefs.workspace_layouts)
        .unwrap()
        .to_string();
    assert!(!saved.contains("independent demo screen") && !saved.contains("pwd"));
    let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    cc.storage = Some(&storage);
    let mut restored = Pui::new(&cc, Options::default());
    assert_eq!(restored.pending_session.as_ref(), Some(&path));
    assert_eq!(restored.terminals.total_tabs(), 5);
    assert!(!restored.terminals.is_open());
    assert!(!restored.terminals.running());
    assert_eq!(
        labels(&restored.terminals.saved_tabs(), &root),
        vec![1, 2, 3]
    );
    restored.terminals.toggle(&cc.egui_ctx, &root, true);
    assert_eq!(restored.terminals.total_tabs(), 5);
    assert!(!restored.terminals.running());
    assert!(
        restored.state.messages.is_empty()
            && !restored.state.pending
            && restored.state.dialogs.is_empty()
    );
}
