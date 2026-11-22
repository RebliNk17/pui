//! Create repository-local identity only; never change global config or credentials.
use super::*;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Review {
    pub root: PathBuf,
    pub name: String,
    pub email: String,
    pub inherited: Vec<(String, String)>,
    pub override_inherited: bool,
    pub edit_local: bool,
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
    edit: bool,
) -> Result<Vec<(String, String)>, String> {
    // One bounded snapshot instead of seven Git launches under the same deadline.
    // NUL records preserve duplicate keys and embedded newlines for validation.
    let values = identity_config(root, cancel, deadline, false)?;
    if !(values.is_empty() || allow || edit)
        || values
            .keys()
            .any(|key| !matches!(key.as_str(), "user.name" | "user.email"))
    {
        return Err("Identity settings already exist or worktree config is enabled. Existing identity is never replaced.".into());
    }
    let local = if values.is_empty() {
        Default::default()
    } else {
        identity_config(root, cancel, deadline, true)?
    };
    let mut inherited = Vec::new();
    for key in ["user.name", "user.email"] {
        let Some(entries) = values.get(key) else {
            continue;
        };
        if edit {
            if local.get(key) != Some(entries) {
                return Err(
                    "Editing requires one direct local value without inherited ambiguity.".into(),
                );
            }
        } else if local.contains_key(key) {
            return Err("Repository-local identity already exists; nothing replaced.".into());
        }
        if entries.len() != 1 || entries[0].len() > 512 || entries[0].chars().any(char::is_control)
        {
            return Err("Inherited identity is ambiguous or oversized.".into());
        }
        inherited.push((key.into(), entries[0].clone()));
    }
    if edit && inherited.len() != 2 {
        return Err("Editing requires both existing local name and email.".into());
    }
    Ok(inherited)
}
fn identity_config(
    root: &Path,
    cancel: &AtomicBool,
    deadline: Instant,
    local: bool,
) -> Result<std::collections::BTreeMap<String, Vec<String>>, String> {
    let mut command = read_only::command(root);
    command.arg("config").arg("--null");
    if local {
        command.args(["--local", "--no-includes"]);
    } else {
        command.arg("--includes");
    }
    command.args([
        "--get-regexp",
        r"^((user|author|committer)\.(name|email)|extensions\.worktreeconfig)$",
    ]);
    let (out, status) = read_only::run(
        &mut command,
        cancel,
        deadline,
        16384,
        "Identity configuration",
        "Identity configuration exceeds limit",
    )?;
    if status.code() == Some(1) && out.is_empty() {
        return Ok(Default::default());
    }
    if !status.success() || !out.ends_with(&[0]) {
        return Err("Identity configuration unavailable or malformed.".into());
    }
    let mut values = std::collections::BTreeMap::<String, Vec<String>>::new();
    for record in out[..out.len() - 1].split(|byte| *byte == 0) {
        let record = std::str::from_utf8(record).map_err(|_| "Inherited identity is not UTF-8")?;
        let (key, value) = record
            .split_once('\n')
            .ok_or("Invalid inherited identity")?;
        values.entry(key.into()).or_default().push(value.into());
    }
    Ok(values)
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
    inspect_mode(root, name, email, allow, false, cancel)
}
pub(crate) fn inspect_mode(
    root: &Path,
    name: &str,
    email: &str,
    allow: bool,
    edit: bool,
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
    let inherited = absent(root, cancel, deadline, allow, edit)?;
    Ok(Review {
        root: root.into(),
        inherited,
        override_inherited: allow,
        edit_local: edit,
        name: name.trim().into(),
        email: email.into(),
        config: config(root, cancel, deadline)?,
    })
}
/// Read-only form population; this result is not an authorization to publish.
pub(crate) fn load_local(root: &Path, cancel: &AtomicBool) -> Result<(String, String), String> {
    let review = inspect_mode(
        root,
        "Read only",
        "read-only@example.invalid",
        false,
        true,
        cancel,
    )?;
    Ok((review.inherited[0].1.clone(), review.inherited[1].1.clone()))
}
pub(crate) fn apply(review: Review, cancel: &AtomicBool) -> Result<(), String> {
    let validate = || {
        if inspect_mode(
            &review.root,
            &review.name,
            &review.email,
            review.override_inherited,
            review.edit_local,
            cancel,
        )? != review
        {
            return Err("Identity configuration changed; review again.".into());
        }
        Ok(())
    };
    validate()?;
    super::super::push::remote::update_mode(
        &review.root,
        &[
            ("user.name".into(), review.name.clone()),
            ("user.email".into(), review.email.clone()),
        ],
        cancel,
        Instant::now() + DEADLINE,
        review.edit_local,
        validate,
    )
}
#[cfg(test)]
pub(crate) mod tests;
