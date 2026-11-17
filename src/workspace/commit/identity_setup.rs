//! Create repository-local identity only; never change global config or credentials.
use super::*;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Review {
    pub root: PathBuf,
    pub name: String,
    pub email: String,
    pub inherited: Vec<(String, String)>,
    pub override_inherited: bool,
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
fn absent(
    root: &Path,
    cancel: &AtomicBool,
    deadline: Instant,
    allow: bool,
) -> Result<Vec<(String, String)>, String> {
    let mut inherited = Vec::new();
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
        if status.code() == Some(1) && out.is_empty() {
            continue;
        }
        if !allow || !matches!(key, "user.name" | "user.email") || !status.success() {
            return Err("Identity settings already exist or worktree config is enabled. Existing identity is never replaced.".into());
        }
        let (local, local_status) = read_only::run(
            read_only::command(root).args(["config", "--local", "--no-includes", "--get-all", key]),
            cancel,
            deadline,
            16384,
            "Local identity",
            "Local identity exceeds limit",
        )?;
        if local_status.code() != Some(1) || !local.is_empty() {
            return Err("Repository-local identity already exists; nothing replaced.".into());
        }
        let value = String::from_utf8(out).map_err(|_| "Inherited identity is not UTF-8")?;
        let value = value
            .strip_suffix('\n')
            .ok_or("Invalid inherited identity")?;
        if value.len() > 512 || value.chars().any(char::is_control) {
            return Err("Inherited identity is ambiguous or oversized.".into());
        }
        inherited.push((key.into(), value.into()));
    }
    Ok(inherited)
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
    inspect_override(root, name, email, false, cancel)
}
pub(crate) fn inspect_override(
    root: &Path,
    name: &str,
    email: &str,
    allow: bool,
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
    let inherited = absent(root, cancel, deadline, allow)?;
    Ok(Review {
        root: root.into(),
        inherited,
        override_inherited: allow,
        name: name.trim().into(),
        email: email.into(),
        config: config(root, cancel, deadline)?,
    })
}
pub(crate) fn apply(review: Review, cancel: &AtomicBool) -> Result<(), String> {
    let validate = || {
        if inspect_override(
            &review.root,
            &review.name,
            &review.email,
            review.override_inherited,
            cancel,
        )? != review
        {
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
