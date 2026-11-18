//! Bounded commit-message inspection for a validated, explicitly chosen local OID.
use super::*;
const MAX_WIRE: usize = 32 * 1024;
pub(crate) const MAX_MESSAGE: usize = 24 * 1024;
const MAX_PARENTS: usize = 16;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Detail {
    pub oid: String,
    pub time: i64,
    pub parents: Vec<String>,
    pub message: String,
    /// Empty when unsigned, P when an embedded signature is present but unverified.
    pub signature: String,
}
fn command(root: &Path, hash: &str) -> Command {
    let mut command = super::command(root);
    // Never use %G?/%GS here: they can launch a configured external verifier.
    command.args([
        "show",
        "--no-patch",
        "--no-decorate",
        "--no-show-signature",
        "--no-notes",
        "--no-use-mailmap",
        "--no-ext-diff",
        "--no-textconv",
        "--encoding=UTF-8",
        "--format=%H%x00%ct%x00%P%x00N%x00%x00%B%x00",
        "--end-of-options",
        hash,
        "--",
    ]);
    command
}
fn parse(bytes: &[u8], expected: &str) -> Result<Detail, String> {
    if !oid(expected) || bytes.len() > MAX_WIRE {
        return Err("Commit detail exceeds its limits.".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "Commit message is not UTF-8.")?;
    let mut fields = text
        .strip_suffix("\0\n")
        .ok_or("Invalid commit detail framing.")?
        .split('\0');
    let hash = fields
        .next()
        .filter(|hash| *hash == expected)
        .ok_or("Commit detail does not match the selected object.")?;
    let time = fields
        .next()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value >= 0 && *value <= 253_402_300_799)
        .ok_or("Invalid commit timestamp.")?;
    let parents = fields.next().ok_or("Missing commit parents.")?;
    let parents = if parents.is_empty() {
        Vec::new()
    } else {
        let mut result = Vec::new();
        for parent in parents.split(' ') {
            if result.len() >= MAX_PARENTS
                || !oid(parent)
                || parent.len() != expected.len()
                || parent == expected
                || result.iter().any(|previous| previous == parent)
            {
                return Err("Invalid or excessive commit parents. Nothing is shown.".into());
            }
            result.push(parent.to_owned());
        }
        result.shrink_to_fit();
        result
    };
    let sig_status = fields.next().ok_or("Missing signature status.")?;
    let sig_status = sig_status.trim();
    if sig_status.len() != 1 || !sig_status.is_ascii() {
        return Err("Invalid signature status.".into());
    }
    let signer = fields.next().ok_or("Missing signature signer.")?;
    let signature = if sig_status == "N" {
        String::new()
    } else {
        // U=unknown, X/B/R/E=bad/expired/revoked/error, G/Y=good/trusted.
        let signer = if signer.chars().any(char::is_control) {
            String::new()
        } else {
            signer.to_owned()
        };
        format!("{sig_status} {signer}").trim().to_owned()
    };
    let message = fields
        .next()
        .filter(|message| {
            message.len() <= MAX_MESSAGE
                && !message
                    .chars()
                    .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
        })
        .ok_or(
            "Commit message exceeds 24 KiB or contains unsupported controls. Nothing is shown.",
        )?;
    if fields.next().is_some() {
        return Err("Invalid commit message framing.".into());
    }
    Ok(Detail {
        oid: hash.into(),
        time,
        parents,
        message: message.into(),
        signature,
    })
}
pub(crate) fn read(root: &Path, hash: &str, cancel: &AtomicBool) -> Result<Detail, String> {
    if !oid(hash) {
        return Err("Invalid selected commit object ID.".into());
    }
    let (bytes, status) = read_only::run(
        &mut command(root, hash),
        cancel,
        Instant::now() + DEADLINE,
        MAX_WIRE,
        "Commit detail read",
        "Commit detail exceeds 32 KiB. Nothing is shown.",
    )?;
    if !status.success() {
        return Err("Selected local commit is unavailable. No network fetch was attempted.".into());
    }
    let mut detail = parse(&bytes, hash)?;
    let (raw, status) = read_only::run(
        super::command(root).args(["cat-file", "commit", hash]),
        cancel,
        Instant::now() + DEADLINE,
        128 * 1024,
        "Embedded signature inspection",
        "Commit object exceeds signature inspection limit",
    )?;
    if !status.success() {
        return Err("Local commit object unavailable; no verifier was run.".into());
    }
    let headers = raw
        .split(|b| *b == b'\n')
        .take_while(|line| !line.is_empty());
    if headers
        .into_iter()
        .any(|line| line.starts_with(b"gpgsig ") || line.starts_with(b"gpgsig-sha256 "))
    {
        detail.signature = "P".into();
    }
    Ok(detail)
}
#[cfg(test)]
mod tests;
