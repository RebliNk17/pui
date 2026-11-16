//! Whole-hunk extraction only; no line splitting, patch synthesis or rename guessing.
use super::*;
pub(crate) mod selection;
fn ranges(snapshot: &Snapshot) -> Option<(usize, Vec<usize>)> {
    if !match snapshot.action {
        Action::Stage => matches!(snapshot.status.as_str(), " M" | "MM"),
        Action::Unstage => matches!(snapshot.status.as_str(), "M " | "MM"),
        Action::Discard => false,
    } {
        return None;
    }
    let text = &snapshot.preview;
    if text.matches("diff --git ").count() != 1
        || text.lines().any(|l| {
            l.starts_with("old mode ")
                || l.starts_with("new mode ")
                || l.starts_with("new file mode ")
                || l.starts_with("deleted file mode ")
                || l.starts_with("rename ")
                || l.starts_with("Binary ")
        })
    {
        return None;
    }
    let mut offset = 0;
    let mut starts = Vec::new();
    for line in text.split_inclusive('\n') {
        if line.starts_with("@@ -") && line.contains(" @@") {
            starts.push(offset);
        }
        offset += line.len();
    }
    if starts.len() > 64 {
        return None;
    }
    let first = *starts.first()?;
    if !text[..first].lines().any(|l| l.starts_with("--- a/"))
        || !text[..first].lines().any(|l| l.starts_with("+++ b/"))
    {
        return None;
    }
    Some((first, starts))
}
pub(crate) fn count(snapshot: &Snapshot) -> usize {
    ranges(snapshot).map_or(0, |(_, starts)| starts.len())
}
#[cfg(test)]
pub(crate) fn extract(snapshot: &Snapshot, index: usize) -> Option<String> {
    let (first, starts) = ranges(snapshot)?;
    let start = *starts.get(index)?;
    let end = starts
        .get(index + 1)
        .copied()
        .unwrap_or(snapshot.preview.len());
    let mut patch = String::with_capacity(first + end - start);
    patch.push_str(&snapshot.preview[..first]);
    patch.push_str(&snapshot.preview[start..end]);
    Some(patch)
}
pub(crate) fn extract_selection(snapshot: &Snapshot) -> Option<String> {
    let (first, starts) = ranges(snapshot)?;
    if !snapshot.hunk.valid(starts.len()) {
        return None;
    }
    snapshot.hunk.0?;
    let mut patch = String::with_capacity(snapshot.preview.len());
    patch.push_str(&snapshot.preview[..first]);
    for (n, start) in starts.iter().enumerate() {
        if snapshot.hunk.contains(n) {
            let end = starts.get(n + 1).copied().unwrap_or(snapshot.preview.len());
            patch.push_str(&snapshot.preview[*start..end]);
        }
    }
    Some(patch)
}
#[cfg(test)]
mod tests;
