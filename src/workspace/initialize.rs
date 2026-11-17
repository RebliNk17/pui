//! Explicit repository initialization only; never add files, commit or contact remotes.
use super::*;
use std::fs;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Review {
    pub root: PathBuf,
    pub branch: String,
}
pub(crate) fn inspect(root: &Path, branch: &str, cancel: &AtomicBool) -> Result<Review, String> {
    if !root.is_absolute()
        || fs::canonicalize(root).map_err(|_| "Project folder unavailable")? != root
        || !root.is_dir()
    {
        return Err("Initialization requires an existing canonical project folder.".into());
    }
    if !super::branches::create::valid_name(branch) {
        return Err("Choose a valid initial branch name.".into());
    }
    match fs::symlink_metadata(root.join(".git")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err("A .git entry already exists; nothing initialized.".into()),
    }
    let (out, status) = read_only::run(
        read_only::command(root).args(["rev-parse", "--absolute-git-dir"]),
        cancel,
        Instant::now() + DEADLINE,
        8193,
        "Existing repository inspection",
        "Repository path exceeds limit",
    )?;
    if status.success() || !out.is_empty() || status.code() != Some(128) {
        return Err("Folder already belongs to a Git repository, or repository discovery is unavailable. Nothing initialized.".into());
    }
    Ok(Review {
        root: root.into(),
        branch: branch.into(),
    })
}
pub(crate) fn apply(review: Review, cancel: &AtomicBool) -> Result<(), String> {
    if inspect(&review.root, &review.branch, cancel)? != review {
        return Err("Initialization folder changed; inspect again.".into());
    }
    // Atomic ownership reservation; never reinitialize an existing or concurrently created .git.
    fs::create_dir(review.root.join(".git"))
        .map_err(|_| "Cannot reserve .git; nothing reinitialized.")?;
    // After reservation never delete: interrupted Git initialization may leave useful metadata.
    let mut command = read_only::command(&review.root);
    command
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "init",
            "--quiet",
            "--template=",
            "--initial-branch",
            &review.branch,
            "--",
        ])
        .arg(&review.root);
    let result = read_only::run(
        &mut command,
        cancel,
        Instant::now() + DEADLINE,
        1024,
        "Initialize repository",
        "Initialization output exceeds limit",
    );
    match result{Ok((_,status))if status.success()=>Ok(()),_=>Err("Initialization failed, was cancelled or timed out. A partial .git directory may remain; inspect it before retrying. Project files were not staged or committed.".into())}
}
#[cfg(test)]
mod tests;
