//! Explicit, local-only branch discovery. No branch name is ever a command argument.
use std::{path::Path, process::Command, sync::atomic::AtomicBool, time::Instant};

pub(crate) const MAX_BRANCHES: usize = 256;
pub(crate) const MAX_NAME_BYTES: usize = 1024;
const MAX_BYTES: usize = 128 * 1024;
use super::read_only::{self, DEADLINE};

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
    let mut command = read_only::command(root);
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

#[cfg(test)]
fn supervise(
    child: &mut std::process::Child,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<(Vec<u8>, std::process::ExitStatus), String> {
    read_only::supervise(
        child,
        cancel,
        deadline,
        MAX_BYTES,
        "Local branch read",
        "Local branch response exceeds 128 KiB. No partial list is shown.",
    )
}
fn run(
    root: &Path,
    list: bool,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<(Vec<u8>, std::process::ExitStatus), String> {
    read_only::run(
        &mut command(root, list),
        cancel,
        deadline,
        MAX_BYTES,
        "Local branch read",
        "Local branch response exceeds 128 KiB. No partial list is shown.",
    )
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
