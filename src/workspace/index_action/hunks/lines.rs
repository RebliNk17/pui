//! Bounded exact changed-line selections. Empty/invalid active intent never means whole file.
use super::*;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LineSelection {
    pub(crate) lines: Vec<Vec<usize>>,
    pub(crate) active: bool,
}
impl LineSelection {
    pub(crate) fn count(&self) -> usize {
        self.lines.iter().map(Vec::len).sum()
    }
    pub(crate) fn any(&self) -> bool {
        self.active && self.count() > 0
    }
}
struct Hunk<'a> {
    old: usize,
    new: usize,
    body: Vec<&'a str>,
}
fn span(s: &str, sign: char) -> Option<(usize, usize)> {
    let s = s.strip_prefix(sign)?;
    let (start, count) = s.split_once(',').unwrap_or((s, "1"));
    Some((start.parse().ok()?, count.parse().ok()?))
}
fn hunks(snapshot: &Snapshot) -> Option<(usize, Vec<Hunk<'_>>)> {
    let (first, _) = super::ranges(snapshot)?;
    let mut hunks: Vec<Hunk<'_>> = Vec::new();
    for line in snapshot.preview[first..].split_inclusive('\n') {
        if line.starts_with("@@ ") {
            let mut fields = line.split_whitespace();
            if fields.next() != Some("@@") {
                return None;
            }
            let (old, _) = span(fields.next()?, '-')?;
            let (new, _) = span(fields.next()?, '+')?;
            if fields.next() != Some("@@") {
                return None;
            }
            hunks.push(Hunk {
                old,
                new,
                body: Vec::new(),
            });
        } else {
            // Missing-newline markers require paired EOF semantics; refuse rather than guess.
            if !line.ends_with('\n') || !matches!(line.as_bytes().first(), Some(b' ' | b'+' | b'-'))
            {
                return None;
            }
            hunks.last_mut()?.body.push(line);
        }
    }
    Some((first, hunks))
}
/// All +/- lines, in original patch order; ordinal is global, never inferred from row position.
pub(crate) fn parse(snapshot: &Snapshot) -> Option<Vec<Vec<usize>>> {
    let (_, hunks) = hunks(snapshot)?;
    let mut ordinal = 0;
    Some(
        hunks
            .iter()
            .map(|h| {
                h.body
                    .iter()
                    .filter(|l| matches!(l.as_bytes()[0], b'+' | b'-'))
                    .map(|_| {
                        let n = ordinal;
                        ordinal += 1;
                        n
                    })
                    .collect()
            })
            .collect(),
    )
}
pub(crate) fn labels(snapshot: &Snapshot) -> Vec<String> {
    snapshot
        .preview
        .lines()
        .skip_while(|l| !l.starts_with("@@ "))
        .filter(|l| l.starts_with('+') || l.starts_with('-'))
        .map(|l| l.chars().take(160).collect())
        .collect()
}
pub(crate) fn synthesize(snapshot: &Snapshot) -> Option<String> {
    if !snapshot.lines.any() || snapshot.hunk.0.is_some() {
        return None;
    }
    let (first, hunks) = hunks(snapshot)?;
    let available = parse(snapshot)?;
    if snapshot.lines.lines.len() != hunks.len() {
        return None;
    }
    for (selected, valid) in snapshot.lines.lines.iter().zip(&available) {
        if selected.len() > valid.len() || selected.iter().any(|n| !valid.contains(n)) {
            return None;
        }
        for (i, n) in selected.iter().enumerate() {
            if selected[..i].contains(n) {
                return None;
            }
        }
    }
    let mut out = snapshot.preview[..first].to_owned();
    let mut ordinal = 0;
    let mut delta = 0isize;
    for (h, selected) in hunks.iter().zip(&snapshot.lines.lines) {
        let mut body = String::new();
        let mut old = 0;
        let mut new = 0;
        for line in &h.body {
            let kind = line.as_bytes()[0];
            let chosen = if kind == b' ' {
                false
            } else {
                let n = ordinal;
                ordinal += 1;
                selected.contains(&n)
            };
            let emitted = if chosen || kind == b' ' {
                Some(kind)
            } else {
                match (snapshot.action, kind) {
                    (Action::Stage, b'-') | (Action::Unstage, b'+') => Some(b' '),
                    _ => None,
                }
            };
            if let Some(kind) = emitted {
                if kind != b'+' {
                    old += 1;
                }
                if kind != b'-' {
                    new += 1;
                }
                body.push(char::from(kind));
                body.push_str(&line[1..]);
            }
        }
        if selected.is_empty() {
            continue;
        }
        // Anchor to the unchanged source side. Recompute destination offsets after omitted edits.
        let source_count = if snapshot.action == Action::Stage {
            h.body.iter().filter(|l| !l.starts_with('+')).count()
        } else {
            h.body.iter().filter(|l| !l.starts_with('-')).count()
        };
        let start = if snapshot.action == Action::Stage {
            h.old
        } else {
            h.new
        };
        let pos = if source_count == 0 {
            start
        } else {
            start.checked_sub(1)?
        };
        let (old_pos, new_pos) = if snapshot.action == Action::Stage {
            (pos, pos.checked_add_signed(delta)?)
        } else {
            (pos.checked_add_signed(-delta)?, pos)
        };
        let old_start = if old == 0 {
            old_pos
        } else {
            old_pos.checked_add(1)?
        };
        let new_start = if new == 0 {
            new_pos
        } else {
            new_pos.checked_add(1)?
        };
        out.push_str(&format!("@@ -{old_start},{old} +{new_start},{new} @@\n"));
        out.push_str(&body);
        delta += new as isize - old as isize;
    }
    Some(out)
}
#[cfg(test)]
mod tests;
