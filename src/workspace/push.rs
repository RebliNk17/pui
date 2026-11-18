//! Explicit reviewed single-branch Push; inspection remains local-only.
use super::*;
mod diagnostics;
pub(crate) mod remote;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub root: PathBuf,
    pub branch: String,
    pub head: String,
    pub remote: String,
    pub destination: String,
    pub endpoint: String,
    pub explicit_target: bool,
    pub signing_config: Vec<u8>,
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
    if Path::new(url).is_absolute() {
        return Ok(url.into());
    }
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
    prepare_target(root, None, cancel)
}
pub(crate) fn prepare_target(
    root: &Path,
    target: Option<(&str, &str)>,
    cancel: &AtomicBool,
) -> Result<Snapshot, String> {
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
    let bytes = if let Some((remote, destination)) = target {
        format!("{remote}\0{destination}\n").into_bytes()
    } else {
        run(
            root,
            &[
                "for-each-ref",
                "--format=%(upstream:remotename)%00%(upstream:remoteref)",
                &branch,
            ],
            cancel,
            deadline,
        )?
    };
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
    run(root, &["check-ref-format", destination], cancel, deadline)?;
    // Require an actual configured remote name, never interpret a URL/path as remote.
    let remotes = run(root, &["remote"], cancel, deadline)?;
    if !std::str::from_utf8(&remotes).is_ok_and(|s| s.lines().any(|name| name == remote)) {
        return Err("Choose an existing named remote.".into());
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
        explicit_target: target.is_some(),
        signing_config: signing_config(root, cancel, deadline)?,
    })
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Updated,
    UpToDate,
}
fn outcome(bytes: &[u8], success: bool) -> Result<Outcome, String> {
    // Only a fixed enum escapes this parser. Remote URLs/reasons/hook text stay hidden.
    let mut flag = None;
    for line in bytes.split(|b| *b == b'\n') {
        if line.len() >= 2 && line[1] == b'\t' {
            let fields = line.split(|b| *b == b'\t').collect::<Vec<_>>();
            if fields.len() != 3 || flag.is_some() || !matches!(line[0], b' ' | b'=' | b'*' | b'!')
            {
                return Err(
                    "Push response unavailable or ambiguous; inspect remote state before retrying."
                        .into(),
                );
            }
            flag = Some(line[0]);
        }
    }
    match(flag,success){(Some(b'='),true)=>Ok(Outcome::UpToDate),(Some(b' '|b'*'),true)=>Ok(Outcome::Updated),(Some(b'!'),_)=>Err("Push rejected for the reviewed branch. Remote was not updated by this refspec. Remote details are hidden; inspect its state and policy before retrying. No automatic retry.".into()),_=>Err("Push failed or response unavailable. Authentication/connection or remote policy may be responsible; inspect remote state before retrying. Remote details hidden; no automatic retry.".into())}
}
fn signing_config(root: &Path, cancel: &AtomicBool, deadline: Instant) -> Result<Vec<u8>, String> {
    let (bytes, status) = read_only::run(
        read_only::command(root).args([
            "config",
            "--includes",
            "--null",
            "--get-regexp",
            "^(gpg\\.|user\\.signingkey$|push\\.gpgSign$)",
        ]),
        cancel,
        deadline,
        16384,
        "Push signing configuration",
        "Signing configuration exceeds16KiB",
    )?;
    if status.success() || (status.code() == Some(1) && bytes.is_empty()) {
        Ok(bytes)
    } else {
        Err("Cannot inspect signing configuration".into())
    }
}
#[cfg(test)]
pub(crate) fn apply(snapshot: Snapshot, cancel: &AtomicBool) -> Result<Outcome, String> {
    apply_with_hooks(snapshot, false, cancel)
}
#[cfg(test)]
pub(crate) fn apply_with_hooks(
    snapshot: Snapshot,
    hooks: bool,
    cancel: &AtomicBool,
) -> Result<Outcome, String> {
    apply_signed(snapshot, hooks, false, false, 30, 30, cancel)
}
#[cfg(test)]
pub(crate) fn apply_diagnostics(
    snapshot: Snapshot,
    hooks: bool,
    details: bool,
    cancel: &AtomicBool,
) -> Result<Outcome, String> {
    apply_signed(snapshot, hooks, details, false, 30, 30, cancel)
}
#[cfg(test)]
pub(crate) fn apply_timed(
    snapshot: Snapshot,
    hooks: bool,
    details: bool,
    seconds: u64,
    cancel: &AtomicBool,
) -> Result<Outcome, String> {
    apply_signed(snapshot, hooks, details, false, seconds, 30, cancel)
}
pub(crate) fn apply_signed(
    snapshot: Snapshot,
    hooks: bool,
    details: bool,
    sign: bool,
    seconds: u64,
    sign_timeout: u64,
    cancel: &AtomicBool,
) -> Result<Outcome, String> {
    if !(1..=300).contains(&sign_timeout) {
        return Err("Push signing timeout must be 1–300 seconds.".into());
    }
    if !(1..=300).contains(&seconds) {
        return Err("Push timeout must be 1–300 seconds.".into());
    }
    let target = snapshot
        .explicit_target
        .then_some((snapshot.remote.as_str(), snapshot.destination.as_str()));
    if prepare_target(&snapshot.root, target, cancel)? != snapshot {
        return Err("Push source or destination changed; inspect again.".into());
    }
    let mut command = network_command(&snapshot.root);
    if sign {
        // Config recheck only, not signer availability. Actual signing occurs during Git push.
        let (signing, st) = read_only::run(
            read_only::command(&snapshot.root).args([
                "config",
                "--includes",
                "--null",
                "--get-regexp",
                "^(gpg\\.|user\\.signingkey$|push\\.gpgSign$)",
            ]),
            cancel,
            Instant::now() + std::time::Duration::from_secs(sign_timeout),
            16384,
            "Push signing configuration",
            "Push signing configuration exceeds 16 KiB",
        )?;
        if signing != snapshot.signing_config
            || !(st.success() || st.code() == Some(1) && signing.is_empty())
        {
            return Err(
                "Cannot inspect push signing configuration. Configure a signing key before requesting a signed push.".into(),
            );
        }
        command.args(["-c", "push.gpgSign=yes"]);
    } else {
        command.args(["-c", "push.gpgSign=false"]);
    }
    if !hooks {
        command.args(["-c", "core.hooksPath=/dev/null"]);
    }
    command.args([
        "-c",
        "push.followTags=false",
        "-c",
        "push.recurseSubmodules=no",
        "push",
        "--porcelain",
        "--no-follow-tags",
        "--recurse-submodules=no",
    ]);
    if sign {
        command.arg("--signed=yes");
    }
    if !hooks {
        command.arg("--no-verify");
    }
    command.args([
        "--",
        &snapshot.endpoint,
        &format!("{}:{}", snapshot.head, snapshot.destination),
    ]);
    // Signing is negotiated over the network; conservatively cap the whole child
    // by both chosen budgets. This is NOT an independent signer-only deadline.
    let seconds = if sign {
        seconds.min(sign_timeout)
    } else {
        seconds
    };
    // No remote aliases/refspec defaults, no '+' or force, one exact reviewed OID/ref.
    if details {
        return match diagnostics::run(
            &mut command,
            cancel,
            Instant::now() + std::time::Duration::from_secs(seconds),
            "Push diagnostics",
        ) {
            Ok((out, err, status)) => match outcome(&out, status.success()) {
                Ok(result) => Ok(result),
                Err(error) => {
                    let mut combined = out;
                    combined.extend_from_slice(b"\n[stderr]\n");
                    combined.extend_from_slice(&err);
                    let text = String::from_utf8_lossy(&combined);
                    let mut literal = String::new();
                    for ch in text.chars() {
                        if ch.is_control() && !matches!(ch, '\n' | '\t') {
                            literal.extend(ch.escape_default());
                        } else {
                            literal.push(ch);
                        }
                    }
                    Err(format!(
                        "{error}\nOpted-in untrusted remote/hook stdout then stderr — may contain secrets; review before sharing. Not redacted; stream ordering not preserved.\n{literal}"
                    ))
                }
            },
            Err(error) => Err(error),
        };
    }
    let result = read_only::run(
        &mut command,
        cancel,
        Instant::now() + std::time::Duration::from_secs(seconds),
        16384,
        "Confirmed push",
        "Push output exceeded limit; remote completion is uncertain.",
    );
    match result{Ok((bytes,status))=>outcome(&bytes,status.success()),Err(_)=>Err("Push cancelled, timed out or unavailable; remote completion may be uncertain. Inspect remote state before retrying. No automatic retry.".into())}
}
pub(crate) fn network_command(root: &Path) -> std::process::Command {
    let mut command = read_only::command(root);
    command
        .env_remove("GIT_SSH")
        .env_remove("GIT_SSH_COMMAND")
        .env_remove("GIT_SSH_VARIANT")
        .env(
            "GIT_SSH_COMMAND",
            "/usr/bin/ssh -oBatchMode=yes -oStrictHostKeyChecking=yes -oConnectTimeout=10",
        )
        .env("GIT_ASKPASS", "/usr/bin/false")
        .env("SSH_ASKPASS", "/usr/bin/false")
        .env("GIT_NO_REPLACE_OBJECTS", "1");
    command
}
fn remote_oid(bytes: &[u8], destination: &str) -> Result<Option<String>, String> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "Invalid remote response")?;
    let text = text.strip_suffix('\n').ok_or("Invalid remote response")?;
    let (oid, name) = text.split_once('\t').ok_or("Invalid remote response")?;
    if name != destination || !history::oid(oid) {
        return Err("Remote response does not match the exact branch.".into());
    }
    Ok(Some(oid.into()))
}
pub(crate) fn inspect_remote(
    snapshot: &Snapshot,
    cancel: &AtomicBool,
) -> Result<Option<String>, String> {
    let target = snapshot
        .explicit_target
        .then_some((snapshot.remote.as_str(), snapshot.destination.as_str()));
    if prepare_target(&snapshot.root, target, cancel)? != *snapshot {
        return Err("Push destination changed; inspect again.".into());
    }
    let mut command = network_command(&snapshot.root);
    command.args([
        "ls-remote",
        "--refs",
        "--",
        &snapshot.endpoint,
        &snapshot.destination,
    ]);
    let (out, status) = read_only::run(
        &mut command,
        cancel,
        Instant::now() + std::time::Duration::from_secs(30),
        1024,
        "Remote branch inspection",
        "Remote response exceeds limit",
    )
    .map_err(|_| {
        "Remote inspection cancelled/timed out or unavailable. No automatic retry.".to_string()
    })?;
    if !status.success() {
        return Err("Remote inspection failed; authentication/connection may be unavailable. Remote details hidden.".into());
    }
    remote_oid(&out, &snapshot.destination)
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Relation {
    Absent,
    Equal,
    Counts { ahead: u32, behind: u32 },
    Unavailable,
}
impl Relation {
    pub(crate) fn label(&self) -> String {
        match self{Self::Absent=>"Remote branch was absent at inspection.".into(),Self::Equal=>"Observed remote commit equals the reviewed local commit.".into(),Self::Counts{ahead,behind}=>format!("Compared with observed remote: {ahead} ahead · {behind} behind{}",if *ahead>0&&*behind>0{" · diverged"}else{""}),Self::Unavailable=>"Observed remote commit is not fully available locally; comparison unavailable. No fetch performed.".into()}
    }
}
fn parse_counts(bytes: &[u8]) -> Option<Relation> {
    let text = std::str::from_utf8(bytes).ok()?.strip_suffix('\n')?;
    let (a, b) = text.split_once('\t')?;
    let parse = |s: &str| {
        if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
            s.parse::<u32>().ok()
        } else {
            None
        }
    };
    Some(Relation::Counts {
        ahead: parse(a)?,
        behind: parse(b)?,
    })
}
pub(crate) fn compare(snapshot: &Snapshot, remote: Option<&str>, cancel: &AtomicBool) -> Relation {
    let Some(remote) = remote else {
        return Relation::Absent;
    };
    if !history::oid(remote) || remote.len() != snapshot.head.len() {
        return Relation::Unavailable;
    }
    if remote == snapshot.head {
        return Relation::Equal;
    }
    let deadline = Instant::now() + DEADLINE;
    let shallow = read_only::run(
        read_only::command(&snapshot.root).args(["rev-parse", "--is-shallow-repository"]),
        cancel,
        deadline,
        16,
        "Local graph availability",
        "Invalid graph availability",
    );
    if !matches!(shallow,Ok((ref bytes,ref status))if status.success()&&bytes==b"false\n") {
        return Relation::Unavailable;
    }
    let mut command = read_only::command(&snapshot.root);
    command.env("GIT_NO_REPLACE_OBJECTS", "1").args([
        "rev-list",
        "--left-right",
        "--count",
        &format!("{}...{}", snapshot.head, remote),
        "--",
    ]);
    match read_only::run(
        &mut command,
        cancel,
        Instant::now() + DEADLINE,
        128,
        "Local branch comparison",
        "Invalid branch counts",
    ) {
        Ok((bytes, status)) if status.success() => {
            parse_counts(&bytes).unwrap_or(Relation::Unavailable)
        }
        _ => Relation::Unavailable,
    }
}
#[cfg(test)]
mod tests;
