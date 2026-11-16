use super::*;
#[test]
fn failing_hook_captures_both_pipes_escapes_controls_and_never_runs_output() {
    let mut command = std::process::Command::new("/bin/sh");
    command.args([
        "-c",
        "printf 'stdout 世界\\n'; printf '\\033[31mstderr\\n' >&2; exit 7",
    ]);
    let error = run(
        &mut command,
        &AtomicBool::new(false),
        Instant::now() + std::time::Duration::from_secs(3),
        "Test hook",
    )
    .unwrap_err();
    assert!(error.contains("stdout 世界"));
    assert!(error.contains("stderr"));
    assert!(!error.contains('\u{1b}'));
    assert!(error.contains("Untrusted local hook output"));
}
#[test]
fn oversized_stderr_refuses_whole_and_cancel_never_spawns() {
    let mut command = std::process::Command::new("/bin/sh");
    command.args(["-c", "yes secret | head -c 20000 >&2; exit 1"]);
    let error = run(
        &mut command,
        &AtomicBool::new(false),
        Instant::now() + std::time::Duration::from_secs(3),
        "Test hook",
    )
    .unwrap_err();
    assert!(error.contains("exceeds 16 KiB"));
    assert!(!error.contains("secret"));
    let mut never = std::process::Command::new("/does/not/exist");
    assert!(
        run(
            &mut never,
            &AtomicBool::new(true),
            Instant::now() + std::time::Duration::from_secs(3),
            "Test hook"
        )
        .unwrap_err()
        .contains("cancelled")
    );
}
