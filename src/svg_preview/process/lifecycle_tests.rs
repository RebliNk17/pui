use super::*;
#[cfg(unix)]
fn command(path: &std::path::Path, body: &str) -> Command {
    // Record the direct child's PID before Python initialization, which may
    // legitimately take longer than the deliberately short protocol timeout.
    // exec preserves that PID: the assertion still checks the actual owned
    // helper, including cancellation before Python finishes starting.
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-c",
            "printf '%s' \"$$\" > \"$1\"; exec python3 -c \"$2\"",
            "pui-svg-helper-fixture",
        ])
        .arg(path)
        .arg(body);
    command
}
#[cfg(unix)]
fn assert_reaped(path: &std::path::Path) {
    let pid = std::fs::read_to_string(path)
        .unwrap()
        .parse::<i32>()
        .unwrap();
    // SAFETY: signal zero inspects existence only; it never signals a process.
    let exists = unsafe { libc::kill(pid, 0) };
    assert_eq!(exists, -1, "owned SVG helper {pid} survived");
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}
#[test]
#[cfg(unix)]
fn timeout_cancel_exit_stall_and_failure_reap_only_owned_direct_helper() {
    for body in ["import time;time.sleep(30)", "import sys;sys.exit(7)"] {
        let path = std::env::temp_dir().join(format!("pui-svg-owned-{}", uuid::Uuid::new_v4()));
        let result = run(
            &mut command(&path, body),
            "x",
            None,
            |_| Ok(()),
            Duration::from_millis(300),
        );
        assert!(result.is_err());
        assert_reaped(&path);
    }
    let path = std::env::temp_dir().join(format!("pui-svg-owned-{}", uuid::Uuid::new_v4()));
    let cancel = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let path_ref = &path;
        let cancel_ref = &cancel;
        scope.spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !path_ref.exists() {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            cancel_ref.store(true, Ordering::Release);
        });
        assert!(
            run(
                &mut command(&path, "import time;time.sleep(30)"),
                "x",
                Some(&cancel),
                |_| Ok(()),
                Duration::from_secs(3)
            )
            .unwrap_err()
            .contains("cancelled")
        );
    });
    assert_reaped(&path);
    let path = std::env::temp_dir().join(format!("pui-svg-owned-{}", uuid::Uuid::new_v4()));
    let body = "import sys,struct,time;h=sys.stdin.buffer.read(8);sys.stdin.buffer.read(struct.unpack('<I',h[4:])[0]);sys.stdout.buffer.write(b'PSV1'+struct.pack('<II',1,1));sys.stdout.flush();sys.stdin.buffer.read(1);sys.stdout.buffer.write(b'1234');sys.stdout.flush();sys.stdout.close();time.sleep(30)";
    assert!(
        run(
            &mut command(&path, body),
            "x",
            None,
            |_| Ok(()),
            Duration::from_millis(300)
        )
        .unwrap_err()
        .contains("timed out")
    );
    assert_reaped(&path);
}
