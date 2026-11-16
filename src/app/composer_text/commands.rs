//! Composer-only commands. Native menu accelerators and keyboard events share
//! this path; every transformation is an explicit undo point in TextEdit.
use super::*;
use egui::{
    text::{CCursor, CCursorRange},
    widgets::text_edit::TextEditState,
};

#[derive(Clone, Copy)]
pub(in crate::app) enum Format {
    Bold,
    Italic,
    Underline,
}
impl Format {
    fn delimiters(self) -> (&'static str, &'static str) {
        match self {
            Self::Bold => ("**", "**"),
            Self::Italic => ("*", "*"),
            Self::Underline => ("<u>", "</u>"),
        }
    }
}

fn change(
    ctx: &egui::Context,
    text: &mut String,
    edit: impl FnOnce(&mut String, &mut CCursorRange),
) {
    let id = egui::Id::new("composer");
    let Some(mut state) = TextEditState::load(ctx, id) else {
        return;
    };
    let Some(mut range) = state.cursor.char_range() else {
        return;
    };
    range.primary.index = range.primary.index.min(text.chars().count());
    range.secondary.index = range.secondary.index.min(text.chars().count());
    let mut undo = state.undoer();
    undo.add_undo(&(range, text.clone()));
    edit(text, &mut range);
    // feed_state clears an abandoned redo branch; add_undo alone does not.
    undo.feed_state(ctx.input(|i| i.time), &(range, text.clone()));
    undo.add_undo(&(range, text.clone()));
    state.set_undoer(undo);
    state.cursor.set_char_range(Some(range));
    state.store(ctx, id);
    ctx.request_repaint();
}

pub(in crate::app) fn format(ctx: &egui::Context, text: &mut String, format: Format) {
    change(ctx, text, |text, range| {
        let chars = range.as_sorted_char_range();
        let start = byte_index(text, chars.start);
        let end = byte_index(text, chars.end);
        let (open, close) = format.delimiters();
        let parsed = parse_source(text);
        let ranges: Vec<_> = match format {
            Format::Underline => underline_ranges(text),
            _ => Parser::new(&parsed)
                .into_offset_iter()
                .filter_map(|(event, range)| match (format, event) {
                    (Format::Bold, Event::Start(Tag::Strong))
                    | (Format::Italic, Event::Start(Tag::Emphasis)) => Some(range),
                    _ => None,
                })
                .collect(),
        };
        let inside = ranges.iter().any(|r| r.start == start && r.end == end)
            && end >= start + open.len() + close.len();
        let bold_marker = matches!(format, Format::Italic)
            && Parser::new(&parsed).into_offset_iter().any(|(event, r)| {
                matches!(event, Event::Start(Tag::Strong))
                    && (start == r.start + 1 || start + 1 == r.end)
            });
        let outside = !bold_marker
            && text[..start].ends_with(open)
            && text[end..].starts_with(close)
            && if start == end {
                !matches!(format, Format::Italic)
                    || (!text[..start - open.len()].ends_with('*')
                        && !text[end + close.len()..].starts_with('*'))
            } else {
                ranges
                    .iter()
                    .any(|r| r.start == start - open.len() && r.end == end + close.len())
            };
        let (new_start, new_end) = if inside {
            text.replace_range(end - close.len()..end, "");
            text.replace_range(start..start + open.len(), "");
            (chars.start, chars.end - open.len() - close.len())
        } else if outside {
            text.replace_range(end..end + close.len(), "");
            text.replace_range(start - open.len()..start, "");
            (chars.start - open.len(), chars.end - open.len())
        } else {
            text.insert_str(end, close);
            text.insert_str(start, open);
            (chars.start + open.len(), chars.end + open.len())
        };
        let forward = range.primary.index >= range.secondary.index;
        *range = if forward {
            CCursorRange::two(CCursor::new(new_start), CCursor::new(new_end))
        } else {
            CCursorRange::two(CCursor::new(new_end), CCursor::new(new_start))
        };
    });
}

pub(in crate::app) fn keys(ctx: &egui::Context, text: &mut String) {
    if !ctx.memory(|m| m.has_focus(egui::Id::new("composer"))) {
        return;
    }
    // Never reinterpret an in-progress IME composition.
    if ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Ime(_)))) {
        return;
    }
    for (key, action) in [
        (egui::Key::B, Format::Bold),
        (egui::Key::I, Format::Italic),
        (egui::Key::U, Format::Underline),
    ] {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, key)) {
            format(ctx, text, action);
        }
    }
    let shift = ctx.input(|i| i.modifiers.shift);
    let modifiers = if shift {
        egui::Modifiers::SHIFT
    } else {
        egui::Modifiers::NONE
    };
    let tab = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::Tab, pressed: true, modifiers: m, .. } if *m == modifiers)));
    if tab && indent(ctx, text, shift) {
        ctx.input_mut(|i| {
            i.consume_key(modifiers, egui::Key::Tab);
        });
        ctx.memory_mut(|m| {
            m.request_focus_with_filter(
                egui::Id::new("composer"),
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: false,
                },
            )
        });
    }
}

fn indent(ctx: &egui::Context, text: &mut String, outdent: bool) -> bool {
    let Some(state) = TextEditState::load(ctx, egui::Id::new("composer")) else {
        return false;
    };
    let Some(range) = state.cursor.char_range() else {
        return false;
    };
    let selection = range.as_sorted_char_range();
    let begin = byte_index(text, selection.start);
    let finish = byte_index(text, selection.end);
    let first = text[..begin].rfind('\n').map_or(0, |i| i + 1);
    let last = if finish > begin && text[..finish].ends_with('\n') {
        finish - 1
    } else {
        finish
    };
    let presentation = Presentation::new(text, None);
    let mut edits = Vec::new();
    let mut offset = first;
    for line in text[first..].split_inclusive('\n') {
        if offset > last {
            break;
        }
        let Some(prefix) = ListPrefix::parse(line) else {
            return false;
        };
        if !presentation.lists.iter().any(|r| r.contains(&offset)) {
            return false;
        }
        let remove = if outdent {
            if prefix.indent.ends_with('\t') {
                1
            } else {
                prefix.indent.len().min(4)
            }
        } else {
            0
        };
        edits.push((offset, remove));
        offset += line.len();
    }
    if edits.is_empty() {
        return false;
    }
    change(ctx, text, |text, range| {
        for (offset, remove) in edits.into_iter().rev() {
            let index = text[..offset].chars().count();
            text.replace_range(offset..offset + remove, if outdent { "" } else { "    " });
            for cursor in [&mut range.primary, &mut range.secondary] {
                if cursor.index >= index {
                    cursor.index = if outdent {
                        cursor.index.saturating_sub(remove).max(index)
                    } else {
                        cursor.index + 4
                    };
                }
            }
        }
    });
    true
}

pub(super) fn underline_ranges(source: &str) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    let parsed = parse_source(source);
    let literal: Vec<_> = Parser::new(&parsed)
        .into_offset_iter()
        .filter_map(|(e, r)| {
            matches!(e, Event::Code(_) | Event::Start(Tag::CodeBlock(_))).then_some(r)
        })
        .collect();
    let mut offset = 0;
    while let Some(open) = source[offset..].find("<u>") {
        let start = offset + open;
        let Some(close) = source[start + 3..].find("</u>") else {
            break;
        };
        let end = start + 3 + close + 4;
        if !literal.iter().any(|r| r.contains(&start)) {
            result.push(start..end);
        }
        offset = end;
    }
    result
}
