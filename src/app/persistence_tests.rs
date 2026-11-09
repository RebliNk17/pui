use super::*;

#[derive(Default)]
struct MemoryStorage(std::collections::HashMap<String, String>);
impl eframe::Storage for MemoryStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.into(), value);
    }
    fn flush(&mut self) {}
}

fn fixture() -> (PathBuf, PathBuf) {
    let cwd = std::env::temp_dir().join(format!("pui-reopen-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&cwd).unwrap();
    let path = cwd.join("chat.jsonl");
    std::fs::write(&path, format!("{}\n", json!({"type":"session","cwd":cwd}))).unwrap();
    (cwd, path)
}

fn reopen(storage: &MemoryStorage, options: Options) -> Pui {
    let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    cc.storage = Some(storage);
    Pui::new(&cc, options)
}

#[test]
fn save_and_reopen_restore_the_visible_chat_and_both_sidebar_views() {
    let (cwd, path) = fixture();
    for projects in [true, false] {
        let mut storage = MemoryStorage::default();
        let mut app = reopen(
            &storage,
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        app.new_chat = false;
        app.projects = projects;
        app.prefs.cwd = cwd.clone();
        app.state = State {
            session_file: Some(path.display().to_string()),
            ..Default::default()
        };
        // A transient model/settings view should not replace the selected chat.
        app.settings_open = true;
        eframe::App::save(&mut app, &mut storage);

        let restored = reopen(&storage, Options::default());
        assert_eq!(restored.projects, projects);
        assert_eq!(restored.prefs.cwd, cwd);
        assert_eq!(restored.pending_session.as_ref(), Some(&path));
        assert_eq!(restored.state.session_file.as_deref(), path.to_str());
        assert!(!restored.new_chat);
        assert!(restored.loading_chat);
        assert!(!restored.settings_open);
        assert!(!restored.state.pending);
        assert!(restored.draft.is_empty());
    }
}

#[test]
fn startup_selection_respects_cli_overrides_and_ignores_missing_saved_chats() {
    let (cwd, path) = fixture();
    let prefs = Preferences {
        last_session: Some(path.clone()),
        ..Default::default()
    };
    assert_eq!(prefs.startup_session(&Options::default()), Some(path));
    assert!(
        prefs
            .startup_session(&Options {
                cwd: Some(cwd.clone()),
                ..Default::default()
            })
            .is_none()
    );
    let explicit = cwd.join("explicit.jsonl");
    assert_eq!(
        prefs.startup_session(&Options {
            cwd: Some(cwd.clone()),
            session: Some(explicit.clone()),
            ..Default::default()
        }),
        Some(explicit)
    );
    assert!(
        prefs
            .startup_session(&Options {
                demo: true,
                ..Default::default()
            })
            .is_none()
    );

    let mut storage = MemoryStorage::default();
    eframe::set_value(
        &mut storage,
        "preferences",
        &Preferences {
            cwd: cwd.clone(),
            projects_view: true,
            last_session: Some(cwd.join("deleted.jsonl")),
            ..Default::default()
        },
    );
    let restored = reopen(&storage, Options::default());
    assert!(restored.projects);
    assert!(restored.new_chat);
    assert!(restored.pending_session.is_none());
    assert!(restored.state.session_file.is_none());
}

#[test]
fn blank_chat_clears_previous_selection_and_previews_never_overwrite_preferences() {
    let (cwd, path) = fixture();
    let mut storage = MemoryStorage::default();
    eframe::set_value(
        &mut storage,
        "preferences",
        &Preferences {
            cwd,
            last_session: Some(path),
            projects_view: true,
            ..Default::default()
        },
    );
    let mut app = reopen(
        &storage,
        Options {
            demo: true,
            ..Default::default()
        },
    );
    let saved = storage.0.clone();
    app.projects = false;
    eframe::App::save(&mut app, &mut storage);
    assert_eq!(storage.0, saved);
    app.demo = false;
    app.screenshot = Some(PathBuf::from("preview.png"));
    eframe::App::save(&mut app, &mut storage);
    assert_eq!(storage.0, saved);

    app.screenshot = None;
    app.new_chat = true;
    // Pi can already have allocated a session path for an empty composer.
    app.state.session_file = Some("empty-unsent-chat.jsonl".into());
    eframe::App::save(&mut app, &mut storage);
    let restored = reopen(&storage, Options::default());
    assert!(!restored.projects);
    assert!(restored.new_chat);
    assert!(restored.pending_session.is_none());
}

#[test]
fn reopen_selects_the_saved_worker_in_its_original_project_and_tree() {
    let (cwd, path) = fixture();
    let parent_path = cwd.join("parent.jsonl");
    std::fs::write(&parent_path, "").unwrap();
    let mut storage = MemoryStorage::default();
    let prefs: Preferences = serde_json::from_value(json!({
        "cwd": cwd,
        "last_session": path,
        "projects_view": true,
        "agent_tree": [
            {"id":"agent-1","parent":null,"name":"Parent","cwd":cwd,
             "path":parent_path,"access":"workspace","worktree":null},
            {"id":"agent-2","parent":"agent-1","name":"Reader","cwd":cwd,
             "path":path,"access":"read-only","worktree":null}
        ]
    }))
    .unwrap();
    eframe::set_value(&mut storage, "preferences", &prefs);
    let restored = reopen(&storage, Options::default());
    assert_eq!(restored.agents.active, "agent-2");
    assert_eq!(
        restored.agents.nodes["agent-2"].parent.as_deref(),
        Some("agent-1")
    );
    assert_eq!(restored.agents.nodes["agent-2"].access, "read-only");
    assert_eq!(restored.state.session_file.as_deref(), path.to_str());
    assert!(restored.agents.nodes["agent-1"].stopped);
    assert!(restored.loading_chat);
    assert!(restored.client.is_some());
    assert!(!restored.new_chat);
    assert!(!restored.state.pending);
}

#[test]
fn saving_during_a_chat_switch_remembers_the_requested_chat_not_the_old_one() {
    let (cwd, path) = fixture();
    let mut storage = MemoryStorage::default();
    let mut app = reopen(
        &storage,
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.prefs.cwd = cwd;
    app.new_chat = true;
    app.state.session_file = Some("previous-chat.jsonl".into());
    app.pending_session = Some(path.clone());
    app.projects = true;
    eframe::App::save(&mut app, &mut storage);
    let restored = reopen(&storage, Options::default());
    assert_eq!(restored.pending_session.as_ref(), Some(&path));
    assert!(restored.projects);
}

#[test]
fn older_preferences_default_to_recent_and_a_blank_chat() {
    let prefs: Preferences = serde_json::from_str("{}").unwrap();
    assert!(!prefs.projects_view);
    assert!(prefs.last_session.is_none());
    assert!(prefs.startup_session(&Options::default()).is_none());
}
