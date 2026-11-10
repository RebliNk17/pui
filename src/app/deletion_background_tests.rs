use super::*;
use tests::{app, home, session};

fn saved(app: &mut Pui, home: &Path, name: &str) -> Session {
    let session = Session {
        path: session(home, name),
        cwd: home.into(),
        project: home.into(),
        title: name.into(),
        modified: std::time::SystemTime::now(),
    };
    app.sessions.push(session.clone());
    session
}

// Hold UI completion independently of filesystem/thread timing.
fn hold_completion(app: &mut Pui) -> mpsc::Sender<Result<(), String>> {
    let (target, rx) = app.deletion.jobs.remove(0);
    rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
    let (tx, rx) = mpsc::channel();
    app.deletion.jobs.push((target, rx));
    tx
}

#[test]
fn deleting_active_chat_closes_dialog_and_allows_navigation_and_sending() {
    let mut app = app();
    let home = home();
    let deleting = saved(&mut app, &home, "deleting");
    let other = saved(&mut app, &home, "other");
    app.state = State {
        session_file: Some(deleting.path.display().to_string()),
        ..Default::default()
    };
    app.client = Some(rpc::Client::stub());
    app.connected = true;
    let id = app.ensure_agent();
    app.request_delete(Target::Chat(deleting.path.clone()));
    app.begin_delete(Target::Chat(deleting.path.clone()))
        .unwrap();
    let done = hold_completion(&mut app);
    assert!(app.deletion.pending.is_none());
    assert!(!app.deletion.confirming());
    assert!(!app.deletion.in_progress());
    assert!(app.new_chat);
    assert!(app.state.session_file.is_none());
    let ctx = app.ctx.clone();
    let output = ctx.run(Default::default(), |ctx| app.deletion_dialog(ctx));
    assert!(output.shapes.is_empty(), "No waiting modal");

    app.view_agent(&id);
    app.open_chat(deleting);
    assert!(
        app.state.session_file.is_none(),
        "Cannot reopen deleting chat"
    );
    app.open_chat(other.clone());
    assert_eq!(app.state.session_file.as_deref(), other.path.to_str());
    app.loading_chat = false;
    app.connected = true;
    app.ultracode_ready = true;
    app.preview_rx = None;
    app.pending_session = None;
    app.draft = "keep working".into();
    app.submit(&ctx);
    assert!(app.draft.is_empty(), "Other chats can still send");
    app.draft = "preserve this draft".into();
    done.send(Ok(())).unwrap();
    app.poll_deletions();
    assert!(app.deletion.jobs.is_empty());
    assert_eq!(app.state.session_file.as_deref(), other.path.to_str());
    assert_eq!(app.draft, "preserve this draft");
    assert!(!app.agents.nodes.contains_key(&id));
    app.shutdown_agents();
}

#[test]
fn can_start_new_chat_and_delete_another_chat_while_deleting() {
    let mut app = app();
    let home = home();
    let first = saved(&mut app, &home, "first");
    let second = saved(&mut app, &home, "second");
    app.begin_delete(Target::Chat(first.path.clone())).unwrap();
    let done = hold_completion(&mut app);
    app.start_chat_in(home);
    assert!(app.new_chat);
    assert!(app.client.is_some());
    app.new_task();
    assert!(app.tasks.open);
    app.open_agents_board();
    assert!(app.kanban.open);
    app.request_delete(Target::Chat(second.path.clone()));
    assert_eq!(
        app.deletion.pending,
        Some(Target::Chat(second.path.clone()))
    );
    app.begin_delete(Target::Chat(second.path.clone())).unwrap();
    assert_eq!(app.deletion.jobs.len(), 2);
    assert!(app.begin_delete(Target::Chat(second.path)).is_err());
    done.send(Ok(())).unwrap();
    app.poll_deletions();
    if !app.deletion.jobs.is_empty() {
        let done = hold_completion(&mut app);
        done.send(Ok(())).unwrap();
        app.poll_deletions();
    }
    assert!(app.deletion.jobs.is_empty());
    assert!(!first.path.exists());
    app.shutdown_agents();
}

#[test]
fn failures_keep_chat_available_and_do_not_play_success_sound() {
    for disconnect in [false, true] {
        let mut app = app();
        let home = home();
        let chat = saved(&mut app, &home, "retry");
        let (tx, rx) = mpsc::channel();
        app.deletion
            .jobs
            .push((Target::Chat(chat.path.clone()), rx));
        if !disconnect {
            tx.send(Err("Cannot delete transcript".into())).unwrap();
        }
        drop(tx);
        app.poll_deletions();
        assert!(app.deletion.jobs.is_empty());
        assert!(
            app.state
                .error
                .as_ref()
                .unwrap()
                .contains("Deletion failed")
        );
        assert!(app.notifications.sent.is_empty());
        assert!(chat.path.exists());
        assert!(app.sessions.iter().any(|s| s.path == chat.path));
        app.open_chat(chat.clone());
        assert_eq!(app.state.session_file.as_deref(), chat.path.to_str());
        app.shutdown_agents();
    }
}

#[test]
fn deletion_sound_respects_preference_and_never_sends_agent_desktop_banner() {
    for sound in [false, true] {
        let mut app = app();
        let home = home();
        let chat = saved(&mut app, &home, "sound");
        app.prefs.notifications.sound = sound;
        app.prefs.notifications.desktop = true;
        app.begin_delete(Target::Chat(chat.path)).unwrap();
        let done = hold_completion(&mut app);
        done.send(Ok(())).unwrap();
        app.poll_deletions();
        app.poll_deletions();
        let expected = usize::from(sound && notifications::Notifications::supported());
        assert_eq!(app.notifications.sent.len(), expected);
        assert!(
            app.notifications
                .sent
                .iter()
                .all(|settings| !settings.desktop)
        );
    }
}

#[test]
fn sidebar_shows_red_deleting_spinner_for_root_and_nested_chats() {
    for nested in [false, true] {
        let mut app = app();
        let home = home();
        let chat = saved(&mut app, &home, "spinner");
        let (tx, rx) = mpsc::channel();
        app.deletion
            .jobs
            .push((Target::Chat(chat.path.clone()), rx));
        app.prefs.cwd = home.clone();
        app.projects = nested;
        if nested {
            let parent = saved(&mut app, &home, "parent");
            app.state.session_file = Some(parent.path.display().to_string());
            let parent = app.ensure_agent();
            app.park_tree();
            app.state.session_file = Some(chat.path.display().to_string());
            let child = app.ensure_agent();
            app.agents.nodes.get_mut(&child).unwrap().parent = Some(parent.clone());
            app.agents
                .nodes
                .get_mut(&parent)
                .unwrap()
                .children
                .push(child);
        }
        let ctx = app.ctx.clone();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| app.sidebar(ctx),
        );
        fn red_spinner(shape: &egui::Shape, red: Color32) -> bool {
            match shape {
                egui::Shape::Path(path) => {
                    path.points.len() >= 8
                        && path.stroke.color == egui::epaint::ColorMode::Solid(red)
                }
                egui::Shape::Vec(shapes) => shapes.iter().any(|s| red_spinner(s, red)),
                _ => false,
            }
        }
        assert!(
            output
                .shapes
                .iter()
                .any(|s| red_spinner(&s.shape, theme::palette(&ctx).red))
        );
        drop(tx);
    }
}
