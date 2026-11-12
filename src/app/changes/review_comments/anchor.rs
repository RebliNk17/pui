//! Exact character selection in one immutable numbered-diff label. No text matching.
use super::*;
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Selection {
    pub chars: Range<usize>,
    pub rows: (usize, usize),
    pub hunk: usize,
    pub old: Option<(usize, usize)>,
    pub new: Option<(usize, usize)>,
    pub located: bool,
}
impl Selection {
    pub(super) fn label(&self) -> String {
        let mut label = format!("diff rows {}–{}", self.rows.0, self.rows.1);
        if self.located {
            for (name, span) in [("old", self.old), ("new", self.new)] {
                if let Some((first, last)) = span {
                    label.push_str(&format!(" · {name} {first}–{last}"));
                }
            }
        } else {
            label.push_str(" · file coordinates unavailable");
        }
        label
    }
}

/// Display offsets are Unicode characters, including the renderer's synthetic gutters.
/// Soft wraps add no characters. Headers, metadata, gutter-only and cross-hunk ranges fail.
pub(super) fn selected(diff: &str, range: Range<usize>) -> Option<Selection> {
    if range.start >= range.end || diff.len() > MAX_DIFF_BYTES {
        return None;
    }
    let mut result = Selection {
        chars: range.clone(),
        rows: (0, 0),
        hunk: 0,
        old: None,
        new: None,
        located: true,
    };
    let mut offset = 0usize;
    let mut content = false;
    let mut seen = false;
    for row in lines::rows(diff) {
        let gutter = if row.header {
            0
        } else {
            row.gutter().chars().count()
        };
        let chars = row.text.chars().count();
        let length = if row.header {
            lines::HUNK_CAPTION.chars().count()
        } else {
            gutter.checked_add(chars)?.checked_add(1)?
        };
        let next = offset.checked_add(length)?;
        if range.start < next && range.end > offset {
            if !row.selectable() || row.header || seen && result.hunk != row.hunk {
                return None;
            }
            if !seen {
                result.rows.0 = row.index + 1;
                result.hunk = row.hunk;
                seen = true;
            }
            result.rows.1 = row.index + 1;
            result.located &= row.located;
            for (span, value) in [(&mut result.old, row.old), (&mut result.new, row.new)] {
                if let Some(value) = value {
                    *span = Some(span.map_or((value, value), |(first, _)| (first, value)));
                }
            }
            let text_start = offset.checked_add(gutter)?;
            // A selected newline or gutter is not source content by itself.
            content |= range.start < text_start + chars && range.end > text_start;
        }
        offset = next;
        if range.end <= offset {
            break;
        }
    }
    (seen && content && range.end <= offset).then_some(result)
}
