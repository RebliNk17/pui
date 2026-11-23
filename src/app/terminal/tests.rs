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
