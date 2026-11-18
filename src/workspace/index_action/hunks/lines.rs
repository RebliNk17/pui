//! Per-line stage/unstage selection with a mandatory dry-run validity gate.
//! Only changed lines (added for Stage, removed for Unstage) are individually
//! selectable; context and the opposing side are always preserved so a hunk
//! remains a valid patch. A selection that `git apply --check` cannot apply to
//! a private index copy is refused rather than risk corrupting the index.
use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LineSelection {
    /// Per hunk index, selected changed-line ordinals.
    pub(crate) lines: Vec<Vec<usize>>,
    pub(crate) active: bool,
}
impl LineSelection {
    pub(crate) fn count(&self) -> usize {
        self.lines.iter().map(|l| l.len()).sum()
    }
    pub(crate) fn any(&self) -> bool {
        self.active && self.lines.iter().any(|l| !l.is_empty())
    }
}

struct Hunk {
    header: String,
    body: Vec<(u8, Option<usize>, String)>, // kind, changed ordinal, line text
}
/// Split the diff into hunks; each changed (+/-) line gets a stable ordinal.
pub(crate) fn parse(snapshot: &Snapshot) -> Option<Vec<Vec<usize>>> {
    if !match snapshot.action {
        Action::Stage => matches!(snapshot.status.as_str(), " M" | "MM"),
        Action::Unstage => matches!(snapshot.status.as_str(), "M " | "MM"),
        Action::Discard => false,
    } {
        return None;
    }
    let target = match snapshot.action {
        Action::Stage => b'+',
        Action::Unstage => b'-',
        Action::Discard => b' ',
    };
    let mut ordinal = 0usize;
    let mut hunks = Vec::new();
    let mut body = Vec::new();
    let mut in_hunk = false;
    let mut header = String::new();
    for line in snapshot.preview.split_inclusive('\n') {
        if line.starts_with("@@ ") {
            if in_hunk {
                hunks.push(Hunk {
                    header: std::mem::take(&mut header),
                    body: std::mem::take(&mut body),
                });
            }
            header = line.to_owned();
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        let kind = line.as_bytes().first().copied()?;
        let changed = (kind == target).then(|| {
            let n = ordinal;
            ordinal += 1;
            n
        });
        body.push((kind, changed, line.to_owned()));
    }
    if in_hunk {
        hunks.push(Hunk { header, body });
    }
    if hunks.is_empty() {
        return None;
    }
    let per_hunk = hunks
        .iter()
        .map(|h| h.body.iter().filter_map(|(_, c, _)| *c).collect())
        .collect();
    Some(per_hunk)
}

/// Build a patch keeping context, the opposing side, and only selected target
/// lines. Returns None for a selection that is not a clean subset.
pub(crate) fn synthesize(snapshot: &Snapshot) -> Option<String> {
    if !snapshot.lines.any() {
        return None;
    }
    let mut hunks = Vec::new();
    let mut body = Vec::new();
    let mut in_hunk = false;
    let mut header = String::new();
    let mut ordinal = 0usize;
    for line in snapshot.preview.split_inclusive('\n') {
        if line.starts_with("@@ ") {
            if in_hunk {
                hunks.push(Hunk {
                    header: std::mem::take(&mut header),
                    body: std::mem::take(&mut body),
                });
            }
            header = line.to_owned();
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        let kind = line.as_bytes().first().copied()?;
        let target = match snapshot.action {
            Action::Stage => b'+',
            Action::Unstage => b'-',
            Action::Discard => b' ',
        };
        let changed = (kind == target).then(|| {
            let n = ordinal;
            ordinal += 1;
            n
        });
        body.push((kind, changed, line.to_owned()));
    }
    if in_hunk {
        hunks.push(Hunk { header, body });
    }
    if hunks.is_empty() {
        return None;
    }
    let diff_header = snapshot.preview.split_once("\n@@ ").map_or("", |(h, _)| h);
    let mut out = diff_header.to_owned();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    let mut emitted = false;
    for (index, hunk) in hunks.iter().enumerate() {
        let sel = snapshot
            .lines
            .lines
            .get(index)
            .map_or(&[][..], |v| v.as_slice());
        let target = match snapshot.action {
            Action::Stage => b'+',
            Action::Unstage => b'-',
            Action::Discard => b' ',
        };
        let mut old = 0usize;
        let mut new = 0usize;
        let mut body_text = String::new();
        let mut target_kept = false;
        for (kind, changed, text) in &hunk.body {
            if matches!(kind, b'+' | b'-') {
                let keep = if *kind == target {
                    changed.is_some_and(|c| sel.contains(&c))
                } else {
                    true
                };
                if keep {
                    if *kind == b'-' {
                        old += 1;
                    } else {
                        new += 1;
                    }
                    if *kind == target {
                        target_kept = true;
                    }
                    body_text.push_str(text);
                }
            } else {
                old += 1;
                new += 1;
                body_text.push_str(text);
            }
        }
        // Only emit a hunk that actually selects at least one target line.
        if old == 0 && new == 0 || !target_kept {
            continue;
        }
        out.push_str(&recount(&hunk.header, old, new));
        out.push_str(&body_text);
        emitted = true;
    }
    if !emitted {
        return None;
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Some(out)
}
fn recount(header: &str, old: usize, new: usize) -> String {
    let trimmed = header
        .trim_start_matches("@@")
        .trim_end_matches("@@")
        .trim();
    let mut parts = trimmed.split_whitespace();
    let strip = |s: &str| s.trim_start_matches(['+', '-']).to_owned();
    let start_a = strip(parts.next().unwrap_or("0").split(',').next().unwrap_or("0"));
    let start_b = strip(parts.next().unwrap_or("0").split(',').next().unwrap_or("0"));
    format!("@@ -{start_a},{old} +{start_b},{new} @@\n")
}
#[cfg(test)]
mod tests;
