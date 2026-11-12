//! Display text only. Never pretend rendered Markdown offsets are source line coordinates.
use super::*;
use std::{ops::Range, sync::Arc};

const DEMO_MARKDOWN: &str = "Rendered **chosen reply** with `literal code` and [a link](https://example.test).\n\nOnly the chosen displayed text becomes draft context.";
const DEMO_TEXT: &str = "Rendered chosen reply with literal code and a link.";
fn demo_key() -> egui::Id {
    egui::Id::new("rendered-selection-demo")
}
pub(in crate::app) fn demo(ctx: &egui::Context) -> &'static str {
    ctx.data_mut(|data| data.insert_temp(demo_key(), true));
    DEMO_MARKDOWN
}

pub(crate) fn offer(response: &egui::Response, label: &str, text: &str) {
    let demo = text == DEMO_TEXT
        && response
            .ctx
            .data_mut(|data| data.remove_temp::<bool>(demo_key()))
            .unwrap_or(false);
    source::menu(response, source::Kind::Rendered, demo, |range| {
        let range = if demo { Some(9..21) } else { range };
        (
            range
                .and_then(|range| selected(text, range, label))
                .map(Arc::new),
            None,
        )
    });
}
fn selected(text: &str, range: Range<usize>, label: &str) -> Option<Snippet> {
    if range.start >= range.end {
        return None;
    }
    let mut indices = text.char_indices();
    let first = indices.nth(range.start)?;
    let length = range.end - range.start;
    let last = if length == 1 {
        first
    } else {
        indices.nth(length - 2)?
    };
    let start = first.0;
    let end = last.0 + last.1.len_utf8();
    Some(Snippet {
        label: format!("{label} · selected rendered text (not Markdown source)"),
        text: source::bounded(&text[start..end]),
    })
}

#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
