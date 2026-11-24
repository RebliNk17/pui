//! Explicit reviewed branch creation or clean switching; hooks bypassed.
use super::*;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Review {
    pub root: std::path::PathBuf,
    pub source: String,
    pub head: String,
    pub name: String,
    pub switch: bool,
    pub target: String,
    pub files: Vec<crate::workspace::history::files::Entry>,
}
pub(crate) fn valid_start(start: &str) -> bool {
    crate::workspace::history::oid(start)
}
pub(crate) fn valid_name(name: &str) -> bool {
    !name.starts_with('-') && valid_ref(&format!("refs/heads/{name}"))
}
pub(crate) fn inspect(root: &Path, name: &str, cancel: &AtomicBool) -> Result<Review, String> {
    inspect_action(root, name, false, cancel)
}
pub(crate) fn inspect_action(
    root: &Path,
    name: &str,
    switch: bool,
    cancel: &AtomicBool,
) -> Result<Review, String> {
    if !valid_name(name) {
        return Err("Invalid local branch name (up to 1 KiB full ref).".into());
    }
    let (source, head) =
        super::super::commit::branch_head(root, cancel, Instant::now() + DEADLINE)?;
    if head.bytes().all(|b| b == b'0') {
        return Err("Create an initial commit first.".into());
    }
    let snapshot = super::read(root, cancel)?;
    let exists = snapshot.local.iter().any(|n| n == name);
    if switch && !exists {
        return Err("Choose an existing local branch; remote guessing is disabled.".into());
    }
    if !switch && exists {
        return Err("Local branch already exists; nothing replaced.".into());
    }
    let target = if switch {
        let deadline = Instant::now() + DEADLINE;
        read_only::checkout_filters_disabled(root, cancel, deadline)?;
        // Refuse all local data, including untracked/ignored files: no implicit loss/merge.
        let (out, status) = read_only::run(
            read_only::command(root).args([
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--ignored",
                "--ignore-submodules=none",
            ]),
            cancel,
            deadline,
            128 * 1024,
            "Branch switch cleanliness",
            "Status exceeds limit",
        )?;
        if !status.success() || !out.is_empty() {
            return Err("Branch switching requires a clean index/worktree with no untracked or ignored files. Nothing switched.".into());
        }
        let (out, status) = read_only::run(
            read_only::command(root).args([
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "MERGE_HEAD",
                "--git-path",
                "CHERRY_PICK_HEAD",
                "--git-path",
                "REVERT_HEAD",
                "--git-path",
                "rebase-merge",
                "--git-path",
                "rebase-apply",
            ]),
            cancel,
            deadline,
            8193,
            "Git operation state",
            "State path exceeds limit",
        )?;
        let paths = std::str::from_utf8(&out)
            .map_err(|_| "Invalid operation path")?
            .lines()
            .collect::<Vec<_>>();
        if !status.success() || paths.len() != 5 || paths.iter().any(|p| Path::new(p).exists()) {
            return Err("Finish the active Git operation first.".into());
        }
        let reference = format!("refs/heads/{name}^{{commit}}");
        let (out, status) = read_only::run(
            read_only::command(root)
                .env("GIT_NO_REPLACE_OBJECTS", "1")
                .args(["rev-parse", "--verify", &reference]),
            cancel,
            deadline,
            128,
            "Target branch commit",
            "Invalid target commit",
        )?;
        let oid = std::str::from_utf8(&out)
            .map_err(|_| "Invalid target commit")?
            .trim_end_matches('\n');
        if !status.success() || !crate::workspace::history::oid(oid) {
            return Err("Target branch commit unavailable.".into());
        }
        oid.to_owned()
    } else {
        head.clone()
    };
    let (out, status) = read_only::run(
        read_only::command(root).args(["rev-parse", "--show-toplevel"]),
        cancel,
        Instant::now() + DEADLINE,
        8193,
        "Branch root",
        "Root exceeds limit",
    )?;
    if !status.success() || !root.is_absolute() || out != format!("{}\n", root.display()).as_bytes()
    {
        return Err("Exact repository root required.".into());
    }
    if switch {
        for oid in [&head, &target] {
            let (out, status) = read_only::run(
                read_only::command(root)
                    .env("GIT_NO_REPLACE_OBJECTS", "1")
                    .args(["ls-tree", "-r", "-z", oid, "--"]),
                cancel,
                Instant::now() + DEADLINE,
                128 * 1024,
                "Checkout tree inspection",
                "Checkout tree exceeds 128 KiB; switch refused",
            )?;
            if !status.success() {
                return Err("Cannot inspect checkout tree modes.".into());
            }
            if out
                .split(|b| *b == 0)
                .any(|entry| entry.starts_with(b"160000 "))
            {
                return Err("Branch switching with submodule entries is unsupported; no checkout performed.".into());
            }
        }
    }
    let files = if switch && target != head {
        crate::workspace::history::files::read(root, &target, Some(&head), cancel)?
    } else {
        Vec::new()
    };
    Ok(Review {
        root: root.into(),
        source,
        head,
        name: name.into(),
        switch,
        target,
        files,
    })
}
/// A supplied start must be a full immutable commit OID, never a moving revision expression.
pub(crate) fn inspect_start(
    root: &Path,
    name: &str,
    start: &str,
    cancel: &AtomicBool,
) -> Result<Review, String> {
    if !valid_start(start) {
        return Err(
            "Use a full 40- or 64-character commit ID, not a branch or revision expression.".into(),
        );
    }
    let mut review = inspect(root, name, cancel)?;
    if start.len() != review.head.len() {
        return Err("Commit ID object format differs from repository.".into());
    }
    let (out, status) = read_only::run(
        read_only::command(root)
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .args(["cat-file", "-t", start]),
        cancel,
        Instant::now() + DEADLINE,
        32,
        "Branch start commit",
        "Invalid object type",
    )?;
    if !status.success() || out != b"commit\n" {
        return Err(
            "Start commit is unavailable locally or is not a commit. No fetch performed.".into(),
        );
    }
    review.target = start.into();
    Ok(review)
}
pub(crate) fn apply(review: Review, cancel: &AtomicBool) -> Result<(), String> {
    let fresh = if review.switch {
        inspect_action(&review.root, &review.name, true, cancel)?
    } else {
        inspect_start(&review.root, &review.name, &review.target, cancel)?
    };
    if fresh != review {
        return Err("Source branch or commit changed; inspect again.".into());
    }
    if review.switch {
        let (_, status) = read_only::run(
            read_only::command(&review.root)
                .env("GIT_NO_REPLACE_OBJECTS", "1")
                .args([
                    "-c",
                    "core.hooksPath=/dev/null",
                    "switch",
                    "--no-guess",
                    "--no-overwrite-ignore",
                    &review.name,
                ]),
            cancel,
            Instant::now() + DEADLINE,
            1024,
            "Switch local branch",
            "Switch output exceeds limit; inspect repository state",
        )?;
        if !status.success() {
            return Err("Branch switch failed or was refused (possibly checked out elsewhere). Inspect repository state before retrying.".into());
        }
        let (source, head) =
            super::super::commit::branch_head(&review.root, cancel, Instant::now() + DEADLINE)?;
        if source != format!("refs/heads/{}", review.name) || head != review.target {
            return Err("Branch switch completed with unexpected state; inspect repository before continuing.".into());
        }
        return Ok(());
    }
    let destination = format!("refs/heads/{}", review.name);
    let zero = "0".repeat(review.head.len());
    let (_, status) = read_only::run(
        read_only::command(&review.root).args([
            "-c",
            "core.hooksPath=/dev/null",
            "update-ref",
            "-m",
            "Pui: create local branch",
            &destination,
            &review.target,
            &zero,
        ]),
        cancel,
        Instant::now() + DEADLINE,
        1024,
        "Create local branch",
        "Unexpected branch output",
    )?;
    if !status.success() {
        return Err("Cannot create branch; another writer may have created it. Inspect branches before retrying.".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests;
