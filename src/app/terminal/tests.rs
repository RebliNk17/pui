use super::*;
use crate::app::files::tests::{app, click, folder, frame, text};

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
        let output = frame(&ctx, &mut app, vec![]);
        let terminal_button = folder(&output) + egui::vec2(36.0, 0.0);
        click(&ctx, &mut app, terminal_button);
        assert!(app.terminals.is_open());
        assert_eq!(app.terminals.projects.len(), 1);
        assert!(app.terminals.focused(&ctx));
        let output = frame(&ctx, &mut app, vec![]);
        assert!(text(&output, "Terminal").y > 400.0);
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
            app.terminals.projects[&other].session.is_none(),
            "Demos never spawn real shells"
        );
        app.prefs.cwd = root.clone();
        frame(&ctx, &mut app, vec![]);
        assert!(
            app.terminals.projects[&root]
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
