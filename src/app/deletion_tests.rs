use super::*;
fn app() -> Pui {
    let mut app = Pui::new(
        &eframe::CreationContext::_new_kittest(egui::Context::default()),
        Options {
            demo: true,
            ..Default::default()
        },
    );
    app.demo = false;
    app.sessions.clear();
    app
}
fn session(home: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(home).unwrap();
    let path = home.join(format!("{name}.jsonl"));
    std::fs::write(&path, "{\"type\":\"session\",\"cwd\":\"/fixture\"}\n").unwrap();
    path
}
fn home() -> PathBuf {
    std::env::temp_dir().join(format!("pui-delete-test-{}", uuid::Uuid::new_v4()))
}
fn complete(app: &mut Pui) {
    let (target, rx) = app.deletion.job.take().unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
    app.finish_delete(&target);
}
#[test]
fn chat_delete_removes_transcript_and_preferences_but_keeps_project_and_tasks() {
    let mut app = app();
    let home = home();
    let path = session(&home, "chat");
    let other = session(&home, "other");
    app.prefs.cwd = home.clone();
    app.prefs.last_session = Some(path.clone());
    app.prefs.chats.insert(path.clone(), Default::default());
    app.state.session_file = Some(path.display().to_string());
    app.state.busy = false;
    let id = app.ensure_agent();
    app.new_task();
    let task = app.tasks.selected.clone().unwrap();
    app.tasks.open = false;
    app.pending_session = None;
    app.loading_chat = false;
    app.begin_delete(Target::Chat(path.clone())).unwrap();
    complete(&mut app);
    assert!(!path.exists());
    assert!(other.exists());
    assert!(home.exists());
    assert!(app.prefs.last_session.is_none());
    assert!(!app.prefs.chats.contains_key(&path));
    assert!(!app.agents.nodes.contains_key(&id));
    assert!(app.state.session_file.is_none());
    assert!(app.new_chat);
    assert!(
        app.tasks
            .records
            .iter()
            .find(|t| t.id == task)
            .unwrap()
            .parent_session
            .is_none()
    );
}
#[test]
fn chat_overflow_has_permanent_delete_and_requires_a_second_explicit_click() {
    let mut app = app();
    let home = home();
    let path = session(&home, "menu");
    app.prefs.cwd = home.clone();
    app.sessions.push(Session {
        path: path.clone(),
        cwd: home.clone(),
        project: home,
        title: "Delete fixture".into(),
        created: None,
        modified: std::time::SystemTime::now(),
    });
    let ctx = app.ctx.clone();
    fn locate(shape: &egui::Shape, label: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(t) if t.galley.job.text == label => {
                Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| locate(s, label)),
            _ => None,
        }
    }
    let render = |app: &mut Pui, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| app.sidebar(ctx),
        )
    };
    for _ in 0..3 {
        render(&mut app, vec![]);
    }
    let output = render(&mut app, vec![]);
    let row = output
        .shapes
        .iter()
        .find_map(|s| locate(&s.shape, "Delete fixture"))
        .unwrap();
    let click = |app: &mut Pui, pos| {
        for pressed in [true, false] {
            render(
                app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        pressed,
                        button: egui::PointerButton::Primary,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    };
    click(&mut app, egui::pos2(230.0, row.center().y + 3.0));
    for _ in 0..3 {
        render(&mut app, vec![]);
    }
    let output = render(&mut app, vec![]);
    let delete = output
        .shapes
        .iter()
        .find_map(|s| locate(&s.shape, "Delete chat…"))
        .expect("Permanent delete menu entry");
    click(&mut app, delete.center());
    assert_eq!(app.deletion.pending, Some(Target::Chat(path.clone())));
    assert!(app.deletion.job.is_none());
    assert!(path.exists());
}
#[test]
fn busy_chats_and_running_tasks_cannot_be_deleted() {
    let mut app = app();
    let home = home();
    let path = session(&home, "busy");
    app.state.session_file = Some(path.display().to_string());
    app.state.busy = true;
    assert!(app.begin_delete(Target::Chat(path.clone())).is_err());
    assert!(path.exists());
    app.new_task();
    let id = app.tasks.selected.clone().unwrap();
    app.tasks
        .records
        .iter_mut()
        .find(|t| t.id == id)
        .unwrap()
        .phase = crate::tasks::Phase::Building;
    assert!(app.begin_delete(Target::Task(id)).is_err());
    assert!(path.exists());
}
#[test]
fn task_sidebar_overflow_opens_confirmation_for_the_exact_task_without_deleting() {
    let mut app = app();
    app.prefs.cwd = home();
    app.new_task();
    let id = app.tasks.selected.clone().unwrap();
    app.tasks.open = false;
    let ctx = app.ctx.clone();
    let render = |app: &mut Pui, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| app.sidebar(ctx),
        )
    };
    fn text(shape: &egui::Shape, needle: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(t) if t.galley.job.text.starts_with(needle) => {
                Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| text(s, needle)),
            _ => None,
        }
    }
    for _ in 0..3 {
        render(&mut app, vec![]);
    }
    let output = render(&mut app, vec![]);
    let row = output
        .shapes
        .iter()
        .find_map(|s| text(&s.shape, &format!("Task {}", &id[..8])))
        .expect("Sidebar task");
    let click = |app: &mut Pui, pos| {
        for pressed in [true, false] {
            render(
                app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        pressed,
                        button: egui::PointerButton::Primary,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    };
    click(&mut app, egui::pos2(230.0, row.center().y));
    for _ in 0..3 {
        render(&mut app, vec![]);
    }
    let output = render(&mut app, vec![]);
    let delete = output
        .shapes
        .iter()
        .find_map(|s| text(&s.shape, "Delete task…"))
        .expect("Delete task menu");
    click(&mut app, delete.center());
    assert_eq!(app.deletion.pending, Some(Target::Task(id.clone())));
    assert_eq!(app.tasks.records[0].id, id);
    assert!(!app.tasks.open);
}
#[cfg(unix)]
#[test]
fn symlinks_and_non_session_files_are_never_deleted() {
    let home = home();
    let path = session(&home, "real");
    let link = home.join("link.jsonl");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(sessions::delete(&link).is_err());
    assert!(path.exists());
    assert!(link.exists());
    let other = home.join("not-session.jsonl");
    std::fs::write(&other, "{\"type\":\"message\"}").unwrap();
    assert!(sessions::delete(&other).is_err());
    assert!(other.exists());
}
#[test]
fn deletion_requires_confirmation_and_escape_cancels_without_stopping_work() {
    let mut app = app();
    let home = home();
    let path = session(&home, "cancel");
    app.state.busy = true;
    app.request_delete(Target::Chat(path.clone()));
    assert!(path.exists());
    let ctx = app.ctx.clone();
    for _ in 0..3 {
        let _ = ctx.run(Default::default(), |ctx| app.deletion_dialog(ctx));
    }
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| app.deletion_dialog(ctx),
    );
    assert!(app.deletion.pending.is_none());
    assert!(app.deletion.job.is_none());
    assert!(app.state.busy);
    assert!(path.exists());
}
