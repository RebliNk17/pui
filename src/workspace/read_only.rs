//! Shared local Git pipe supervision. Deadlines bound ordinary userspace waits,
//! not an uninterruptible kernel wait. Only our direct child is killed/reaped.
use std::{
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

pub(super) const DEADLINE: Duration = Duration::from_secs(3);

/// Delivery owns cancellation: replacing/dropping the receiver abandons the
/// actual read, including when a saved session layout would otherwise retain it.
pub(crate) struct Request<T> {
    rx: Receiver<T>,
    cancel: Arc<AtomicBool>,
}
impl<T> Request<T> {
    pub(crate) fn new(rx: Receiver<T>, cancel: Arc<AtomicBool>) -> Self {
        Self { rx, cancel }
    }
    pub(crate) fn try_recv(&self) -> Result<T, TryRecvError> {
        self.rx.try_recv()
    }
    #[cfg(test)]
    pub(crate) fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<T, std::sync::mpsc::RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }
}
impl<T> Drop for Request<T> {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
#[cfg(test)]
impl<T> From<Receiver<T>> for Request<T> {
    fn from(rx: Receiver<T>) -> Self {
        Self::new(rx, Arc::new(AtomicBool::new(false)))
    }
}

pub(super) fn command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args([
            "--no-optional-locks",
            "--no-pager",
            "-c",
            "core.fsmonitor=false",
        ])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for variable in [
        "GIT_DIR",
        "GIT_COMMON_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(variable);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

/// Fail closed even for unused configured drivers: status/diff can otherwise
/// invoke clean/process commands while merely comparing worktree contents.
/// Names only; configuration values and stderr never enter UI/error text.
/// As with Git itself, concurrently changing trusted repository config is not
/// an atomic snapshot; this check is not a sandbox against config-file races.
pub(super) fn filters_disabled(
    root: &Path,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<(), String> {
    filter_check(root, cancel, deadline, false)
}
/// Checkout may execute smudge drivers even when read-only inspection is safe.
pub(super) fn checkout_filters_disabled(
    root: &Path,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<(), String> {
    filter_check(root, cancel, deadline, true)
}
fn filter_check(
    root: &Path,
    cancel: &AtomicBool,
    deadline: Instant,
    checkout: bool,
) -> Result<(), String> {
    let pattern = if checkout {
        "^filter\\..*\\.(clean|smudge|process)$"
    } else {
        "^filter\\..*\\.(clean|process)$"
    };
    let (names, exit) = run(
        command(root).args([
            "config",
            "--includes",
            "--null",
            "--name-only",
            "--get-regexp",
            pattern,
        ]),
        cancel,
        deadline,
        4096,
        "Git filter configuration read",
        "Git filter configuration exceeds 4 KiB. Working-tree inspection is unavailable.",
    )?;
    if exit.code() == Some(1) && names.is_empty() {
        return Ok(());
    }
    if exit.success() && !names.is_empty() {
        if checkout {
            return Err("Git clean/smudge/process filters are configured. Branch switching is unavailable because checkout filters can execute commands.".into());
        }
        return Err("Git clean/process filters are configured. Read-only working-tree inspection is unavailable because these filters can execute commands.".into());
    }
    Err("Cannot verify Git filter configuration. Working-tree inspection is unavailable.".into())
}

pub(super) fn run(
    command: &mut Command,
    cancel: &AtomicBool,
    deadline: Instant,
    max_bytes: usize,
    label: &str,
    over_limit: &str,
) -> Result<(Vec<u8>, ExitStatus), String> {
    check(cancel, deadline, label)?;
    let mut child = command
        .spawn()
        .map_err(|_| format!("Cannot start read-only {label}."))?;
    supervise(&mut child, cancel, deadline, max_bytes, label, over_limit)
}

fn check(cancel: &AtomicBool, deadline: Instant, label: &str) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        return Err(format!("{label} cancelled."));
    }
    if Instant::now() >= deadline {
        return Err(format!("{label} timed out (3 seconds). Refresh to retry."));
    }
    Ok(())
}

pub(super) fn supervise(
    child: &mut Child,
    cancel: &AtomicBool,
    deadline: Instant,
    max_bytes: usize,
    label: &str,
    over_limit: &str,
) -> Result<(Vec<u8>, ExitStatus), String> {
    let result = drain(child, cancel, deadline, max_bytes, label, over_limit);
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait(); // finish direct-child cleanup before releasing a worker permit
    }
    result
}

// Fail closed off Unix rather than creating an uncancellable pipe-reader thread.
#[cfg(unix)]
fn drain(
    child: &mut Child,
    cancel: &AtomicBool,
    deadline: Instant,
    max_bytes: usize,
    label: &str,
    over_limit: &str,
) -> Result<(Vec<u8>, ExitStatus), String> {
    use std::{io::Read, os::fd::AsRawFd};
    let mut pipe = child
        .stdout
        .take()
        .ok_or_else(|| format!("Cannot read {label}."))?;
    let fd = pipe.as_raw_fd();
    // SAFETY: pipe owns this descriptor throughout this bounded read.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(format!("Cannot prepare bounded {label}."));
    }
    let mut bytes = Vec::with_capacity(max_bytes + 1);
    let mut eof = false;
    loop {
        check(cancel, deadline, label)?;
        let mut waiting = false;
        if !eof {
            let mut buffer = [0; 4096];
            let capacity = buffer.len().min(max_bytes + 1 - bytes.len());
            match pipe.read(&mut buffer[..capacity]) {
                Ok(0) => eof = true,
                Ok(n) => bytes.extend_from_slice(&buffer[..n]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => waiting = true,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(format!("Cannot read {label}.")),
            }
            if bytes.len() > max_bytes {
                return Err(over_limit.into());
            }
        }
        if eof
            && let Some(status) = child
                .try_wait()
                .map_err(|_| format!("Cannot finish {label}."))?
        {
            return Ok((bytes, status));
        }
        if eof || waiting {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
#[cfg(not(unix))]
fn drain(
    _: &mut Child,
    _: &AtomicBool,
    _: Instant,
    _: usize,
    label: &str,
    _: &str,
) -> Result<(Vec<u8>, ExitStatus), String> {
    Err(format!("Bounded {label} is unavailable on this platform."))
}

pub(crate) fn worktree_list_porcelain(root: &Path) -> Result<String, String> {
    let output = command(root)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .map_err(|e| format!("Failed to run git worktree list: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "git worktree list failed.".into()
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
pub(crate) mod tests;
