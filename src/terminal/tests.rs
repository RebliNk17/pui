use super::*;
use std::time::{Duration, Instant};

fn wait_for(session: &Session, predicate: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if predicate(&session.state.lock().unwrap()) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Timed out waiting for terminal output"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn ansi_unicode_scrollback_and_device_replies_are_bounded() {
    let (tx, rx) = mpsc::sync_channel(64);
    let mut state = State::new(
        portable_pty::PtySize {
            rows: 4,
            cols: 30,
            ..Default::default()
        },
        tx,
    );
    state
        .parser
        .process("\x1b[31mHello 世界\x1b[0m\r\n".as_bytes());
    assert!(state.parser.screen().contents().contains("Hello 世界"));
    assert_eq!(
        state.parser.screen().cell(0, 0).unwrap().fgcolor(),
        vt100::Color::Idx(1)
    );
    state.parser.process(b"\x1b[6n");
    let Input::Write(reply) = rx.try_recv().unwrap() else {
        panic!("Expected device reply")
    };
    assert_eq!(reply, b"\x1b[2;1R");
    for _ in 0..3_000 {
        state.parser.process(b"line\r\n");
    }
    assert_eq!(state.parser.screen().scrollback_len(), SCROLLBACK);
    state.parser.process(b"\x1b[?1049h\x1b[2Jfull screen");
    assert!(state.parser.screen().alternate_screen());
    assert!(state.parser.screen().contents().contains("full screen"));
    state.parser.process(b"\x1b[?1049l");
    assert!(!state.parser.screen().alternate_screen());
}

#[test]
fn invalid_workspace_does_not_silently_start_in_home() {
    let root = std::env::temp_dir().join(format!("pui-terminal-missing-{}", uuid::Uuid::new_v4()));
    assert!(Session::start(&egui::Context::default(), &root, PtySize::default()).is_err());
}

#[cfg(unix)]
#[test]
fn real_pty_uses_project_cwd_accepts_input_resizes_and_reports_exit() {
    let root = std::env::temp_dir().join(format!("pui terminal ' 世界 {}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let mut command = CommandBuilder::new("/bin/sh");
    command.cwd(&root);
    command.env("PS1", "PTY> ");
    let mut session =
        Session::spawn(&egui::Context::default(), command, PtySize::default()).unwrap();
    session
        .send(b"printf '\\033[32mREADY\\033[0m\\n'; pwd\r".to_vec())
        .unwrap();
    wait_for(&session, |s| {
        s.parser
            .screen()
            .contents()
            .contains(&root.to_string_lossy().to_string())
    });
    wait_for(&session, |s| s.parser.screen().contents().contains("READY"));
    session.resize(30, 100);
    session.send(b"stty size\r".to_vec()).unwrap();
    wait_for(&session, |s| {
        s.parser.screen().contents().contains("30 100")
    });
    assert!(session.send(vec![b'x'; MAX_INPUT + 1]).is_err());
    session.send(b"exit 7\r".to_vec()).unwrap();
    wait_for(&session, |s| {
        s.exited.as_ref().is_some_and(|s| s.contains('7'))
    });
    assert!(!session.running());
    assert!(session.send(b"pwd\r".to_vec()).is_err());
    drop(session);
}

#[cfg(unix)]
#[test]
fn dropping_session_terminates_and_reaps_its_shell() {
    let mut command = CommandBuilder::new("/bin/sh");
    command.arg("-i");
    let session = Session::spawn(&egui::Context::default(), command, PtySize::default()).unwrap();
    session.send(b"printf 'SHELL_READY\\n'\r".to_vec()).unwrap();
    wait_for(&session, |s| {
        s.parser.screen().contents().contains("SHELL_READY")
    });
    let finished = session.finished.clone();
    drop(session);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !finished.load(Ordering::Acquire) {
        assert!(
            Instant::now() < deadline,
            "Shell was not reaped after terminal drop"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
