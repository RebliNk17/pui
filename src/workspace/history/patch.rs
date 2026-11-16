//! Bounded literal patch between local commit trees; no working-tree conversion.
use super::*;
const MAX_BYTES: usize = 32 * 1024;
const MAX_LINES: usize = 1024;
fn command(root: &Path, hash: &str, parent: Option<&str>) -> Command {
    let mut cmd = super::command(root);
    cmd.args([
        "diff-tree",
        "--no-commit-id",
        "--no-color",
        "-p",
        "-r",
        "--no-renames",
        "--no-ext-diff",
        "--no-textconv",
        "--submodule=short",
        "--ignore-submodules=none",
        "--unified=3",
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
fn parse(bytes: Vec<u8>) -> Result<String, String> {
    if bytes.len() > MAX_BYTES {
        return Err("Commit patch exceeds 32 KiB. Nothing is shown.".into());
    }
    let text =
        String::from_utf8(bytes).map_err(|_| "Commit patch is not UTF-8. Nothing is shown.")?;
    if text.lines().count() > MAX_LINES
        || text
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err(
            "Commit patch exceeds 1,024 lines or contains unsupported controls. Nothing is shown."
                .into(),
        );
    }
    Ok(text)
}
pub(crate) fn read(
    root: &Path,
    hash: &str,
    parent: Option<&str>,
    cancel: &AtomicBool,
) -> Result<String, String> {
    read_path(root, hash, parent, None, cancel)
}
pub(crate) fn read_path(
    root: &Path,
    hash: &str,
    parent: Option<&str>,
    path: Option<&str>,
    cancel: &AtomicBool,
) -> Result<String, String> {
    if path.is_some_and(|p| {
        p.is_empty()
            || p.len() > 4096
            || p.starts_with('/')
            || p.contains('\0')
            || p.split('/')
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
    }) {
        return Err("Invalid literal commit path.".into());
    }
    if !oid(hash) || parent.is_some_and(|p| !oid(p) || p.len() != hash.len() || p == hash) {
        return Err("Invalid commit comparison identity.".into());
    }
    let mut cmd = command(root, hash, parent);
    if let Some(path) = path {
        cmd.env("GIT_LITERAL_PATHSPECS", "1")
            .env_remove("GIT_GLOB_PATHSPECS")
            .env_remove("GIT_NOGLOB_PATHSPECS")
            .env_remove("GIT_ICASE_PATHSPECS")
            .arg(path);
    }
    let (bytes, status) = read_only::run(
        &mut cmd,
        cancel,
        Instant::now() + DEADLINE,
        MAX_BYTES,
        "Commit patch inspection",
        "Commit patch exceeds 32 KiB. Nothing is shown.",
    )?;
    if !status.success() {
        return Err("Local commit patch is unavailable. No network fetch was attempted.".into());
    }
    parse(bytes)
}
#[cfg(test)]
mod tests;
