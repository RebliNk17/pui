use super::*;
use crate::app::files::tests::{app, click, frame, text};

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn run_command_opens_terminal_and_echoes_in_demo_without_executing() {
    let ctx = egui::Context::default();
    let mut terminals = Terminals::default();
    let root = PathBuf::from("/tmp/pui-run-command");
    terminals.run_command(&ctx, &root, true, "git status\ngit log -1");
    assert!(terminals.is_open());
    let project = terminals.projects.get(&root).unwrap();
    assert_eq!(project.tabs.len(), 1);
    let screen = project
        .active()
        .unwrap()
        .demo
        .as_ref()
        .unwrap()
        .screen()
        .contents();
    assert!(screen.contains("git status"), "{screen}");
    assert!(screen.contains("git log -1"), "{screen}");
    assert!(screen.contains("not run"), "{screen}");
    // A second run reuses the same shell instead of opening another tab.
    terminals.run_command(&ctx, &root, true, "pwd");
    assert_eq!(terminals.projects.get(&root).unwrap().tabs.len(), 1);
}

#[test]
fn queued_run_input_waits_for_the_shell_to_start() {
    let (_tx, rx) = mpsc::channel();
    let mut tab = Tab {
        pending: b"echo hi\r".to_vec(),
        starting: Some(rx),
        ..Default::default()
    };
    tabs::poll_tab(&mut tab);
    assert_eq!(tab.pending, b"echo hi\r");
}

#[test]
fn stable_tab_ids_and_focus_tab_by_id_open_and_select_exact_tab() {
    let ctx = egui::Context::default();
    let mut terminals = Terminals::default();
    let root = PathBuf::from("/tmp/pui-tab-deep-links");
    terminals.toggle(&ctx, &root, true);
    terminals.start(&ctx, true);
    terminals.start(&ctx, true);

    let tabs = &terminals.projects[&root].tabs;
    assert_eq!(tabs.len(), 3);
    assert_eq!(tabs[0].tab_id, "term-1");
    assert_eq!(tabs[1].tab_id, "term-2");
    assert_eq!(tabs[2].tab_id, "term-3");
    assert_eq!(terminals.projects[&root].selected, 2);

    // Closing the middle tab preserves existing IDs and advances monotonically.
    let closed_internal_id = tabs[1].id;
    terminals.close_tab(&ctx, &root, closed_internal_id);
    terminals.start(&ctx, true);
    let tabs = &terminals.projects[&root].tabs;
    assert_eq!(
        tabs.iter().map(|t| t.tab_id.as_str()).collect::<Vec<_>>(),
        vec!["term-1", "term-3", "term-4"]
    );

    // Closing the panel and focusing term-1 reopens and selects index 0.
    terminals.close(&ctx);
    assert!(!terminals.is_open());
    assert!(terminals.focus_tab_by_id(&ctx, "term-1"));
    assert!(terminals.is_open());
    assert_eq!(terminals.projects[&root].selected, 0);
    assert_eq!(terminals.projects[&root].active().unwrap().tab_id, "term-1");

    // Unknown or closed tab IDs return false without changing selection or opening.
    terminals.close(&ctx);
    assert!(!terminals.focus_tab_by_id(&ctx, "term-2"));
    assert!(!terminals.is_open());
    assert!(!terminals.focus_tab_by_id(&ctx, "term-999"));
    assert!(!terminals.focus_tab_by_id(&ctx, ""));
}

#[test]
fn terminal_deep_link_in_process_open_links_focuses_tab_or_sets_notice() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let root = app.prefs.cwd.clone();
    app.terminals.toggle(&ctx, &root, true);
    app.terminals.start(&ctx, true);
    let first_id = app.terminals.projects[&root].tabs[0].tab_id.clone();
    let second_id = app.terminals.projects[&root].tabs[1].tab_id.clone();
    assert_eq!(first_id, "term-1");
    assert_eq!(second_id, "term-2");

    app.terminals.close(&ctx);
    assert!(!app.terminals.is_open());

    let _ = ctx.run(Default::default(), |ctx| {
        ctx.open_url(egui::OpenUrl::new_tab(format!("#terminal/{first_id}")));
        app.process_open_links(ctx);
    });
    assert!(app.terminals.is_open());
    assert_eq!(
        app.terminals.projects[&root].active().unwrap().tab_id,
        first_id
    );
    assert!(app.state.notice.is_none());

    let _ = ctx.run(Default::default(), |ctx| {
        ctx.open_url(egui::OpenUrl::new_tab("#terminal/term-999"));
        app.process_open_links(ctx);
    });
    assert_eq!(
        app.state.notice.as_deref(),
        Some("Terminal tab 'term-999' is no longer open.")
    );
}

#[test]
fn terminal_keys_and_safe_bracketed_paste() {
    for (keycode, bytes) in [
        (egui::Key::Enter, b"\r".as_slice()),
        (egui::Key::Tab, b"\t"),
        (egui::Key::Escape, b"\x1b"),
        (egui::Key::Backspace, b"\x7f"),
        (egui::Key::ArrowUp, b"\x1b[A"),
    ] {
        assert_eq!(
            encode(&key(keycode, egui::Modifiers::NONE), false, false).unwrap(),
            bytes
        );
    }
    assert_eq!(
        encode(&key(egui::Key::C, egui::Modifiers::CTRL), false, false).unwrap(),
        [3]
    );
    assert_eq!(
        encode(&key(egui::Key::ArrowUp, egui::Modifiers::NONE), true, false).unwrap(),
        b"\x1bOA"
    );
    assert_eq!(
        encode(
            &key(egui::Key::ArrowLeft, egui::Modifiers::CTRL),
            false,
            false
        )
        .unwrap(),
        b"\x1b[1;5D"
    );
    assert_eq!(
        encode(&egui::Event::Text("世界".into()), false, false).unwrap(),
        "世界".as_bytes()
    );
    assert_eq!(
        encode(
            &egui::Event::Paste("a\r\nb\x1b[201~\x03".into()),
            false,
            true
        )
        .unwrap(),
        b"\x1b[200~a\nb[201~\x1b[201~"
    );
}

#[test]
fn toolbar_terminal_docks_without_changing_chat_and_preserves_projects() {
    for theme in [theme::Theme::Dark, theme::Theme::Light] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.prefs.theme = theme;
        app.state.busy = true;
        app.draft = "Keep my draft".into();
        let messages = app.state.messages.len();
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        let terminal_button = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("terminal-toolbar-button")))
            .unwrap()
            .center();
        click(&ctx, &mut app, terminal_button);
        assert!(app.terminals.is_open());
        assert_eq!(app.terminals.projects.len(), 1);
        assert!(app.terminals.focused(&ctx));
        let output = frame(&ctx, &mut app, vec![]);
        assert!(
            output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text)
                if text.galley.job.text == "Terminal" && text.pos.y > 400.0)
            }),
            "The terminal panel should remain docked below chat, independently of its breadcrumb"
        );
        let root = app.prefs.cwd.clone();
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(app.state.busy, "Terminal Escape must not stop Pi");
        assert_eq!(app.draft, "Keep my draft");
        assert_eq!(app.state.messages.len(), messages);
        app.terminals
            .projects
            .get_mut(&root)
            .unwrap()
            .active_mut()
            .unwrap()
            .demo
            .as_mut()
            .unwrap()
            .process(b"MARKER");
        click(&ctx, &mut app, terminal_button);
        assert!(!app.terminals.is_open());
        assert_eq!(
            app.terminals.projects.len(),
            1,
            "Hiding must not discard sessions"
        );
        click(&ctx, &mut app, terminal_button);
        assert!(
            app.terminals.projects[&root]
                .active()
                .unwrap()
                .demo
                .as_ref()
                .unwrap()
                .screen()
                .contents()
                .contains("MARKER")
        );
        let other = root.join("src");
        app.prefs.cwd = other.clone();
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.terminals.root, other);
        assert_eq!(app.terminals.projects.len(), 2);
        assert!(
            app.terminals.projects[&other]
                .active()
                .unwrap()
                .session
                .is_none(),
            "Demos never spawn real shells"
        );
        app.prefs.cwd = root.clone();
        frame(&ctx, &mut app, vec![]);
        assert!(
            app.terminals.projects[&root]
                .active()
                .unwrap()
                .demo
                .as_ref()
                .unwrap()
                .screen()
                .contents()
                .contains("MARKER")
        );
        app.terminals.expanded = true;
        let output = frame(&ctx, &mut app, vec![]);
        assert!(app.terminals.is_open());
        assert!(text(&output, "Terminal").y < 100.0);
        assert!(!output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text.contains("Design preview · example conversation"))));
    }
}
