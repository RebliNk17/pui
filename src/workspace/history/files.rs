//! Exact local commit tree comparison; never reads the index or working tree.
use super::*;
const MAX_BYTES: usize = 128 * 1024;
const MAX_FILES: usize = 256;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub status: char,
    pub path: String,
}
fn command(root: &Path, hash: &str, parent: Option<&str>) -> Command {
    let mut cmd = super::command(root);
    cmd.args([
        "diff-tree",
        "--no-commit-id",
        "--no-color",
        "--name-status",
        "-z",
        "-r",
        "--no-renames",
        "--no-ext-diff",
        "--no-textconv",
        "--ignore-submodules=none",
    ]);
    if parent.is_none() {
        cmd.arg("--root");
    }
    cmd.arg("--end-of-options");
    if let Some(parent) = parent {
        cmd.arg(parent);
    }
    cmd.arg(hash).arg("--");
    cmd
}
pub(in crate::workspace) fn parse(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    if bytes.len() > MAX_BYTES {
        return Err("Commit file list exceeds 128 KiB. Nothing is shown.".into());
    }
    if bytes.is_empty() {
        return Ok(vec![]);
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| "Commit paths are not UTF-8. Nothing is shown.")?;
    let mut fields = text
        .strip_suffix('\0')
        .ok_or("Invalid commit file framing.")?
        .split('\0');
    let mut entries = Vec::new();
    while let Some(status) = fields.next() {
        let path = fields.next().ok_or("Incomplete commit file entry.")?;
        if entries.len() >= MAX_FILES
            || path.is_empty()
            || path.len() > 4096
            || !matches!(status, "A" | "D" | "M" | "T")
            || entries.iter().any(|entry: &Entry| entry.path == path)
            || path.starts_with('/')
            || path
                .split('/')
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
        {
            return Err(
                "Invalid or excessive commit file list (256 files / 4 KiB path). Nothing is shown."
                    .into(),
            );
        }
        entries.push(Entry {
            status: status.chars().next().unwrap(),
            path: path.into(),
        });
    }
    entries.shrink_to_fit();
    Ok(entries)
}
pub(crate) fn read(
    root: &Path,
    hash: &str,
    parent: Option<&str>,
    cancel: &AtomicBool,
) -> Result<Vec<Entry>, String> {
    if !oid(hash) || parent.is_some_and(|p| !oid(p) || p.len() != hash.len() || p == hash) {
        return Err("Invalid commit comparison identity.".into());
    }
    let (bytes, status) = read_only::run(
        &mut command(root, hash, parent),
        cancel,
        Instant::now() + DEADLINE,
        MAX_BYTES,
        "Commit file inspection",
        "Commit file list exceeds 128 KiB. Nothing is shown.",
    )?;
    if !status.success() {
        return Err("Local commit tree is unavailable. No network fetch was attempted.".into());
    }
    parse(&bytes)
}
#[cfg(test)]
mod tests;
