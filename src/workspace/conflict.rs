//! Explicit bounded index conflict sides. Object-only, never resolves/stages/writes.
use super::*;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Side {
    pub mode: String,
    pub oid: String,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Review {
    pub root: PathBuf,
    pub path: String,
    pub sides: [Option<Side>; 3],
}
fn run(
    root: &Path,
    args: &[&str],
    cancel: &AtomicBool,
    deadline: Instant,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let (bytes, status) = read_only::run(
        read_only::command(root)
            .env("GIT_LITERAL_PATHSPECS", "1")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .args(args),
        cancel,
        deadline,
        limit,
        "Conflict inspection",
        "Conflict inspection exceeds limit",
    )?;
    if !status.success() {
        return Err("Conflict data unavailable locally; no fetch attempted.".into());
    }
    Ok(bytes)
}
fn entries(bytes: &[u8], path: &str) -> Result<[Option<(String, String)>; 3], String> {
    let mut result = [None, None, None];
    if bytes.is_empty() || bytes.last() != Some(&0) {
        return Err("No conflict entries or invalid index framing.".into());
    }
    for entry in bytes[..bytes.len() - 1].split(|b| *b == 0) {
        let tab = entry
            .iter()
            .position(|b| *b == b'\t')
            .ok_or("Invalid conflict entry")?;
        if &entry[tab + 1..] != path.as_bytes() {
            return Err("Conflict path changed or ambiguous.".into());
        }
        let header = std::str::from_utf8(&entry[..tab]).map_err(|_| "Invalid index metadata")?;
        let fields = header.split(' ').collect::<Vec<_>>();
        if fields.len() != 3
            || !matches!(fields[0], "100644" | "100755")
            || !history::oid(fields[1])
        {
            return Err("Conflict preview supports ordinary text blobs only; symlink/submodule/type conflicts refuse.".into());
        }
        let index = match fields[2] {
            "1" => 0,
            "2" => 1,
            "3" => 2,
            _ => return Err("Invalid conflict stage".into()),
        };
        if result[index].is_some() {
            return Err("Duplicate conflict stage".into());
        }
        result[index] = Some((fields[0].into(), fields[1].into()));
    }
    Ok(result)
}
pub(crate) fn inspect(root: &Path, path: &str, cancel: &AtomicBool) -> Result<Review, String> {
    inspect_at(root, path, cancel, Instant::now() + DEADLINE, false)
}
pub(super) fn inspect_at(
    root: &Path,
    path: &str,
    cancel: &AtomicBool,
    deadline: Instant,
    verified_outer: bool,
) -> Result<Review, String> {
    if !root.is_absolute()
        || path.is_empty()
        || path.len() > 4096
        || path.starts_with('/')
        || path.contains('\0')
        || path
            .split('/')
            .any(|s| s.is_empty() || matches!(s, "." | ".."))
    {
        return Err("Invalid literal conflict path.".into());
    }
    if !verified_outer
        && run(
            root,
            &["rev-parse", "--show-toplevel"],
            cancel,
            deadline,
            8193,
        )? != format!("{}\n", root.display()).as_bytes()
    {
        return Err("Exact repository root required.".into());
    }
    let args = ["ls-files", "--unmerged", "-z", "--", path];
    let raw = run(root, &args, cancel, deadline, 16384)?;
    let entries = entries(&raw, path)?;
    let mut sides = [None, None, None];
    let mut total = 0;
    for (index, entry) in entries.into_iter().enumerate() {
        if let Some((mode, oid)) = entry {
            let bytes = run(
                root,
                &["cat-file", "blob", &oid],
                cancel,
                deadline,
                64 * 1024,
            )?;
            total += bytes.len();
            if total > 128 * 1024 {
                return Err("Conflict sides exceed128KiB combined; nothing shown.".into());
            }
            let text = String::from_utf8(bytes)
                .map_err(|_| "Conflict side is binary/non-UTF8; text preview unavailable")?;
            if text.lines().count() > 2048
                || text
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            {
                return Err("Conflict side exceeds2048lines or has unsupported controls.".into());
            }
            sides[index] = Some(Side { mode, oid, text });
        }
    }
    if !verified_outer && run(root, &args, cancel, deadline, 16384)? != raw {
        return Err("Conflict stages changed during inspection; review again.".into());
    }
    Ok(Review {
        root: root.into(),
        path: path.into(),
        sides,
    })
}
#[cfg(test)]
mod tests;
