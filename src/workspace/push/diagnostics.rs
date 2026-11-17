//! Bounded dual-pipe hook diagnostics; no automatic draft capture or execution.
use super::*;
use std::io::Read;
const LIMIT: usize = 16 * 1024;
#[cfg(unix)]
pub(super) fn run(
    command: &mut std::process::Command,
    cancel: &AtomicBool,
    deadline: Instant,
    label: &str,
) -> Result<(Vec<u8>, Vec<u8>, std::process::ExitStatus), String> {
    use std::{os::fd::AsRawFd, process::Stdio, sync::atomic::Ordering, time::Duration};
    if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Err(format!(
            "{label} cancelled or timed out; remote completion may be uncertain."
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
                return Err("Cannot bound hook pipes; remote completion may be uncertain.".into());
            }
        }
        let mut streams = [Vec::new(), Vec::new()];
        let mut done = [false; 2];
        loop {
            if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(format!(
                    "{label} cancelled or timed out; output omitted, remote completion may be uncertain. Inspect local state."
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
                        if streams[0].len() + streams[1].len() + n > LIMIT {
                            return Err(format!(
                                "{label} output exceeds 16 KiB; output omitted, remote completion may be uncertain."
                            ));
                        }
                        streams[index].extend_from_slice(&buffer[..n]);
                    }
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => {
                        return Err(
                            "Cannot read hook output; remote completion may be uncertain.".into(),
                        );
                    }
                }
            }
            if done.iter().all(|v| *v)
                && let Some(status) = child.try_wait().map_err(|_| "Cannot finish hook")?
            {
                let [out, err] = streams;
                return Ok((out, err, status));
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
) -> Result<(Vec<u8>, Vec<u8>, std::process::ExitStatus), String> {
    Err("Bounded hook diagnostics unavailable on this platform.".into())
}
