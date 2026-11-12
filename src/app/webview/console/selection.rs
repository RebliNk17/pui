//! Exact one-console-label selection. Character offsets refer to the retained
//! message, never the page DOM, its source URL, or a neighboring console row.
use super::*;
use std::ops::Range;
fn selected(line: &ConsoleLine, key: &str, range: Range<usize>) -> Option<(String, String)> {
    if range.start >= range.end {
        return None;
    }
    let mut offsets = line
        .text
        .char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(line.text.len()));
    let begin = offsets.nth(range.start)?;
    let end = offsets.nth(range.end.checked_sub(range.start + 1)?)?;
    let excerpt = line.text.get(begin..end)?;
    if excerpt.is_empty() {
        return None;
    }
    let label = format!(
        "Browser console {} · {} · message {} · selected characters {}–{} (end exclusive)",
        if line.error { "error" } else { "message" },
        page_label(key, &line.page),
        line.id,
        range.start + 1,
        range.end + 1
    );
    let mut text = String::with_capacity(excerpt.len() + line.source.len() + 120);
    text.push_str(excerpt);
    if !line.source.is_empty() {
        text.push_str("\nSource: ");
        text.push_str(&line.source);
        if line.line > 0 {
            text.push(':');
            text.push_str(&line.line.to_string());
        }
    }
    if line.limited {
        text.push_str(
            "\n[Selection is from a console message/source limited to 4,000 characters total]",
        );
    }
    Some((label, text))
}
pub(super) fn offer(response: &egui::Response, key: &str, line: &ConsoleLine) {
    context_capture::offer_console_selection(response, |range| {
        range.and_then(|range| selected(line, key, range))
    });
}
#[cfg(test)]
mod tests;
