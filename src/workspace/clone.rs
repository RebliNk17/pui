//! Explicit reviewed clone into an empty destination; conservative network policy.
use super::*;
use std::{fs, sync::atomic::Ordering};
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Review {
    pub parent: PathBuf,
    pub name: String,
    pub url: String,
    pub branch: Option<String>,
}
pub(crate) fn valid_dest_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && !name.starts_with('-')
        && !name.contains('\0')
        && !name.chars().any(char::is_control)
}
pub(crate) fn inspect(
    parent: &Path,
    name: &str,
    url: &str,
    branch: Option<&str>,
    cancel: &AtomicBool,
) -> Result<Review, String> {
    if cancel.load(Ordering::Acquire) {
        return Err("Clone cancelled before review.".into());
    }
    if !super::push::remote::valid_url(url) {
        return Err(
            "Use a supported credential-free repository URL (HTTPS/SSH or git@host:path).".into(),
        );
    }
    if !valid_dest_name(name) {
        return Err(
            "Choose a destination folder name without slashes, controls or path separators.".into(),
        );
    }
    if let Some(b) = branch {
        if !super::branches::create::valid_name(b) {
            return Err(
                "Choose a valid branch to start from, or leave it blank for the remote default."
                    .into(),
            );
        }
    }
    if !parent.is_absolute()
        || !matches!(fs::symlink_metadata(parent.join(name)),Err(ref e)if e.kind()==std::io::ErrorKind::NotFound)
    {
        return Err(
            "Choose an existing canonical parent folder with an absent destination.".into(),
        );
    }
    let canonical = fs::canonicalize(parent).map_err(|_| "Parent folder unavailable")?;
    if canonical != parent || !parent.is_dir() {
        return Err("Parent folder must be an existing canonical directory.".into());
    }
    Ok(Review {
        parent: parent.into(),
        name: name.into(),
        url: url.into(),
        branch: branch.map(str::to_owned),
    })
}
pub(crate) fn apply(review: Review, timeout: u64, cancel: &AtomicBool) -> Result<(), String> {
    if !(1..=300).contains(&timeout) {
        return Err("Clone timeout must be 1–300 seconds.".into());
    }
    if inspect(
        &review.parent,
        &review.name,
        &review.url,
        review.branch.as_deref(),
        cancel,
    )? != review
    {
        return Err("Clone destination changed; inspect again.".into());
    }
    read_only::checkout_filters_disabled(&review.parent, cancel, Instant::now() + DEADLINE)?;
    // Reserve ownership atomically. Never clone into a pre-existing empty folder.
    fs::create_dir(review.parent.join(&review.name))
        .map_err(|_| "Destination was created or unavailable; nothing reused")?;
    let mut command = super::push::network_command(&review.parent);
    command.args([
        "-c",
        "core.hooksPath=/dev/null",
        "clone",
        "--quiet",
        "--no-local",
        "--no-recurse-submodules",
        "--template=",
    ]);
    if let Some(b) = &review.branch {
        command.args(["--branch", b]);
    }
    command
        .args(["--", &review.url])
        .arg(review.parent.join(&review.name));
    let result = read_only::run(
        &mut command,
        cancel,
        Instant::now() + std::time::Duration::from_secs(timeout),
        16 * 1024,
        "Clone repository",
        "Clone output exceeded limit; destination completion is uncertain.",
    );
    match result{Ok((_,status))if status.success()=>Ok(()),_=>Err("Clone was cancelled, timed out or failed. Remote completion or destination may be uncertain; inspect the destination before retrying. No automatic retry. Credential-free, no hooks, no force.".into())}
}
#[cfg(test)]
mod tests;
