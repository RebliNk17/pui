use super::*;
use crate::app::files::tests::{app, click, frame};

#[test]
fn tab_switch_clears_frozen_capture_and_quote_names_the_exact_shell() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let root = app.prefs.cwd.clone();
    app.terminals.toggle(&ctx, &root, true);
    app.terminals.start(&ctx, true);
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    let open = |app: &mut Pui| {
        let rect = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("terminal-context-button")))
            .unwrap();
        click(&ctx, app, rect.center());
    };
    open(&mut app);
    assert!(egui::Popup::is_any_open(&ctx));
    app.terminals.select_tab(&ctx, 0);
    assert!(!egui::Popup::is_any_open(&ctx));
    for _ in 0..2 {
        frame(&ctx, &mut app, vec![]);
    }
    open(&mut app);
    let add = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("terminal-context-add")))
        .unwrap();
    click(&ctx, &mut app, add.center());
    assert!(
        app.outgoing_text().contains(" · tab term-1 · shell 1]"),
        "{}",
        app.outgoing_text()
    );
    app.terminals.select_tab(&ctx, 1);
    frame(&ctx, &mut app, vec![]);
    open(&mut app);
    let add = ctx
        .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("terminal-context-add")))
        .unwrap();
    click(&ctx, &mut app, add.center());
    assert!(app.outgoing_text().contains(" · tab term-2 · shell 2]"));
}

#[cfg(unix)]
#[test]
fn actual_two_ptys_are_independent_hidden_shell_survives_and_only_closed_shell_exits() {
    let root = std::env::temp_dir().join(format!("pui-tab-pty-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let ctx = egui::Context::default();
    let mut terminal = Terminals::default();
    terminal.toggle(&ctx, &root, true);
    terminal.start(&ctx, true);
    let mut states = Vec::new();
    for (index, tab) in terminal
        .projects
        .get_mut(&root)
        .unwrap()
        .tabs
        .iter_mut()
        .enumerate()
    {
        let session = Session::fixture(&ctx, &root).unwrap();
        session
            .send(format!("stty -echo; printf 'TAB_{}\\n'\n", index + 1).into_bytes())
            .unwrap();
        states.push(session.state.clone());
        tab.demo = None;
        tab.session = Some(session);
    }
    let wait = |predicate: &dyn Fn() -> bool| {
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while !predicate() {
            assert!(std::time::Instant::now() < deadline, "PTY fixture timeout");
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    wait(&|| {
        states[0]
            .lock()
            .unwrap()
            .parser
            .screen()
            .contents()
            .contains("TAB_1")
            && states[1]
                .lock()
                .unwrap()
                .parser
                .screen()
                .contents()
                .contains("TAB_2")
    });
    terminal.close(&ctx);
    assert!(terminal.running());
    terminal.toggle(&ctx, &root, true);
    let id = terminal.projects[&root].tabs[0].id;
    terminal.close_tab(&ctx, &root, id);
    wait(&|| states[0].lock().unwrap().exited.is_some());
    assert!(states[1].lock().unwrap().exited.is_none());
    terminal.projects[&root]
        .active()
        .unwrap()
        .session
        .as_ref()
        .unwrap()
        .send(b"printf 'SURVIVED\\n'\n".to_vec())
        .unwrap();
    wait(&|| {
        states[1]
            .lock()
            .unwrap()
            .parser
            .screen()
            .contents()
            .contains("SURVIVED")
    });
    terminal.shutdown();
    wait(&|| states[1].lock().unwrap().exited.is_some());
}

#[test]
fn bounded_tab_fixture_has_no_background_poll_or_per_frame_new_shells() {
    let ctx = egui::Context::default();
    let mut terminal = Terminals::default();
    let started = std::time::Instant::now();
    for n in 0..4 {
        let root = PathBuf::from(format!("/bounded-{n}"));
        terminal.workspace(&ctx, &root, true);
        for _ in 1..MAX_TABS {
            terminal.start(&ctx, true);
        }
    }
    assert_eq!(terminal.total_tabs(), MAX_TOTAL_TABS);
    let ids = terminal
        .projects
        .values()
        .flat_map(|group| group.tabs.iter().map(|tab| tab.id))
        .collect::<Vec<_>>();
    for _ in 0..10000 {
        terminal.context_inputs(&ctx, false);
        assert_eq!(terminal.total_tabs(), MAX_TOTAL_TABS);
    }
    let after = terminal
        .projects
        .values()
        .flat_map(|group| group.tabs.iter().map(|tab| tab.id))
        .collect::<Vec<_>>();
    assert_eq!(ids, after);
    eprintln!(
        "32 demo shell tabs+10k hidden in-memory completion scans {:?}; Tab {}B shallow, project {}B; excludes VT screens/scrollback/native PTY/worker allocation",
        started.elapsed(),
        std::mem::size_of::<Tab>(),
        std::mem::size_of::<Project>()
    );
}

#[test]
fn pending_hidden_tab_completion_is_polled_without_stealing_active_focus() {
    let ctx = egui::Context::default();
    let mut terminal = Terminals::default();
    let root = Path::new("/demo");
    terminal.toggle(&ctx, root, true);
    let (tx, rx) = mpsc::channel();
    terminal
        .projects
        .get_mut(root)
        .unwrap()
        .active_mut()
        .unwrap()
        .starting = Some(rx);
    terminal.start(&ctx, true);
    terminal.focus = false;
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("composer")));
    tx.send(Err("fixture spawn error".into())).unwrap();
    terminal.context_inputs(&ctx, false);
    assert_eq!(
        terminal.projects[root].tabs[0].error.as_deref(),
        Some("fixture spawn error")
    );
    assert!(!terminal.focus);
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new("composer"))));
}
