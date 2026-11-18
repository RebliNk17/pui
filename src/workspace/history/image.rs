//! Bounded before/after image extraction for a commit path; decode in the app.
use super::*;
const MAX_BLOB: usize = 16 * 1024 * 1024;
pub(crate) fn is_image(path: &str) -> bool {
    matches!(
        path.rsplit('.')
            .next()
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp")
    )
}
fn blob(root: &Path, treeish: &str, path: &str, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    let mut command = read_only::command(root);
    command
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .args(["cat-file", "blob"])
        .arg(format!("{treeish}:{path}"));
    let (bytes, status) = read_only::run(
        &mut command,
        cancel,
        Instant::now() + DEADLINE,
        MAX_BLOB,
        "Image blob read",
        "Image blob exceeds 16 MiB; preview refused",
    )?;
    if !status.success() {
        return Err(
            "Image blob is unavailable in this commit tree. No fetch was attempted.".into(),
        );
    }
    if bytes.is_empty() || bytes.len() > MAX_BLOB {
        return Err("Image blob is empty or oversized; nothing shown.".into());
    }
    Ok(bytes)
}
pub(crate) struct ImageDiff {
    pub before: Vec<u8>,
    pub after: Vec<u8>,
}
pub(crate) fn read_image_diff(
    root: &Path,
    hash: &str,
    parent: Option<&str>,
    path: &str,
    cancel: &AtomicBool,
) -> Result<ImageDiff, String> {
    if !oid(hash)
        || parent.is_some_and(|p| !oid(p) || p.len() != hash.len() || p == hash)
        || !is_image(path)
        || path.is_empty()
        || path.len() > 4096
        || path.starts_with('/')
        || path.contains('\0')
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err("Invalid literal image path or identity.".into());
    }
    let before = match parent {
        Some(p) => blob(root, p, path, cancel)?,
        None => Vec::new(),
    };
    let after = blob(root, hash, path, cancel)?;
    Ok(ImageDiff { before, after })
}
#[cfg(test)]
mod tests;
