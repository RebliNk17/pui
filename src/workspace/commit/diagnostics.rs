//! Bounded dual-pipe hook diagnostics; no automatic draft capture or execution.
use super::*;
const LIMIT: usize = 16 * 1024;
fn failure(label: &str, bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut literal = String::new();
    for ch in text.chars() {
        if ch.is_control() && !matches!(ch, '\n' | '\t') {
            literal.extend(ch.escape_default());
        } else {
            literal.push(ch);
        }
    }
    format!(
        "{label} failed. Nothing committed; no bypass fallback.\nUntrusted local hook output — may contain secrets; review before sharing. stdout/stderr ordering is not preserved.\n{}",
        if literal.is_empty() {
            "(no output)"
        } else {
            &literal
        }
    )
}
#[cfg(unix)]
pub(super) fn run(
    command: &mut std::process::Command,
    cancel: &AtomicBool,
    deadline: Instant,
    label: &str,
) -> Result<(), String> {
    use std::{os::fd::AsRawFd, process::Stdio, sync::atomic::Ordering, time::Duration};
    if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Err(format!(
            "{label} cancelled or timed out; nothing committed."
        ));
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|_| format!("Cannot start {label}."))?;
    let result = (|| {
        let mut out = child.stdout.take().ok_or("Missing hook stdout")?;
        let mut err = child.stderr.take().ok_or("Missing hook stderr")?;
        for fd in [out.as_raw_fd(), err.as_raw_fd()] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return Err("Cannot bound hook pipes; nothing committed.".into());
            }
        }
        let mut bytes = Vec::new();
        let mut done = [false; 2];
        loop {
            if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(format!(
                    "{label} cancelled or timed out; output omitted, nothing committed. Inspect local state."
                ));
            }
            for (index, pipe) in [&mut out as &mut dyn Read, &mut err as &mut dyn Read]
                .into_iter()
                .enumerate()
            {
                if done[index] {
                    continue;
                }
                let mut buffer = [0u8; 2048];
                match pipe.read(&mut buffer) {
                    Ok(0) => done[index] = true,
                    Ok(n) => {
                        if bytes.len() + n > LIMIT {
                            return Err(format!(
                                "{label} output exceeds 16 KiB; output omitted, nothing committed."
                            ));
                        }
                        bytes.extend_from_slice(&buffer[..n]);
                    }
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => return Err("Cannot read hook output; nothing committed.".into()),
                }
            }
            if done.iter().all(|v| *v)
                && let Some(status) = child.try_wait().map_err(|_| "Cannot finish hook")?
            {
                return if status.success() {
                    Ok(())
                } else {
                    Err(failure(label, &bytes))
                };
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}
#[cfg(not(unix))]
pub(super) fn run(
    _: &mut std::process::Command,
    _: &AtomicBool,
    _: Instant,
    _: &str,
) -> Result<(), String> {
    Err("Bounded hook diagnostics unavailable on this platform.".into())
}
#[cfg(test)]
mod tests;
