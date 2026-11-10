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

pub(super) struct Check {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
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
fn drain(mut reader: impl Read) -> String {
    let mut bytes = Vec::new();
    let mut buf = [0; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let keep = n.min(4096_usize.saturating_sub(bytes.len()));
                bytes.extend_from_slice(&buf[..keep]);
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
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
            };
        }
    };
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let stdout = std::thread::spawn(move || drain(out));
    let stderr = std::thread::spawn(move || drain(err));
    let start = Instant::now();
    let (success, error) = loop {
        if cancel.load(Ordering::Acquire) {
            break (false, Some("Check cancelled".into()));
        }
        if start.elapsed() >= Duration::from_secs(timeout) {
            break (false, Some("Check timed out".into()));
        }
        match child.try_wait() {
            Ok(Some(status)) => {
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
    let mut output = format!(
        "{}{}",
        stdout.join().unwrap_or_default(),
        stderr.join().unwrap_or_default()
    );
    let mut end = output.len().min(4000);
    while !output.is_char_boundary(end) {
        end -= 1;
    }
    output.truncate(end);
    Check {
        success,
        output,
        error,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
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
