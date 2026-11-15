use super::*;
use crate::app::files::tests::{app, settle};
use std::time::Instant;

fn script(pid: &Path, release: &Path) -> String {
    format!(
        "import os,pathlib,sys,struct,time\np=pathlib.Path({pid:?});gate=pathlib.Path({release:?});h=sys.stdin.buffer.read(8);sys.stdin.buffer.read(struct.unpack('<I',h[4:])[0]);p.write_text(str(os.getpid()))\nwhile not gate.exists(): time.sleep(.005)\nsys.stdout.buffer.write(b'PSV1'+struct.pack('<II',2,1));sys.stdout.flush();ack=sys.stdin.buffer.read(1);sys.stdin.buffer.read();sys.stdout.buffer.write(b'\\x80\\0\\0\\x80\\0\\0\\0\\0');sys.stdout.flush()",
        pid = pid.to_string_lossy(),
        release = release.to_string_lossy()
    )
}
fn await_pid(path: &Path) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(value) = fs::read_to_string(path)
            && let Ok(pid) = value.parse()
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "helper did not enter protocol");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn assert_reaped(pid: i32) {
    // SAFETY: signal zero only checks existence, never delivers a signal.
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        -1,
        "owned helper survived loader cleanup"
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}
fn open(app: &mut Pui, ctx: &egui::Context, path: &Path, script: String) {
    crate::svg_preview::test_support::with(Some(script), || {
        app.files
            .open_target(ctx, &app.prefs.cwd.clone(), path.to_str().unwrap())
            .unwrap()
    });
}
#[test]
fn svg_loader_close_refresh_and_parked_deletion_reap_helper_without_stale_delivery() {
    for action in [0, 1, 2] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.state.session_file = Some(format!("/fixture/svg-cancel-{action}"));
        let path = app.prefs.cwd.join("shape.svg");
        fs::write(&path, "<svg width='2' height='1'/>").unwrap();
        let pid_path = app.prefs.cwd.join("helper.pid");
        let release = app.prefs.cwd.join("helper.release");
        open(&mut app, &ctx, &path, script(&pid_path, &release));
        let pid = await_pid(&pid_path);
        let pool = pool(&ctx);
        let stale = app.files.tabs[0].rx.take().unwrap();
        match action {
            0 => app.files.close_tab(0),
            1 => {
                // Same-path replacement is plain text and must never receive the
                // old SVG result or retain its reserved budget/texture.
                fs::write(&path, "replacement source").unwrap();
                let reject = "import sys,struct;h=sys.stdin.buffer.read(8);sys.stdin.buffer.read(struct.unpack('<I',h[4:])[0]);sys.stdout.buffer.write(b'PSV1'+struct.pack('<II',2,0));sys.stdout.flush()";
                crate::svg_preview::test_support::with(Some(reject.into()), || {
                    app.files.refresh(&ctx)
                });
            }
            _ => {
                app.leave_session_layout();
                app.session_layouts
                    .forget_path(Path::new("/fixture/svg-cancel-2"));
            }
        }
        super::tests::idle(&pool);
        assert_reaped(pid);
        assert!(matches!(
            stale.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        if action == 1 {
            settle(&mut app.files);
            let Some(Ok(Content::Svg(preview))) = &app.files.tabs[0].content else {
                panic!("replacement missing")
            };
            assert_eq!(preview.test_state().0, "replacement source");
            assert!(preview.test_state().2.is_none());
            assert!(app.files.tabs[0].texture.is_none());
            app.files.close_tab(0);
        }
        let all = budget::Lease::new(&ctx);
        all.reserve(64 * 1024 * 1024).unwrap();
        assert_eq!(pool.0.lock().unwrap().active, 0);
    }
}
#[test]
fn svg_loader_park_keeps_explicit_helper_and_delivers_only_to_original_chat() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.session_file = Some("/fixture/svg-kept".into());
    let path = app.prefs.cwd.join("shape.svg");
    let source = "<svg width='2' height='1'/>";
    fs::write(&path, source).unwrap();
    let pid_path = app.prefs.cwd.join("helper.pid");
    let release = app.prefs.cwd.join("helper.release");
    open(&mut app, &ctx, &path, script(&pid_path, &release));
    let pid = await_pid(&pid_path);
    let pool = pool(&ctx);
    app.leave_session_layout();
    assert!(app.files.tabs.is_empty());
    // SAFETY: signal zero only tests that our controlled helper is still alive.
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
    fs::write(&release, b"release").unwrap();
    super::tests::idle(&pool);
    assert_reaped(pid);
    assert!(app.files.tabs.is_empty());
    app.enter_session_layout();
    settle(&mut app.files);
    let Some(Ok(Content::Svg(preview))) = &app.files.tabs[0].content else {
        panic!("parked preview missing")
    };
    assert_eq!(preview.test_state(), (source, None, Some([2, 1])));
    app.files.close_tab(0);
    let all = budget::Lease::new(&ctx);
    all.reserve(64 * 1024 * 1024).unwrap();
}
