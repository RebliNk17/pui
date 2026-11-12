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
        app.prefs.session_order = sidebar_order::Options {
            sort: sidebar_order::Sort::Name,
            reverse: true,
        };
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
        assert!(restored.prefs.session_order.sort == sidebar_order::Sort::Name);
        assert!(restored.prefs.session_order.reverse);
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
fn both_color_styles_survive_save_and_reopen_in_the_selected_mode() {
    let mut storage = MemoryStorage::default();
    let mut app = reopen(
        &storage,
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.new_chat = true;
    app.state = State::default();
    app.prefs.color_styles = theme::Styles {
        dark: theme::DarkStyle::Plum,
        light: theme::LightStyle::Sage,
    };
    app.prefs.theme = theme::Theme::Light;
    eframe::App::save(&mut app, &mut storage);
    let restored = reopen(&storage, Options::default());
    assert_eq!(restored.prefs.color_styles, app.prefs.color_styles);
    assert_eq!(restored.prefs.theme, theme::Theme::Light);
    assert_eq!(
        theme::palette(&restored.ctx).bg,
        theme::LightStyle::Sage.palette().bg
    );
    assert_eq!(
        restored.ctx.style().visuals.hyperlink_color,
        theme::LightStyle::Sage.palette().accent
    );
    let overridden = reopen(
        &storage,
        Options {
            theme: Some(theme::Theme::Dark),
            ..Default::default()
        },
    );
    assert_eq!(
        theme::palette(&overridden.ctx).bg,
        theme::DarkStyle::Plum.palette().bg
    );
    assert_eq!(overridden.prefs.color_styles.light, theme::LightStyle::Sage);
}

#[test]
fn restart_keeps_draft_and_question_hint_without_restoring_a_live_wait() {
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
    app.new_chat = false;
    app.prefs.cwd = cwd.clone();
    app.state = State {
        session_file: Some(path.display().to_string()),
        busy: true,
        ..Default::default()
    };
    app.state.dialogs.push_back(questions::demo_dialog());
    app.draft = "Keep my unsent answer".into();
    eframe::App::save(&mut app, &mut storage);
    let mut restored = reopen(&storage, Options::default());
    assert_eq!(restored.draft, "Keep my unsent answer");
    assert!(
        restored
            .state
            .restart_notice
            .contains("waiting for your answer")
    );
    assert!(restored.state.dialogs.is_empty());
    assert!(!restored.state.busy);
    assert!(!restored.state.pending);
    assert!(restored.waiting.is_empty());
    assert!(restored.deliveries.is_empty());
    // The checkpoint survives another save while other saved chats are unopened.
    eframe::App::save(&mut restored, &mut storage);
    let restored = reopen(&storage, Options::default());
    assert_eq!(restored.draft, "Keep my unsent answer");
    assert!(!restored.state.restart_notice.is_empty());
}

#[test]
fn startup_blank_draft_is_not_reused_by_new_chat_or_explicit_workspace() {
    let (cwd, _) = fixture();
    let mut storage = MemoryStorage::default();
    let mut app = reopen(
        &storage,
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.new_chat = true;
    app.prefs.cwd = cwd.clone();
    app.state = State {
        session_file: Some("allocated-but-unsent.jsonl".into()),
        ..Default::default()
    };
    app.draft = "Unsent new conversation".into();
    eframe::App::save(&mut app, &mut storage);
    let mut restored = reopen(&storage, Options::default());
    assert_eq!(restored.draft, "Unsent new conversation");
    restored.new_session();
    assert!(restored.draft.is_empty());
    let explicit = reopen(
        &storage,
        Options {
            cwd: Some(cwd),
            ..Default::default()
        },
    );
    assert!(explicit.draft.is_empty());
}

#[test]
fn quit_checkpoint_is_captured_before_stop_clears_the_run() {
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
    app.new_chat = false;
    app.prefs.cwd = cwd;
    app.state = State {
        session_file: Some(path.display().to_string()),
        busy: true,
        ..Default::default()
    };
    app.draft = "Still here".into();
    app.checkpoint_chats();
    app.agents.exiting = true;
    app.state.busy = false;
    app.state.dialogs.clear();
    eframe::App::save(&mut app, &mut storage);
    let restored = reopen(&storage, Options::default());
    assert_eq!(restored.draft, "Still here");
    assert!(restored.state.restart_notice.contains("interrupted"));
}

#[test]
fn navigating_to_non_tree_chat_restores_checkpoint_after_clearing_outgoing_draft() {
    let (cwd, path) = fixture();
    let other = cwd.join("other.jsonl");
    std::fs::write(&other, "").unwrap();
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
    app.prefs.cwd = cwd.clone();
    app.state = State {
        session_file: Some(path.display().to_string()),
        busy: true,
        ..Default::default()
    };
    app.draft = "Recovered on navigation".into();
    eframe::App::save(&mut app, &mut storage);
    for reuse in [false, true] {
        let mut restored = reopen(
            &storage,
            Options {
                session: Some(other.clone()),
                ..Default::default()
            },
        );
        restored.new_chat = false;
        restored.connected = reuse;
        restored.loading_chat = false;
        restored.open_chat(Session {
            path: path.clone(),
            cwd: cwd.clone(),
            project: cwd.clone(),
            title: "Recovered".into(),
            created: None,
            modified: std::time::SystemTime::now(),
        });
        assert_eq!(restored.draft, "Recovered on navigation");
        assert!(!restored.state.restart_notice.is_empty());
    }
}

#[test]
fn deleting_recovery_data_removes_both_persisted_and_startup_copies() {
    let (cwd, path) = fixture();
    let storage = MemoryStorage::default();
    let mut app = reopen(
        &storage,
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.new_chat = false;
    app.prefs.cwd = cwd;
    app.state = State {
        session_file: Some(path.display().to_string()),
        ..Default::default()
    };
    app.draft = "Delete this too".into();
    app.checkpoint_chats();
    app.restart_pending = app.prefs.restart_checkpoints.clone();
    app.forget_restart_checkpoint(&path);
    assert!(app.prefs.restart_checkpoints.is_empty());
    assert!(app.restart_pending.is_empty());
}

#[test]
fn same_session_reconnect_keeps_recovery_notice_until_explicit_dismissal() {
    let (cwd, path) = fixture();
    let storage = MemoryStorage::default();
    let mut app = reopen(
        &storage,
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.prefs.cwd = cwd;
    app.state = State {
        session_file: Some(path.display().to_string()),
        restart_notice: "Review interrupted work".into(),
        ..Default::default()
    };
    app.connect(&app.ctx.clone(), Some(path));
    assert_eq!(app.state.restart_notice, "Review interrupted work");
    app.connect(&app.ctx.clone(), None);
    assert!(app.state.restart_notice.is_empty());
}

#[test]
fn custom_sections_restore_stable_membership_order_and_collapse_without_execution() {
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
    app.new_chat = false;
    app.prefs.cwd = cwd.clone();
    app.state = State {
        session_file: Some(path.display().to_string()),
        ..Default::default()
    };
    let mut section = sidebar::sections::Section::new("Focused review".into());
    section.collapsed = true;
    section.expanded = true;
    let id = section.id.clone();
    app.prefs.sidebar_view.sections.push(section);
    app.prefs.chats.entry(path.clone()).or_default().section = Some(id.clone());
    app.new_sidebar_section(); // Autosave must not persist a half-created section.
    assert_eq!(app.prefs.sidebar_view.sections.len(), 2);
    eframe::App::save(&mut app, &mut storage);
    assert_eq!(app.prefs.sidebar_view.sections.len(), 2);
    let restored = reopen(&storage, Options::default());
    assert_eq!(restored.prefs.sidebar_view.sections.len(), 1);
    let section = &restored.prefs.sidebar_view.sections[0];
    assert_eq!(section.id, id);
    assert_eq!(section.name, "Focused review");
    assert!(section.collapsed && section.expanded);
    assert_eq!(
        restored.prefs.chats[&path].section.as_deref(),
        Some(id.as_str())
    );
    assert_eq!(restored.pending_session.as_ref(), Some(&path));
    assert!(!restored.state.pending);
    assert!(restored.state.dialogs.is_empty());
}

#[test]
fn table_header_overrides_save_reopen_without_loading_any_files_or_replaying_work() {
    let (cwd, chat) = fixture();
    let mut storage = MemoryStorage::default();
    let mut app = reopen(
        &storage,
        Options {
            demo: true,
            cwd: Some(cwd.clone()),
            ..Default::default()
        },
    );
    let path = cwd.join("nonexistent-headerless.csv");
    assert!(app.prefs.table_headers.set(&cwd, &path, false));
    app.demo = false;
    app.state = State {
        session_file: Some(chat.display().to_string()),
        ..Default::default()
    };
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    eframe::App::save(&mut app, &mut storage);
    assert!(commands.try_recv().is_err());
    let mut restored = reopen(
        &storage,
        Options {
            demo: true,
            cwd: Some(cwd.clone()),
            ..Default::default()
        },
    );
    assert!(!restored.prefs.table_headers.header(&cwd, &path));
    assert!(
        restored
            .prefs
            .table_headers
            .header(&cwd.join("other"), &path)
    );
    assert!(!restored.files.is_open());
    assert!(restored.files.navigation_target().is_none());
    assert!(restored.state.dialogs.is_empty() && restored.draft.is_empty());
    restored.prefs.table_headers.set(&cwd, &path, true);
    restored.demo = false;
    eframe::App::save(&mut restored, &mut storage);
    let cleared = reopen(
        &storage,
        Options {
            demo: true,
            cwd: Some(cwd.clone()),
            ..Default::default()
        },
    );
    assert!(cleared.prefs.table_headers.header(&cwd, &path));
}

#[test]
fn older_preferences_default_to_recent_and_a_blank_chat() {
    let prefs: Preferences = serde_json::from_str("{}").unwrap();
    assert!(!prefs.projects_view);
    assert!(prefs.last_session.is_none());
    assert!(prefs.startup_session(&Options::default()).is_none());
}
