//! Local-only upstream review. No remote contact, credential lookup, fetch or push.
use super::*;
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub root: PathBuf,
    pub branch: String,
    pub head: String,
    pub remote: String,
    pub destination: String,
    pub endpoint: String,
}
fn run(
    root: &Path,
    args: &[&str],
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<u8>, String> {
    let (out, status) = read_only::run(
        read_only::command(root).args(args),
        cancel,
        deadline,
        16384,
        "Push destination inspection",
        "Push configuration exceeds 16 KiB.",
    )?;
    if !status.success() {
        return Err(
            "Push destination unavailable. Configure one explicit upstream remote first.".into(),
        );
    }
    Ok(out)
}
fn line(bytes: Vec<u8>) -> Result<String, String> {
    let text = String::from_utf8(bytes).map_err(|_| "Push configuration is not UTF-8")?;
    let text = text.strip_suffix('\n').unwrap_or(&text);
    if text.is_empty() || text.len() > 8192 || text.chars().any(char::is_control) {
        return Err("Push configuration must contain one bounded value.".into());
    }
    Ok(text.into())
}
fn endpoint(url: &str) -> Result<String, String> {
    // Do not echo secrets or arbitrary transport-helper strings into review UI.
    if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("ssh://"))
    {
        let authority = rest.split('/').next().unwrap_or("");
        if authority.is_empty() || authority.contains(['@', '?', '#']) || rest.contains(['?', '#'])
        {
            return Err("Embedded credentials/query fragments in push URLs are unsupported; no URL is shown.".into());
        }
        return Ok(url.into());
    }
    if !url.contains("://")
        && let Some((host, path)) = url.split_once(':')
    {
        let host = host.strip_prefix("git@").unwrap_or(host);
        if !host.is_empty()
            && !host.contains(['@', '/', '\\', ' '])
            && !path.is_empty()
            && !path.starts_with(':')
            && !path.contains(['?', '#'])
        {
            return Ok(url.into());
        }
    }
    Err("Push review supports HTTPS or SSH remotes without embedded credentials (git@host:path is allowed).".into())
}
pub(crate) fn prepare(root: &Path, cancel: &AtomicBool) -> Result<Snapshot, String> {
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
    let (branch, head) = commit::branch_head(root, cancel, deadline)?;
    if branch.is_empty() || head.bytes().all(|b| b == b'0') {
        return Err("Push requires an existing local branch commit.".into());
    }
    let bytes = run(
        root,
        &[
            "for-each-ref",
            "--format=%(upstream:remotename)%00%(upstream:remoteref)",
            &branch,
        ],
        cancel,
        deadline,
    )?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| "Invalid upstream configuration")?
        .strip_suffix('\n')
        .ok_or("Invalid upstream framing")?;
    let (remote, destination) = text.split_once('\0').ok_or("Invalid upstream framing")?;
    if remote.is_empty()
        || remote.len() > 256
        || remote.starts_with('-')
        || remote.chars().any(|c| c.is_control() || c.is_whitespace())
        || remote == "."
        || !destination.starts_with("refs/heads/")
        || destination.len() > 1024
        || destination.chars().any(char::is_control)
    {
        return Err("Configure an upstream local branch on one named remote first.".into());
    }
    // Refuse push-specific routing, mirror and custom refspecs rather than silently guessing.
    for key in [
        format!(
            "branch.{}.pushRemote",
            branch.trim_start_matches("refs/heads/")
        ),
        "remote.pushDefault".into(),
        format!("remote.{remote}.push"),
        format!("remote.{remote}.mirror"),
    ] {
        let (out, status) = read_only::run(
            read_only::command(root).args(["config", "--get-all", &key]),
            cancel,
            deadline,
            16384,
            "Push routing inspection",
            "Push routing exceeds limit",
        )?;
        if status.success() || !out.is_empty() || status.code() != Some(1) {
            return Err("Custom push routing, mirror or push refspec configuration is unsupported in this review.".into());
        }
    }
    let url = line(run(
        root,
        &["remote", "get-url", "--push", "--all", remote],
        cancel,
        deadline,
    )?)?;
    let endpoint = endpoint(&url)?;
    Ok(Snapshot {
        root: root.into(),
        branch,
        head,
        remote: remote.into(),
        destination: destination.into(),
        endpoint,
    })
}
#[cfg(test)]
mod tests;
