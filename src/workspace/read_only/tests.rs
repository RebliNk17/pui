use super::*;

#[cfg(unix)]
fn shell(script: &str) -> Command {
    let mut command = Command::new("sh");
    command
        .args(["-c", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
}

#[cfg(unix)]
pub(crate) fn stalled(cancel: &AtomicBool, started: std::sync::mpsc::Sender<()>) -> (String, bool) {
    stalled_for(cancel, started, DEADLINE)
}
#[cfg(unix)]
pub(crate) fn stalled_for(
    cancel: &AtomicBool,
    started: std::sync::mpsc::Sender<()>,
    duration: Duration,
) -> (String, bool) {
    let mut child = shell("exec sleep 30").spawn().unwrap();
    started.send(()).unwrap();
    let error = supervise(
        &mut child,
        cancel,
        Instant::now() + duration,
        64,
        "fixture",
        "too large",
    )
    .unwrap_err();
    (error, child.try_wait().unwrap().is_some())
}

#[cfg(unix)]
#[test]
fn stalled_stdout_closed_stdout_oversize_cancel_and_success_reap_direct_child() {
    for (script, duration, cancelled, max, expected) in [
        (
            "exec sleep 30",
            Duration::from_millis(35),
            false,
            64,
            "timed out",
        ),
        (
            "exec 1>&-; exec sleep 30",
            Duration::from_millis(35),
            false,
            64,
            "timed out",
        ),
        (
            "exec sleep 30",
            Duration::from_secs(1),
            true,
            64,
            "cancelled",
        ),
        (
            "while :; do printf 0123456789abcdef; done",
            Duration::from_secs(1),
            false,
            64,
            "too large",
        ),
    ] {
        let mut child = shell(script).spawn().unwrap();
        let pid = child.id();
        let started = Instant::now();
        let error = supervise(
            &mut child,
            &AtomicBool::new(cancelled),
            started + duration,
            max,
            "fixture read",
            "too large",
        )
        .unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(child.try_wait().unwrap().is_some());
        let mut status = 0;
        // SAFETY: nonblocking wait for the known fixture child only. The supervisor already reaped it.
        assert_eq!(
            unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }
    let mut child = shell("printf 0123456789abcdef").spawn().unwrap();
    let (bytes, status) = supervise(
        &mut child,
        &AtomicBool::new(false),
        Instant::now() + DEADLINE,
        16,
        "fixture",
        "too large",
    )
    .unwrap();
    assert_eq!(bytes, b"0123456789abcdef");
    assert!(status.success() && child.try_wait().unwrap().is_some());
}

#[cfg(unix)]
#[test]
fn one_deadline_is_shared_and_preflight_cancel_never_spawns() {
    let deadline = Instant::now() + Duration::from_millis(500);
    let cancel = AtomicBool::new(false);
    let start = Instant::now();
    run(
        &mut shell("printf a"),
        &cancel,
        deadline,
        16,
        "fixture",
        "too large",
    )
    .unwrap();
    assert!(
        run(
            &mut shell("exec sleep 30"),
            &cancel,
            deadline,
            16,
            "fixture",
            "too large"
        )
        .unwrap_err()
        .contains("timed out")
    );
    assert!(start.elapsed() < Duration::from_secs(1));
    let mut absent = Command::new("/nonexistent-pui-fixture-command");
    assert!(
        run(
            &mut absent,
            &AtomicBool::new(true),
            Instant::now() + DEADLINE,
            16,
            "fixture",
            "too large"
        )
        .unwrap_err()
        .contains("cancelled")
    );
}

#[test]
fn receiver_move_preserves_ownership_drop_cancels_even_if_delivery_finished() {
    let (tx, rx) = std::sync::mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let request = Request::new(rx, cancel.clone());
    tx.send(42).unwrap();
    let moved = Some(request);
    assert!(!cancel.load(Ordering::Acquire));
    assert_eq!(moved.as_ref().unwrap().try_recv().unwrap(), 42);
    drop(moved);
    assert!(cancel.load(Ordering::Acquire));
    assert!(tx.send(99).is_err());
}

#[test]
fn git_configuration_is_read_only_local_noninteractive_and_environment_isolated() {
    let command = command(Path::new("/fixture"));
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        [
            "--no-optional-locks",
            "--no-pager",
            "-c",
            "core.fsmonitor=false"
        ]
    );
    assert!(
        command
            .get_envs()
            .any(|(key, value)| key == "GIT_NO_LAZY_FETCH"
                && value == Some(std::ffi::OsStr::new("1")))
    );
    for variable in [
        "GIT_DIR",
        "GIT_COMMON_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        assert!(
            command
                .get_envs()
                .any(|(key, value)| key == variable && value.is_none())
        );
    }
}
