//! Create repository-local identity only; never change global config or credentials.
use super::*;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Review {
    pub root: PathBuf,
    pub name: String,
    pub email: String,
    config: Vec<u8>,
}
pub(crate) fn valid(name: &str, email: &str) -> bool {
    !name.trim().is_empty()
        && name.len() <= 512
        && !email.is_empty()
        && email.len() <= 512
        && email.contains('@')
        && ![name, email]
            .iter()
            .any(|s| s.chars().any(char::is_control) || s.contains(['<', '>']))
        && !email.chars().any(char::is_whitespace)
}
fn absent(root: &Path, cancel: &AtomicBool, deadline: Instant) -> Result<(), String> {
    for key in [
        "user.name",
        "user.email",
        "author.name",
        "author.email",
        "committer.name",
        "committer.email",
        "extensions.worktreeConfig",
    ] {
        let (out, status) = read_only::run(
            read_only::command(root).args(["config", "--includes", "--get-all", key]),
            cancel,
            deadline,
            16384,
            "Identity configuration",
            "Identity configuration exceeds limit",
        )?;
        if status.code() != Some(1) || !out.is_empty() {
            return Err("Identity settings already exist or worktree config is enabled. Existing identity is never replaced.".into());
        }
    }
    Ok(())
}
fn config(root: &Path, cancel: &AtomicBool, deadline: Instant) -> Result<Vec<u8>, String> {
    let common = line(run(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        cancel,
        deadline,
    )?)?;
    let path = Path::new(&common).join("config");
    let m = fs::symlink_metadata(&path).map_err(|_| "Config unavailable")?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() > 128 * 1024 {
        return Err("Requires regular local config <=128 KiB.".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "Config unavailable")?
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Config read failed")?;
    if bytes.len() > 128 * 1024 {
        return Err("Config exceeds limit".into());
    }
    Ok(bytes)
}
pub(crate) fn inspect(
    root: &Path,
    name: &str,
    email: &str,
    cancel: &AtomicBool,
) -> Result<Review, String> {
    if !valid(name, email) {
        return Err(
            "Use a name and email up to 512 UTF-8 bytes, without controls or angle brackets."
                .into(),
        );
    }
    let deadline = Instant::now() + DEADLINE;
    if !root.is_absolute()
        || Path::new(&line(run(
            root,
            &["rev-parse", "--show-toplevel"],
            cancel,
            deadline,
        )?)?)
            != root
    {
        return Err("Exact repository root required.".into());
    }
    absent(root, cancel, deadline)?;
    Ok(Review {
        root: root.into(),
        name: name.trim().into(),
        email: email.into(),
        config: config(root, cancel, deadline)?,
    })
}
pub(crate) fn apply(review: Review, cancel: &AtomicBool) -> Result<(), String> {
    let validate = || {
        if inspect(&review.root, &review.name, &review.email, cancel)? != review {
            return Err("Identity configuration changed; review again.".into());
        }
        Ok(())
    };
    validate()?;
    super::super::push::remote::update(
        &review.root,
        &[
            ("user.name".into(), review.name.clone()),
            ("user.email".into(), review.email.clone()),
        ],
        cancel,
        Instant::now() + DEADLINE,
        validate,
    )
}
#[cfg(test)]
pub(crate) mod tests;
