//! Live Markdown presentation without a second document model. The draft, undo
//! history, clipboard and cursor indices always refer to the original Markdown.
use super::*;
use egui::{TextBuffer, TextFormat, text::LayoutJob, widgets::text_edit::TextEditOutput};
use pulldown_cmark::{Event, LinkType, Parser, Tag, TagEnd};
use std::{ops::Range, sync::Arc};

#[derive(Clone, Copy, Default, PartialEq)]
struct Style {
    bold: bool,
    italic: bool,
    code: bool,
    link: bool,
    hidden: bool,
    bullet: bool,
}
struct Link {
    range: Range<usize>,
    url: String,
}
struct Presentation {
    styles: Vec<Style>,
    links: Vec<Link>,
}

fn editing(range: &Range<usize>, selection: Option<Range<usize>>) -> bool {
    selection.is_some_and(|s| {
        if s.is_empty() {
            range.start <= s.start && s.start < range.end
        } else {
            s.start < range.end && s.end > range.start
        }
    })
}

fn safe_link(target: &str) -> bool {
    url::Url::parse(target).is_ok_and(|url| matches!(url.scheme(), "http" | "https" | "mailto"))
}

impl Presentation {
    fn new(source: &str, selection: Option<Range<usize>>) -> Self {
        let mut result = Self {
            styles: vec![Style::default(); source.len()],
            links: Vec::new(),
        };
        let events: Vec<_> = Parser::new(source).into_offset_iter().collect();
        let inline_literals: Vec<_> = events
            .iter()
            .filter_map(|(event, range)| {
                matches!(event, Event::Code(_) | Event::InlineHtml(_)).then_some(range.clone())
            })
            .collect();
        let mut literal = Vec::new();
        let mut in_code = false;
        for (event, range) in events {
            match event {
                Event::Start(Tag::CodeBlock(_)) => {
                    literal.push(range.clone());
                    result.apply(range, |s| s.code = true);
                    in_code = true;
                }
                Event::End(TagEnd::CodeBlock) => in_code = false,
                _ if in_code => {}
                Event::Rule | Event::Html(_) | Event::InlineHtml(_) => literal.push(range),
                Event::Start(Tag::Strong) => {
                    result.apply(range.clone(), |s| s.bold = true);
                    result.delimiters(source, range, 2, selection.clone());
                }
                Event::Start(Tag::Emphasis) => {
                    result.apply(range.clone(), |s| s.italic = true);
                    result.delimiters(source, range, 1, selection.clone());
                }
                Event::Code(_) => {
                    result.apply(range.clone(), |s| s.code = true);
                    let ticks = source[range.clone()]
                        .bytes()
                        .take_while(|b| *b == b'`')
                        .count();
                    result.delimiters(source, range.clone(), ticks, selection.clone());
                    literal.push(range);
                }
                Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    ..
                }) => {
                    result.apply(range.clone(), |s| s.link = true);
                    if !editing(&range, selection.clone()) {
                        let value = &source[range.clone()];
                        if matches!(link_type, LinkType::Autolink | LinkType::Email) {
                            result.hide(range.start..range.start + 1);
                            result.hide(range.end - 1..range.end);
                        } else if value.starts_with('[') {
                            // Locate the matching label bracket, including nested/escaped brackets.
                            let mut depth = 0;
                            let mut escaped = false;
                            for (offset, c) in value.char_indices() {
                                if inline_literals
                                    .iter()
                                    .any(|r| r.contains(&(range.start + offset)))
                                {
                                    continue;
                                }
                                if escaped {
                                    escaped = false;
                                    continue;
                                }
                                match c {
                                    '\\' => escaped = true,
                                    '[' => depth += 1,
                                    ']' => {
                                        depth -= 1;
                                        if depth == 0 {
                                            result.hide(range.start..range.start + 1);
                                            result.hide(range.start + offset..range.end);
                                            break;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    result.links.push(Link {
                        range,
                        url: dest_url.into_string(),
                    });
                }
                _ => {}
            }
        }
        // Recognize a marker as soon as its following space is typed, even if
        // CommonMark has not yet emitted a nonempty list item.
        let mut offset = 0;
        for line in source.split_inclusive('\n') {
            if let Some(prefix) = ListPrefix::parse(line) {
                if prefix.number.is_none()
                    && !literal
                        .iter()
                        .any(|r| r.contains(&(offset + prefix.indent.len())))
                {
                    result.styles[offset + prefix.indent.len()].bullet = true;
                }
            }
            offset += line.len();
        }
        // Plain pasted URLs are links too. Never execute arbitrary URL schemes.
        let mut offset = 0;
        for word in source.split_inclusive(char::is_whitespace) {
            let value = word.trim_end().trim_end_matches(['.', ',', ';', '!', '?']);
            if (value.starts_with("https://") || value.starts_with("http://"))
                && safe_link(value)
                && !literal.iter().any(|r| r.contains(&offset))
                && !result.links.iter().any(|l| l.range.contains(&offset))
            {
                let range = offset..offset + value.len();
                result.apply(range.clone(), |s| s.link = true);
                result.links.push(Link {
                    range,
                    url: value.into(),
                });
            }
            offset += word.len();
        }
        result
    }
    fn apply(&mut self, range: Range<usize>, mut f: impl FnMut(&mut Style)) {
        for style in &mut self.styles[range] {
            f(style);
        }
    }
    fn hide(&mut self, range: Range<usize>) {
        self.apply(range, |s| s.hidden = true);
    }
    fn delimiters(
        &mut self,
        source: &str,
        range: Range<usize>,
        width: usize,
        selection: Option<Range<usize>>,
    ) {
        if width > 0 && range.len() >= width * 2 && !editing(&range, selection) {
            let open = range.start..range.start + width;
            let close = range.end - width..range.end;
            if source.is_char_boundary(open.end) && source.is_char_boundary(close.start) {
                self.hide(open);
                self.hide(close);
            }
        }
    }
}

fn selection(ctx: &egui::Context, source: &str) -> Option<Range<usize>> {
    if !ctx.memory(|m| m.has_focus(egui::Id::new("composer"))) {
        return None;
    }
    let state = egui::TextEdit::load_state(ctx, egui::Id::new("composer"))?;
    let range = state.cursor.char_range()?.as_sorted_char_range();
    Some(byte_index(source, range.start)..byte_index(source, range.end))
}
fn byte_index(source: &str, index: usize) -> usize {
    source
        .char_indices()
        .nth(index)
        .map_or(source.len(), |(i, _)| i)
}

pub(super) fn layout(ui: &egui::Ui, text: &dyn TextBuffer, width: f32) -> Arc<egui::Galley> {
    galley(ui.ctx(), text.as_str(), width)
}

pub(super) fn galley(ctx: &egui::Context, source: &str, width: f32) -> Arc<egui::Galley> {
    let presentation = Presentation::new(source, selection(ctx, source));
    let colors = theme::palette(ctx);
    let mut display = LayoutJob::default();
    display.wrap.max_width = width;
    display.align_baseline = true;
    let mut original = display.clone();
    // Keep exactly one display character per source character. Invisible syntax
    // uses zero-width glyphs, not omitted characters, so egui's selection, IME,
    // bidi, clipboard and undo continue using the source's character indices.
    let mut runs: Vec<(usize, usize, Style)> = Vec::new();
    for (start, c) in source.char_indices() {
        let style = presentation.styles[start];
        if let Some(last) = runs.last_mut().filter(|r| r.2 == style) {
            last.1 = start + c.len_utf8();
        } else {
            runs.push((start, start + c.len_utf8(), style));
        }
    }
    for (start, end, style) in runs {
        let format = TextFormat {
            font_id: if style.code {
                FontId::monospace(14.0)
            } else if style.bold {
                FontId::new(14.0, egui::FontFamily::Name("body-bold".into()))
            } else {
                FontId::proportional(14.0)
            },
            color: if style.link {
                colors.accent
            } else {
                colors.text
            },
            background: if style.code && !style.hidden {
                colors.code_bg
            } else {
                Color32::TRANSPARENT
            },
            italics: style.italic,
            underline: if style.link && !style.hidden {
                Stroke::new(0.5, colors.accent)
            } else {
                Stroke::NONE
            },
            valign: Align::Center,
            ..Default::default()
        };
        let value = &source[start..end];
        let visible: String = value
            .chars()
            .map(|c| {
                if style.hidden && c != '\n' {
                    '\u{200b}'
                } else if style.bullet {
                    '•'
                } else {
                    c
                }
            })
            .collect();
        display.append(&visible, 0.0, format.clone());
        original.append(value, 0.0, format);
    }
    let mut galley = ctx.fonts(|f| f.layout_job(display));
    // Geometry uses presentation glyphs; word navigation/accessibility must see
    // the original text and its byte ranges. Section counts/formats are identical.
    Arc::make_mut(&mut galley).job = Arc::new(original);
    galley
}

pub(super) fn links(ui: &egui::Ui, output: &TextEditOutput, source: &str) {
    let Some(pos) = output.response.hover_pos() else {
        return;
    };
    let cursor = output.galley.cursor_from_pos(pos - output.galley_pos);
    let index = byte_index(source, cursor.index);
    let presentation = Presentation::new(source, None);
    if let Some(link) = presentation
        .links
        .iter()
        .find(|l| l.range.contains(&index) && safe_link(&l.url))
    {
        output.response.clone().on_hover_text(format!(
            "{}-click to open {}",
            if cfg!(target_os = "macos") {
                "⌘"
            } else {
                "Ctrl"
            },
            link.url
        ));
        if ui.input(|i| i.modifiers.command) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if output.response.clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(&link.url));
            }
        }
    }
}

struct ListPrefix<'a> {
    indent: &'a str,
    marker: &'a str,
    end: usize,
    number: Option<u64>,
}
impl<'a> ListPrefix<'a> {
    fn parse(line: &'a str) -> Option<Self> {
        let trimmed = line.trim_start_matches([' ', '\t']);
        let indent = &line[..line.len() - trimmed.len()];
        let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
        let (marker_len, number) = if digits > 0 && digits <= 9 {
            if !matches!(trimmed.as_bytes().get(digits), Some(b'.' | b')')) {
                return None;
            }
            (digits + 1, Some(trimmed[..digits].parse().ok()?))
        } else if trimmed.starts_with(['*', '-', '+']) {
            (1, None)
        } else {
            return None;
        };
        if !matches!(trimmed.as_bytes().get(marker_len), Some(b' ' | b'\t')) {
            return None;
        }
        let end = indent.len() + marker_len + 1;
        Some(Self {
            indent,
            marker: &trimmed[..marker_len],
            end,
            number,
        })
    }
    fn next(&self) -> String {
        if let Some(number) = self.number {
            format!(
                "{}{}{} ",
                self.indent,
                number + 1,
                self.marker.chars().last().unwrap()
            )
        } else {
            format!("{}{} ", self.indent, self.marker)
        }
    }
}

/// Let egui own each edit (including auto-continuation) so undo/redo remains
/// atomic. Pasted Markdown and undo replacements bypass smart-newline handling.
pub(super) struct Buffer<'a> {
    pub text: &'a mut String,
    pub smart_newline: bool,
}
impl TextBuffer for Buffer<'_> {
    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<Buffer<'static>>()
    }
    fn is_mutable(&self) -> bool {
        true
    }
    fn as_str(&self) -> &str {
        self.text
    }
    fn insert_text(&mut self, text: &str, index: usize) -> usize {
        self.text.insert_text(text, index)
    }
    fn delete_char_range(&mut self, range: Range<usize>) {
        self.text.delete_char_range(range);
    }
    fn replace_with(&mut self, text: &str) {
        self.text.replace_with(text);
    }
    fn insert_text_at(&mut self, cursor: &mut egui::text::CCursor, value: &str, limit: usize) {
        if self.smart_newline && value == "\n" {
            let byte = byte_index(self.text, cursor.index);
            let start = self.text[..byte].rfind('\n').map_or(0, |i| i + 1);
            let end = self.text[byte..]
                .find('\n')
                .map_or(self.text.len(), |i| byte + i);
            let literal = Parser::new(self.text).into_offset_iter().any(|(e, r)| {
                r.start <= byte
                    && match e {
                        // Unclosed fenced/indented blocks also include a caret at EOF.
                        Event::Start(Tag::CodeBlock(_)) => byte <= r.end,
                        Event::Code(_) | Event::Rule => byte < r.end,
                        _ => false,
                    }
            });
            if !literal {
                if let Some(prefix) = ListPrefix::parse(&self.text[start..end]) {
                    if byte >= start + prefix.end {
                        if self.text[start + prefix.end..end].trim().is_empty() {
                            let indent = prefix.indent.len();
                            self.text.replace_range(start + indent..end, "");
                            cursor.index = self.text[..start + indent].chars().count();
                        } else {
                            let next = format!("\n{}", prefix.next());
                            self.text.insert_text_at(cursor, &next, limit);
                        }
                        return;
                    }
                }
            }
        }
        self.text.insert_text_at(cursor, value, limit);
    }
}

#[cfg(test)]
mod tests;
