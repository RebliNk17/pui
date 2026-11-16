//! Map one widget's selected diff text across hunks, never quote synthetic gutters.
use super::*;
use std::{ops::Range, sync::Arc};

const DEMO_DIFF: &str = "@@ -10,2 +10,2 @@\n-old value\n+new selected value\n context\n";
fn demo_key() -> egui::Id {
    egui::Id::new("diff-selection-demo")
}
pub(in crate::app) fn demo(ctx: &egui::Context) -> (String, String) {
    ctx.data_mut(|data| data.insert_temp(demo_key(), true));
    ("demo-fixture.rs".into(), DEMO_DIFF.into())
}

pub(in crate::app) fn offer(response: &egui::Response, path: &str, diff: &str, recorded: bool) {
    let demo = diff == DEMO_DIFF
        && response
            .ctx
            .data_mut(|data| data.remove_temp::<bool>(demo_key()))
            .unwrap_or(false);
    source::menu(response, source::Kind::Diff, demo, |range| {
        let range = if demo {
            // Synthetic display range for native framing screenshots; real drag paths have separate tests.
            let start = changes::lines::HUNK_CAPTION.chars().count() + 8;
            let end =
                start + "-old value\n".chars().count() + 8 + "+new selected value".chars().count();
            Some(start..end)
        } else {
            range
        };
        (
            range
                .and_then(|range| selected(diff, range, path, recorded))
                .map(Arc::new),
            None,
        )
    });
}

fn append(text: &mut String, part: &str) {
    if text.len() > MAX_BYTES {
        return;
    }
    let mut end = part.len().min((MAX_BYTES + 4).saturating_sub(text.len()));
    while !part.is_char_boundary(end) {
        end -= 1;
    }
    text.push_str(&part[..end]);
}

/// The renderer uses one character-indexed label: [gutter][original diff line][LF].
/// Hunk captions are synthetic ellipses and cannot be quoted as if they were source.
fn selected(diff: &str, range: Range<usize>, path: &str, recorded: bool) -> Option<Snippet> {
    if range.start >= range.end {
        return None;
    }
    let mut offset = 0usize;
    let mut text = String::with_capacity(diff.len().min(MAX_BYTES + 4));
    let mut hunk = None;
    let mut hunk_count = 0usize;
    let mut first_row = 0;
    let mut last_row = 0;
    let mut old = None;
    let mut new = None;
    let mut located = true;
    for row in changes::lines::rows(diff) {
        let gutter = if row.header { 0 } else { row.gutter().len() };
        let length = if row.header {
            changes::lines::HUNK_CAPTION.chars().count()
        } else {
            gutter + row.text.chars().count() + 1
        };
        let next = offset.checked_add(length)?;
        if range.start < next && range.end > offset {
            if row.header {
                // Only a fully crossed caption between real selected content is supported.
                // Quote the original header, never the rendered synthetic ellipsis.
                if text.is_empty() || range.start > offset || range.end <= next {
                    return None;
                }
                append(&mut text, row.text);
                append(&mut text, "\n");
            } else {
                if !row.selectable() {
                    return None;
                }
                match hunk {
                    Some(previous) if previous != row.hunk => {
                        hunk = Some(row.hunk);
                        hunk_count += 1;
                    }
                    None => {
                        hunk = Some(row.hunk);
                        hunk_count = 1;
                        first_row = row.index + 1;
                    }
                    _ => {}
                }
                last_row = row.index + 1;
                located &= row.located;
                if let Some(value) = row.old {
                    old = Some(old.map_or((value, value), |(first, _)| (first, value)));
                }
                if let Some(value) = row.new {
                    new = Some(new.map_or((value, value), |(first, _)| (first, value)));
                }
                let start = range.start.saturating_sub(offset).saturating_sub(gutter);
                let end = range
                    .end
                    .min(next)
                    .saturating_sub(offset)
                    .saturating_sub(gutter);
                let count = row.text.chars().count();
                if start < end && start < count {
                    let from = row.text.char_indices().nth(start)?.0;
                    let to = row
                        .text
                        .char_indices()
                        .nth(end.min(count))
                        .map_or(row.text.len(), |(byte, _)| byte);
                    append(&mut text, &row.text[from..to]);
                }
                if end > count && start <= count {
                    append(&mut text, "\n");
                }
            }
        }
        offset = next;
        if range.end <= offset {
            break;
        }
    }
    if range.end > offset || text.is_empty() {
        return None;
    }
    let coordinates = if hunk_count > 1 {
        format!("{hunk_count} hunks · original hunk headers included · discontinuous file ranges")
    } else if located {
        let value = |name: &str, span: Option<(usize, usize)>| {
            span.map(|(first, last)| format!("{name} {first}–{last}"))
        };
        [value("old", old), value("new", new)]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ")
    } else {
        "file line coordinates unavailable".into()
    };
    let kind = if recorded {
        "Recorded edits (not net Git)"
    } else {
        "Working tree vs HEAD"
    };
    Some(Snippet {
        label: format!("{path} · {kind} · diff rows {first_row}–{last_row} · {coordinates}"),
        text,
    })
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
