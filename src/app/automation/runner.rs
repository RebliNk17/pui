//! Bounded non-AI shell checks. No transcript or model access.
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct Check {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub exit_code: Option<i32>,
    pub elapsed_ms: Option<u64>,
    pub truncated: bool,
}
fn terminate(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}
fn drain(mut reader: impl Read) -> (String, bool) {
    let mut truncated = false;
    let mut bytes = Vec::new();
    let mut buf = [0; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let keep = n.min(4096_usize.saturating_sub(bytes.len()));
                truncated |= keep < n;
                bytes.extend_from_slice(&buf[..keep]);
            }
        }
    }
    (String::from_utf8_lossy(&bytes).into_owned(), truncated)
}
pub(super) fn run(
    cwd: &std::path::Path,
    command: &str,
    timeout: u64,
    cancel: Arc<AtomicBool>,
) -> Check {
    if cancel.load(Ordering::Acquire) {
        return Check {
            success: false,
            output: String::new(),
            error: Some("Check cancelled".into()),
            ..Default::default()
        };
    }
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", command]);
        c
    };
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("cmd.exe");
        c.args(["/D", "/S", "/C", command]);
        c
    };
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    if let Some(path) = crate::rpc::child_path() {
        cmd.env("PATH", path);
    }
    let mut child = match cmd
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            return Check {
                success: false,
                output: String::new(),
                error: Some("Cannot start monitor shell in its saved workspace.".into()),
                ..Default::default()
            };
        }
    };
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let stdout = std::thread::spawn(move || drain(out));
    let stderr = std::thread::spawn(move || drain(err));
    let start = Instant::now();
    let mut exit_code = None;
    let (success, error) = loop {
        if cancel.load(Ordering::Acquire) {
            break (false, Some("Check cancelled".into()));
        }
        if start.elapsed() >= Duration::from_secs(timeout) {
            break (false, Some("Check timed out".into()));
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                exit_code = status.code();
                break (
                    status.success(),
                    match status.code() {
                        Some(0 | 1) => None,
                        Some(code) => Some(format!("Check exited with status {code}")),
                        None => Some("Check was terminated by a signal".into()),
                    },
                );
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => break (false, Some("Cannot read monitor exit status".into())),
        }
    };
    // Also clean up descendants that inherited the pipes after their shell exited.
    terminate(&mut child);
    let (out, out_cut) = stdout.join().unwrap_or_default();
    let (err, err_cut) = stderr.join().unwrap_or_default();
    let mut output = format!("{out}{err}");
    let truncated = out_cut || err_cut || output.len() > 4000;
    let mut end = output.len().min(4000);
    while !output.is_char_boundary(end) {
        end -= 1;
    }
    output.truncate(end);
    Check {
        success,
        output,
        error,
        exit_code,
        elapsed_ms: Some(start.elapsed().as_millis().min(u64::MAX as u128) as u64),
        truncated,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn observed_exit_duration_and_truncation_are_explicit() {
        let cwd = std::env::temp_dir();
        let cancel = Arc::new(AtomicBool::new(false));
        let one = run(&cwd, "printf waiting; exit 1", 2, cancel.clone());
        assert_eq!(one.exit_code, Some(1));
        assert!(one.elapsed_ms.is_some());
        assert!(!one.truncated);
        assert_eq!(one.output, "waiting");
        let failed = run(&cwd, "exit 7", 2, cancel.clone());
        assert_eq!(failed.exit_code, Some(7));
        assert!(failed.error.is_some());
        let large = run(&cwd, "yes x | head -c 100000", 2, cancel.clone());
        assert!(large.truncated);
        assert!(large.output.len() <= 4000);
        let timeout = run(&cwd, "sleep 5", 1, cancel);
        assert_eq!(timeout.exit_code, None);
        assert!(timeout.elapsed_ms.unwrap() >= 1000);
        assert!(timeout.error.as_deref().unwrap().contains("timed out"));
    }
    #[test]
    fn bounded_output_exit_codes_and_timeout() {
        let cancel = Arc::new(AtomicBool::new(false));
        let cwd = std::env::temp_dir();
        assert!(run(&cwd, "printf ready", 2, cancel.clone()).success);
        assert!(!run(&cwd, "exit 1", 2, cancel.clone()).success);
        assert!(run(&cwd, "exit 2", 2, cancel.clone()).error.is_some());
        assert!(
            run(&cwd, "yes x | head -c 100000", 2, cancel.clone())
                .output
                .len()
                <= 4000
        );
        let t = Instant::now();
        assert_eq!(
            run(&cwd, "sleep 10", 1, cancel).error.as_deref(),
            Some("Check timed out")
        );
        assert!(t.elapsed() < Duration::from_secs(3));
    }
}
