//! Local-only reviewed fast-forward. No hidden Fetch, merge commit, stash or conflict fallback.
use super::*;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Review {
    pub root: PathBuf,
    pub branch: String,
    pub head: String,
    pub tracking: String,
    pub target: String,
    pub files: Vec<history::files::Entry>,
}
fn run(root: &Path, args: &[&str], cancel: &AtomicBool, limit: usize) -> Result<Vec<u8>, String> {
    let (bytes, status) = read_only::run(
        read_only::command(root)
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .args(args),
        cancel,
        Instant::now() + DEADLINE,
        limit,
        "Fast-forward inspection",
        "Fast-forward inspection exceeds limit",
    )?;
    if !status.success() {
        return Err("Local fast-forward inspection unavailable.".into());
    }
    Ok(bytes)
}
fn line(bytes: Vec<u8>) -> Result<String, String> {
    let s = String::from_utf8(bytes).map_err(|_| "Invalid local Git metadata")?;
    let s = s
        .strip_suffix('\n')
        .ok_or("Invalid local Git metadata framing")?;
    if s.is_empty() || s.chars().any(char::is_control) {
        return Err("Invalid local Git metadata".into());
    }
    Ok(s.into())
}
pub(crate) fn inspect(
    root: &Path,
    remote: &str,
    name: &str,
    cancel: &AtomicBool,
) -> Result<Review, String> {
    if !push::remote::valid_name(remote) || !branches::create::valid_name(name) {
        return Err("Choose a simple remote name and exact branch name.".into());
    }
    let (branch, head) = commit::branch_head(root, cancel, Instant::now() + DEADLINE)?;
    let current = branch
        .strip_prefix("refs/heads/")
        .ok_or("Existing current local branch required")?;
    // Reuse the same clean/ignored/untracked/operation/filter/source-tree gates as switching.
    branches::create::inspect_action(root, current, true, cancel)?;
    let key = format!("branch.{current}.mergeOptions");
    let (out, status) = read_only::run(
        read_only::command(root).args(["config", "--includes", "--get-all", &key]),
        cancel,
        Instant::now() + DEADLINE,
        8192,
        "Merge option safety",
        "Merge option configuration exceeds limit",
    )?;
    if status.code() != Some(1) || !out.is_empty() {
        return Err("Custom branch merge options refuse this fast-forward path.".into());
    }
    let tracking = format!("refs/remotes/{remote}/{name}");
    let (_, symbolic) = read_only::run(
        read_only::command(root).args(["symbolic-ref", "-q", &tracking]),
        cancel,
        Instant::now() + DEADLINE,
        8192,
        "Tracking reference kind",
        "Tracking reference exceeds limit",
    )?;
    if symbolic.code() != Some(1) {
        return Err("Symbolic tracking references refuse; choose the fetched branch.".into());
    }
    let target = line(run(
        root,
        &["rev-parse", "--verify", &format!("{tracking}^{{commit}}")],
        cancel,
        128,
    )?)?;
    if !history::oid(&target) || target.len() != head.len() {
        return Err("Fetched commit unavailable; Fetch explicitly first.".into());
    }
    if target == head {
        return Err("Current branch already equals this fetched commit.".into());
    }
    let (_, ancestor) = read_only::run(
        read_only::command(root)
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .args(["merge-base", "--is-ancestor", &head, &target]),
        cancel,
        Instant::now() + DEADLINE,
        128,
        "Fast-forward ancestry",
        "Unexpected ancestry output",
    )?;
    if !ancestor.success() {
        return Err(
            "Local branch is ahead/diverged or ancestry unavailable. No merge or rebase performed."
                .into(),
        );
    }
    let tree = run(
        root,
        &["ls-tree", "-r", "-z", &target, "--"],
        cancel,
        128 * 1024,
    )?;
    if tree
        .split(|b| *b == 0)
        .any(|entry| entry.starts_with(b"160000 "))
    {
        return Err("Submodule target trees refuse automatic checkout.".into());
    }
    let files = history::files::read(root, &target, Some(&head), cancel)?;
    Ok(Review {
        root: root.into(),
        branch,
        head,
        tracking,
        target,
        files,
    })
}
pub(crate) fn apply(
    review: Review,
    remote: &str,
    name: &str,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if inspect(&review.root, remote, name, cancel)? != review {
        return Err("Branch, fetched commit or checkout state changed; review again.".into());
    }
    let (_, status) = read_only::run(
        read_only::command(&review.root)
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "submodule.recurse=false",
                "merge",
                "--ff-only",
                "--no-edit",
                "--no-stat",
                "--no-progress",
                "--no-autostash",
                "--no-verify-signatures",
                "--",
                &review.target,
            ]),
        cancel,
        Instant::now() + DEADLINE,
        16384,
        "Fast-forward branch",
        "Fast-forward output exceeds limit; inspect checkout state",
    )?;
    if !status.success() {
        return Err("Fast-forward failed or completion uncertain. Inspect branch/files before retrying; no automatic retry or rollback.".into());
    }
    // Successful porcelain is success. Later observation failure must not invite duplicate execution.
    Ok(())
}
#[cfg(test)]
mod tests;
