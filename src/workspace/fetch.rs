//! Explicit exact-branch Fetch. No checkout/merge, no FETCH_HEAD, no force or pruning.
use super::*;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Review {
    pub root: PathBuf,
    pub remote: String,
    pub branch: String,
    pub endpoint: String,
    pub tracking: String,
    pub previous: Option<String>,
}
fn output(
    root: &Path,
    args: &[&str],
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<String, String> {
    let (bytes, status) = read_only::run(
        read_only::command(root).args(args),
        cancel,
        deadline,
        16384,
        "Fetch inspection",
        "Fetch configuration exceeds16KiB",
    )?;
    let text = String::from_utf8(bytes).map_err(|_| "Fetch configuration is not UTF8")?;
    if !status.success() {
        return Err("Fetch configuration unavailable.".into());
    }
    let line = text
        .strip_suffix('\n')
        .ok_or("Invalid fetch configuration framing")?;
    if line.is_empty() || line.chars().any(char::is_control) {
        return Err("Fetch requires one bounded value.".into());
    }
    Ok(line.into())
}
fn current(
    root: &Path,
    reference: &str,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Option<String>, String> {
    let (_, sym) = read_only::run(
        read_only::command(root).args(["symbolic-ref", "-q", reference]),
        cancel,
        deadline,
        8192,
        "Tracking ref kind",
        "Tracking ref exceeds limit",
    )?;
    if sym.success() || sym.code() != Some(1) {
        return Err("Symbolic or ambiguous tracking refs are unsupported.".into());
    }
    let (bytes, status) = read_only::run(
        read_only::command(root).args(["show-ref", "--verify", "--hash", reference]),
        cancel,
        deadline,
        128,
        "Tracking ref",
        "Tracking ref exceeds limit",
    )?;
    if !status.success() {
        // show-ref --verify returns128 for absence; distinguish invalid refs using --exists.
        let (out, exists) = read_only::run(
            read_only::command(root).args(["show-ref", "--exists", reference]),
            cancel,
            deadline,
            128,
            "Tracking ref existence",
            "Unexpected ref output",
        )?;
        if exists.code() == Some(2) && out.is_empty() {
            return Ok(None);
        }
        return Err("Tracking reference unavailable or invalid.".into());
    }
    let oid = std::str::from_utf8(&bytes)
        .map_err(|_| "Invalid tracking ref")?
        .trim_end_matches('\n');
    if !history::oid(oid) {
        return Err("Invalid tracking commit ID.".into());
    }
    Ok(Some(oid.into()))
}
pub(crate) fn inspect(
    root: &Path,
    remote: &str,
    branch: &str,
    cancel: &AtomicBool,
) -> Result<Review, String> {
    if !root.is_absolute()
        || !push::remote::valid_name(remote)
        || !branches::create::valid_name(branch)
    {
        return Err("Choose an exact root, simple remote name and valid branch.".into());
    }
    let deadline = Instant::now() + DEADLINE;
    if Path::new(&output(
        root,
        &["rev-parse", "--show-toplevel"],
        cancel,
        deadline,
    )?) != root
    {
        return Err("Exact repository root required.".into());
    }
    let endpoint = output(
        root,
        &["remote", "get-url", "--all", remote],
        cancel,
        deadline,
    )?;
    if !push::remote::valid_url(&endpoint) {
        return Err(
            "Fetch endpoint is ambiguous or unsupported; credentials must not be embedded.".into(),
        );
    }
    let tracking = format!("refs/remotes/{remote}/{branch}");
    let previous = current(root, &tracking, cancel, deadline)?;
    Ok(Review {
        root: root.into(),
        remote: remote.into(),
        branch: branch.into(),
        endpoint,
        tracking,
        previous,
    })
}
pub(crate) fn apply(review: Review, seconds: u64, cancel: &AtomicBool) -> Result<String, String> {
    if !(1..=300).contains(&seconds) {
        return Err("Fetch timeout must be1–300seconds.".into());
    }
    if inspect(&review.root, &review.remote, &review.branch, cancel)? != review {
        return Err("Fetch configuration or tracking ref changed; review again.".into());
    }
    let deadline = Instant::now() + std::time::Duration::from_secs(seconds);
    let source = format!("refs/heads/{}", review.branch);
    let (bytes, status) = read_only::run(
        push::network_command(&review.root).args([
            "ls-remote",
            "--refs",
            "--",
            &review.endpoint,
            &source,
        ]),
        cancel,
        deadline,
        1024,
        "Remote branch for Fetch",
        "Remote branch response exceeds limit",
    )?;
    if !status.success() {
        return Err("Remote inspection failed; details hidden. No retry.".into());
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| "Invalid remote branch response")?
        .strip_suffix('\n')
        .ok_or("Remote branch absent or invalid response")?;
    let (oid, name) = text
        .split_once('\t')
        .ok_or("Invalid remote branch response")?;
    if !history::oid(oid) || name != source {
        return Err("Remote branch response is ambiguous.".into());
    }
    // Download only the observed object into the object DB. Do not let fetch write any refs.
    let (_, status) = read_only::run(
        push::network_command(&review.root).args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "gc.auto=0",
            "-c",
            "maintenance.auto=false",
            "-c",
            "fetch.writeCommitGraph=false",
            "fetch",
            "--quiet",
            "--no-tags",
            "--no-recurse-submodules",
            "--no-write-fetch-head",
            "--no-auto-maintenance",
            "--",
            &review.endpoint,
            oid,
        ]),
        cancel,
        deadline,
        16384,
        "Fetch observed branch objects",
        "Fetch output exceeds limit",
    )?;
    if !status.success() {
        return Err("Fetch failed; objects may remain. No checkout or automatic retry.".into());
    }
    let kind = output(&review.root, &["cat-file", "-t", oid], cancel, deadline)?;
    if kind != "commit" {
        return Err("Fetched branch is not a commit; tracking ref unchanged.".into());
    }
    if let Some(old) = &review.previous
        && old != oid
    {
        let (_, ancestor) = read_only::run(
            read_only::command(&review.root)
                .env("GIT_NO_REPLACE_OBJECTS", "1")
                .args(["merge-base", "--is-ancestor", old, oid]),
            cancel,
            deadline,
            128,
            "Tracking fast-forward check",
            "Unexpected ancestry output",
        )?;
        if !ancestor.success() {
            return Err("Remote branch rewound/diverged or ancestry unavailable; no force update. Objects retained, tracking ref unchanged.".into());
        }
    }
    if inspect(&review.root, &review.remote, &review.branch, cancel)? != review {
        return Err(
            "Fetch target changed during download; objects retained, no tracking publication."
                .into(),
        );
    }
    let expected = review
        .previous
        .clone()
        .unwrap_or_else(|| "0".repeat(oid.len()));
    let (_, status) = read_only::run(
        read_only::command(&review.root).args([
            "-c",
            "core.hooksPath=/dev/null",
            "update-ref",
            "--no-deref",
            "-m",
            "Pui: fetch reviewed branch",
            &review.tracking,
            oid,
            &expected,
        ]),
        cancel,
        deadline,
        1024,
        "Fetch tracking publication",
        "Unexpected tracking update output",
    )?;
    if !status.success() {
        return Err("Tracking publication failed or completion uncertain; inspect before retrying. Objects may remain.".into());
    }
    Ok(oid.into())
}
#[cfg(test)]
mod tests;
