//! Create one named remote via private config and cooperative config.lock publication.
use super::*;
use std::{
    fs,
    io::{Read, Write},
};
fn config_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "Local Git config unavailable")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 128 * 1024 {
        return Err("Requires regular local Git config <=128 KiB.".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "Cannot read config")?
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read config")?;
    if bytes.len() > 128 * 1024 {
        return Err("Config exceeds limit".into());
    }
    Ok(bytes)
}
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        && !name.starts_with('-')
}
pub(crate) fn valid_url(url: &str) -> bool {
    !url.is_empty()
        && url.len() <= 8192
        && !url.chars().any(char::is_control)
        && endpoint(url).is_ok()
}
pub(crate) fn create(
    root: &Path,
    name: &str,
    url: &str,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if !valid_name(name) || !valid_url(url) {
        return Err(
            "Use a simple remote name (letters/numbers/-/_) and a supported credential-free URL."
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
    let names = run(root, &["remote"], cancel, deadline)?;
    if std::str::from_utf8(&names)
        .map_err(|_| "Invalid remote names")?
        .lines()
        .any(|n| n == name)
    {
        return Err("Remote name already exists; nothing replaced.".into());
    }
    update(
        root,
        &[
            (format!("remote.{name}.url"), url.to_owned()),
            (
                format!("remote.{name}.fetch"),
                format!("+refs/heads/*:refs/remotes/{name}/*"),
            ),
        ],
        cancel,
        deadline,
        || Ok(()),
    )
}
pub(in crate::workspace) fn update(
    root: &Path,
    values: &[(String, String)],
    cancel: &AtomicBool,
    deadline: Instant,
    revalidate: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let path = PathBuf::from(line(run(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        cancel,
        deadline,
    )?)?)
    .join("config");
    let original = config_bytes(&path)?;
    let lock_path = path.with_file_name("config.lock");
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut lock = options
        .open(&lock_path)
        .map_err(|_| "Git config locked; nothing changed.")?;
    struct Owned(PathBuf);
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let _owned = Owned(lock_path.clone());
    if config_bytes(&path)? != original {
        return Err("Config changed; inspect again.".into());
    }
    let temp = path.with_file_name(format!("pui-remote-config-{}", uuid::Uuid::new_v4()));
    let mut file = options
        .open(&temp)
        .map_err(|_| "Cannot prepare private config")?;
    let _temp = Owned(temp.clone());
    file.write_all(&original)
        .map_err(|_| "Cannot copy config")?;
    drop(file);
    for (key, value) in values {
        let mut cmd = read_only::command(root);
        cmd.args(["config", "--no-includes", "--file"])
            .arg(&temp)
            .args(["--add", key, value]);
        let (_, status) = read_only::run(
            &mut cmd,
            cancel,
            deadline,
            1024,
            "Private remote config",
            "Unexpected config output",
        )?;
        if !status.success() {
            return Err("Cannot prepare remote configuration.".into());
        }
    }
    let prepared = config_bytes(&temp)?;
    revalidate()?;
    if config_bytes(&path)? != original
        || cancel.load(std::sync::atomic::Ordering::Acquire)
        || Instant::now() >= deadline
    {
        return Err("Config changed or creation cancelled; nothing published.".into());
    }
    lock.write_all(&prepared)
        .and_then(|_| lock.sync_all())
        .map_err(|_| "Cannot write config lock")?;
    fs::set_permissions(
        &lock_path,
        fs::metadata(&path)
            .map_err(|_| "Cannot inspect config mode")?
            .permissions(),
    )
    .map_err(|_| "Cannot preserve config mode")?;
    drop(lock);
    fs::rename(&lock_path, &path).map_err(|_| "Cannot publish remote config")?;
    Ok(())
}
pub(crate) fn set_upstream(snapshot: &Snapshot, cancel: &AtomicBool) -> Result<(), String> {
    let target = snapshot
        .explicit_target
        .then_some((snapshot.remote.as_str(), snapshot.destination.as_str()));
    let validate = || {
        if prepare_target(&snapshot.root, target, cancel)? != *snapshot {
            return Err("Reviewed destination changed; inspect again.".into());
        }
        Ok(())
    };
    validate()?;
    let deadline = Instant::now() + DEADLINE;
    let branch = snapshot
        .branch
        .strip_prefix("refs/heads/")
        .ok_or("Local branch required")?;
    let keys = [
        format!("branch.{branch}.remote"),
        format!("branch.{branch}.merge"),
    ];
    for key in &keys {
        let (out, status) = read_only::run(
            read_only::command(&snapshot.root).args(["config", "--get-all", key]),
            cancel,
            deadline,
            16384,
            "Upstream configuration",
            "Upstream exceeds limit",
        )?;
        if status.code() != Some(1) || !out.is_empty() {
            return Err(
                "An upstream already exists or configuration is ambiguous; nothing replaced."
                    .into(),
            );
        }
    }
    // Reject worktree-specific configuration: the common file may not be authoritative.
    let (out, status) = read_only::run(
        read_only::command(&snapshot.root).args(["config", "--get", "extensions.worktreeConfig"]),
        cancel,
        deadline,
        128,
        "Worktree config",
        "Invalid config",
    )?;
    if status.code() != Some(1) || !out.is_empty() {
        return Err("Worktree-specific configuration is unsupported for upstream creation.".into());
    }
    update(
        &snapshot.root,
        &[
            (keys[0].clone(), snapshot.remote.clone()),
            (keys[1].clone(), snapshot.destination.clone()),
        ],
        cancel,
        deadline,
        validate,
    )
}
#[cfg(test)]
mod tests;
