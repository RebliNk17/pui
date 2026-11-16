//! Explicit first-parent metadata from one pinned local commit. Never a Git write.
pub(crate) mod details;
pub(crate) mod files;
pub(crate) mod patch;
use super::read_only::{self, DEADLINE};
use std::{path::Path, process::Command, sync::atomic::AtomicBool, time::Instant};

pub(crate) const MAX_COMMITS: usize = 100;
const MAX_BYTES: usize = 128 * 1024;
const MAX_SUBJECT: usize = 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Commit {
    pub oid: String,
    pub time: i64,
    pub subject: String,
    pub first_parent: Option<String>,
    pub parent_count: u8,
}
#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub head: String,
    pub commits: Vec<Commit>,
    pub limited: bool,
}
pub(super) fn oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn command(root: &Path) -> Command {
    let mut command = read_only::command(root);
    // Read a local immutable object, never replacements or lazy network fetch.
    command.env("GIT_NO_REPLACE_OBJECTS", "1");
    command
}
fn log(root: &Path, head: &str) -> Command {
    log_mode(root, head, false)
}
fn log_mode(root: &Path, head: &str, merged: bool) -> Command {
    let mut command = command(root);
    command.args([
        "log",
        if merged {
            "--topo-order"
        } else {
            "--first-parent"
        },
        "--max-count=101",
        "--no-patch",
        "--no-decorate",
        "--no-show-signature",
        "--no-notes",
        "--no-use-mailmap",
        "--no-ext-diff",
        "--no-textconv",
        "--encoding=UTF-8",
        "--format=%H%x00%ct%x00%P%x00%s%x00",
        "--end-of-options",
        head,
        "--",
    ]);
    command
}
fn parse(bytes: &[u8], head: String) -> Result<Snapshot, String> {
    if !oid(&head) || bytes.len() > MAX_BYTES {
        return Err("Recent history response exceeds its limits.".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "Recent history is not UTF-8.")?;
    let mut commits = Vec::new();
    for row in text.split_inclusive("\0\n") {
        if commits.len() > MAX_COMMITS {
            return Err("Invalid recent history count.".into());
        }
        let mut fields = row
            .strip_suffix("\0\n")
            .ok_or("Invalid recent history framing.")?
            .split('\0');
        let hash = fields
            .next()
            .filter(|value| oid(value) && value.len() == head.len())
            .ok_or("Invalid history object ID.")?;
        let time = fields
            .next()
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|value| *value >= 0 && *value <= 253_402_300_799)
            .ok_or("Invalid history timestamp.")?;
        let parents = fields.next().ok_or("Missing history parents.")?;
        let mut parent_count = 0u8;
        let mut first_parent = None;
        let mut seen = std::collections::BTreeSet::new();
        if !parents.is_empty() {
            for parent in parents.split(' ') {
                if parent_count >= 16
                    || !oid(parent)
                    || parent.len() != head.len()
                    || parent == hash
                    || !seen.insert(parent)
                {
                    return Err("Invalid or excessive history parents.".into());
                }
                if first_parent.is_none() {
                    first_parent = Some(parent.to_owned());
                }
                parent_count += 1;
            }
        }
        let subject = fields
            .next()
            .filter(|value| value.len() <= MAX_SUBJECT && !value.chars().any(char::is_control))
            .ok_or("History subject exceeds 1 KiB or contains unsupported controls.")?;
        if fields.next().is_some() || commits.iter().any(|commit: &Commit| commit.oid == hash) {
            return Err("Invalid recent history record.".into());
        }
        commits.push(Commit {
            oid: hash.into(),
            time,
            subject: subject.into(),
            first_parent,
            parent_count,
        });
    }
    if commits.first().map(|commit| commit.oid.as_str()) != Some(head.as_str()) {
        return Err("History does not start at the requested local commit.".into());
    }
    let limited = commits.len() > MAX_COMMITS;
    commits.truncate(MAX_COMMITS);
    Ok(Snapshot {
        head,
        commits,
        limited,
    })
}
pub(crate) fn read(root: &Path, cancel: &AtomicBool) -> Result<Snapshot, String> {
    read_mode(root, cancel, false)
}
pub(crate) fn read_mode(
    root: &Path,
    cancel: &AtomicBool,
    merged: bool,
) -> Result<Snapshot, String> {
    let deadline = Instant::now() + DEADLINE;
    let (bytes, status) = read_only::run(
        command(root).args(["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]),
        cancel,
        deadline,
        128,
        "History HEAD read",
        "Invalid history HEAD response.",
    )?;
    if !status.success() {
        return Err(
            "Local HEAD has no readable commit (unborn, unavailable or not a repository).".into(),
        );
    }
    let head = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|text| text.strip_suffix('\n'))
        .filter(|value| oid(value))
        .ok_or("Invalid local history HEAD response.")?
        .to_owned();
    let (bytes, status) = read_only::run(
        &mut if merged {
            log_mode(root, &head, true)
        } else {
            log(root, &head)
        },
        cancel,
        deadline,
        MAX_BYTES,
        "Recent history read",
        "Recent history exceeds 128 KiB. No partial list is shown.",
    )?;
    if !status.success() {
        return Err("Local history is unavailable. No network fetch was attempted.".into());
    }
    parse(&bytes, head)
}
#[cfg(test)]
mod tests;
