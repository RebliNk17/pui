//! Explicit, local-only branch discovery. No branch name is ever a command argument.
use std::{
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub(crate) const MAX_BRANCHES: usize = 256;
pub(crate) const MAX_NAME_BYTES: usize = 1024;
const MAX_BYTES: usize = 128 * 1024;
const DEADLINE: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Current {
    Local(String),
    Detached,
}
#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub current: Current,
    pub local: Vec<String>,
}

fn valid_ref(value: &str) -> bool {
    value.starts_with("refs/heads/")
        && value.len() > "refs/heads/".len()
        && value.len() <= MAX_NAME_BYTES
        && !value.chars().any(|c| c.is_control() || c.is_whitespace())
        && !value.contains(['~', '^', ':', '?', '*', '[', '\\'])
        && !value.contains("..")
        && !value.contains("@{")
        && value.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && !part.ends_with('.')
                && !part.ends_with(".lock")
        })
}
fn current(bytes: Vec<u8>, success: bool, code: Option<i32>) -> Result<Current, String> {
    if !success && code == Some(1) && bytes.is_empty() {
        return Ok(Current::Detached);
    }
    let value = std::str::from_utf8(&bytes).map_err(|_| "Current branch is not UTF-8.")?;
    let value = value
        .strip_suffix('\n')
        .ok_or("Invalid current branch response.")?;
    if !success || !valid_ref(value) {
        return Err("Cannot read the current local branch.".into());
    }
    Ok(Current::Local(value["refs/heads/".len()..].into()))
}
fn parse(bytes: Vec<u8>, current: Current) -> Result<Snapshot, String> {
    let text = std::str::from_utf8(&bytes).map_err(|_| "Local branch list is not UTF-8.")?;
    let mut local = Vec::new();
    for line in text.split_inclusive('\n') {
        let value = line
            .strip_suffix("\0\n")
            .ok_or("Invalid local branch response.")?;
        if !valid_ref(value)
            || local
                .last()
                .is_some_and(|last: &String| last.as_str() >= &value["refs/heads/".len()..])
        {
            return Err("Invalid local branch name or ordering.".into());
        }
        if local.len() == MAX_BRANCHES {
            return Err("Local branch list exceeds 256 branches. No partial list is shown.".into());
        }
        local.push(value["refs/heads/".len()..].into());
    }
    Ok(Snapshot { current, local })
}

fn command(root: &Path, list: bool) -> Command {
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
    if list {
        command.args([
            "for-each-ref",
            "--sort=refname",
            "--count=257",
            "--format=%(refname)%00",
            "refs/heads/",
        ]);
    } else {
        command.args(["symbolic-ref", "--quiet", "HEAD"]);
    }
    command
}

// The only production platform is Unix/macOS. Fail closed elsewhere instead of
// introducing an uncancellable pipe reader or an unbounded helper thread.
#[cfg(unix)]
fn supervise(
    child: &mut Child,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<(Vec<u8>, std::process::ExitStatus), String> {
    use std::os::fd::AsRawFd;
    let mut pipe = child
        .stdout
        .take()
        .ok_or("Cannot read local Git branches.")?;
    let fd = pipe.as_raw_fd();
    // SAFETY: pipe owns this descriptor throughout this bounded read.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err("Cannot prepare bounded local branch read.".into());
    }
    let mut bytes = Vec::with_capacity(MAX_BYTES + 1);
    let mut eof = false;
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err("Local branch read cancelled.".into());
        }
        if Instant::now() >= deadline {
            return Err("Local branch read timed out (3 seconds). Refresh to retry.".into());
        }
        // At most MAX_BYTES + 1 bytes are read/allocated, even for corrupt refs.
        let mut buffer = [0; 4096];
        let capacity = buffer.len().min(MAX_BYTES + 1 - bytes.len());
        let mut waiting = false;
        match pipe.read(&mut buffer[..capacity]) {
            Ok(0) => eof = true,
            Ok(n) => bytes.extend_from_slice(&buffer[..n]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => waiting = true,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err("Cannot read local Git branches.".into()),
        }
        if bytes.len() > MAX_BYTES {
            return Err("Local branch response exceeds 128 KiB. No partial list is shown.".into());
        }
        if eof
            && let Some(status) = child
                .try_wait()
                .map_err(|_| "Cannot finish local Git read.")?
        {
            return Ok((bytes, status));
        }
        // Only this active worker waits; no UI timer or idle repository polling.
        if eof || waiting {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
#[cfg(not(unix))]
fn supervise(
    _: &mut Child,
    _: &AtomicBool,
    _: Instant,
) -> Result<(Vec<u8>, std::process::ExitStatus), String> {
    Err("Bounded local branch discovery is unavailable on this platform.".into())
}
fn run(
    root: &Path,
    list: bool,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<(Vec<u8>, std::process::ExitStatus), String> {
    if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Err("Local branch read cancelled or timed out. Refresh to retry.".into());
    }
    let mut child = command(root, list)
        .spawn()
        .map_err(|_| "Cannot start local Git branch read.")?;
    let result = supervise(&mut child, cancel, deadline);
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait(); // reap our direct Git process before releasing the worker permit
    }
    result
}

pub(crate) fn read(root: &Path, cancel: &AtomicBool) -> Result<Snapshot, String> {
    let deadline = Instant::now() + DEADLINE;
    let (bytes, status) = run(root, false, cancel, deadline)?;
    let before = current(bytes, status.success(), status.code())?;
    let (bytes, status) = run(root, true, cancel, deadline)?;
    if !status.success() {
        return Err("Local branches unavailable. Repository may have changed.".into());
    }
    let snapshot = parse(bytes, before.clone())?;
    let (bytes, status) = run(root, false, cancel, deadline)?;
    if before != current(bytes, status.success(), status.code())? {
        return Err("Current branch changed during discovery. Refresh to retry.".into());
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests;
