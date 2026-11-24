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
    underline: bool,
    block: bool,
    line_break: bool,
}
struct Link {
    range: Range<usize>,
    url: String,
}
struct Presentation {
    styles: Vec<Style>,
    links: Vec<Link>,
    blocks: Vec<Range<usize>>,
    lists: Vec<Range<usize>>,
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
            blocks: Vec::new(),
            lists: Vec::new(),
        };
        let parsed = parse_source(source);
        let events: Vec<_> = Parser::new(&parsed).into_offset_iter().collect();
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
                Event::Start(Tag::CodeBlock(kind)) => {
                    literal.push(range.clone());
                    result.blocks.push(range.clone());
                    result.apply(range.clone(), |s| {
                        s.code = true;
                        s.block = true;
                    });
                    if matches!(kind, pulldown_cmark::CodeBlockKind::Fenced(_)) {
                        let value = &source[range.clone()];
                        let opening_end = value.find('\n').unwrap_or(value.len());
                        let opening = range.start..range.start + opening_end;
                        let trimmed = value.trim_end_matches(['\n', '\r']);
                        let closing_start = trimmed.rfind('\n').map(|i| i + 1);
                        let marker = value.trim_start().as_bytes().first().copied();
                        let opening_width = value
                            .trim_start()
                            .bytes()
                            .take_while(|b| Some(*b) == marker)
                            .count();
                        if !editing(&opening, selection.clone()) {
                            result.hide(opening);
                        }
                        if let Some(start) = closing_start {
                            let line = trimmed[start..].trim();
                            if line.len() >= opening_width
                                && line.bytes().all(|b| Some(b) == marker)
                            {
                                let closing = range.start + start..range.start + trimmed.len();
                                if !editing(&closing, selection.clone()) {
                                    result.hide(closing);
                                }
                            }
                        }
                    }
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
                    if ticks >= 3 {
                        result.apply(range.clone(), |s| s.block = true);
                        result.blocks.push(range.clone());
                        // A one-line triple-backtick snippet still occupies its own
                        // visual block. One display character per source character
                        // keeps copy/undo and cursor indices in the original draft.
                        if range.start > 0 && !source[..range.start].ends_with('\n') {
                            result.styles[range.start].line_break = true;
                        }
                        if range.end < source.len() && !source[range.end..].starts_with('\n') {
                            result.styles[range.end - 1].line_break = true;
                        }
                    }
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
        for range in commands::underline_ranges(source) {
            result.apply(range.clone(), |s| s.underline = true);
            if !editing(&range, selection.clone()) {
                result.hide(range.start..range.start + 3);
                result.hide(range.end - 4..range.end);
            }
        }
        // Recognize a marker as soon as its following space is typed, even if
        // CommonMark has not yet emitted a nonempty list item.
        let mut offset = 0;
        for line in source.split_inclusive('\n') {
            if let Some(prefix) = ListPrefix::parse(line) {
                if !literal
                    .iter()
                    .any(|r| r.contains(&(offset + prefix.indent.len())))
                {
                    result.lists.push(offset..offset + line.len());
                    if prefix.number.is_none() && prefix.letter.is_none() {
                        result.styles[offset + prefix.indent.len()].bullet = true;
                    }
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

// Explicit IDs shared by the regular composer and its embedded instance.
// Only the dock renderer installs this scope; outside it, existing IDs stay stable.
thread_local! {
    // UI-thread scope rather than Context storage: widget IDs are also resolved
    // inside egui memory/data closures, where re-locking Context would deadlock.
    static OWNER: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}
pub(super) fn set_owner(owner: Option<String>) -> Option<String> {
    OWNER.with(|value| value.replace(owner))
}
pub(super) fn widget_id(_ctx: &egui::Context, label: &str) -> egui::Id {
    OWNER.with(|value| match value.borrow().as_deref() {
        Some(owner) if label == "composer" => egui::Id::new(("btw-editor", owner)),
        Some(owner) => egui::Id::new(("side-chat", owner, label)),
        None => egui::Id::new(label),
    })
}

fn selection(ctx: &egui::Context, source: &str) -> Option<Range<usize>> {
    if !ctx.memory(|m| m.has_focus(composer_text::widget_id(ctx, "composer"))) {
        return None;
    }
    let state = egui::TextEdit::load_state(ctx, composer_text::widget_id(ctx, "composer"))?;
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
    let inset = if presentation.lists.is_empty() && presentation.blocks.is_empty() {
        0.0
    } else {
        12.0
    };
    display.wrap.max_width = (width - inset * 2.0).max(24.0);
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
            } else if style.block {
                colors.code_text
            } else if style.code {
                colors.inline_code_text
            } else {
                colors.text
            },
            line_height: style.block.then_some(28.0),
            background: if style.code && !style.block && !style.hidden {
                colors.inline_code_bg
            } else {
                Color32::TRANSPARENT
            },
            italics: style.italic,
            underline: if style.underline {
                Stroke::new(1.0, colors.text)
            } else if style.link && !style.hidden {
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
                if style.line_break {
                    '\n'
                } else if style.hidden && c != '\n' {
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
    let galley_mut = Arc::make_mut(&mut galley);
    galley_mut.job = Arc::new(original);
    let mut index = 0;
    for row in &mut galley_mut.rows {
        let start = byte_index(source, index);
        let end = byte_index(source, index + row.char_count_excluding_newline());
        if presentation
            .blocks
            .iter()
            .any(|r| r.start < end && start < r.end)
            || presentation
                .lists
                .iter()
                .any(|r| r.start <= start && start < r.end)
        {
            row.pos.x += inset;
        }
        index += row.char_count_including_newline();
    }
    galley_mut.rect.max.x += inset;
    galley_mut.mesh_bounds.max.x += inset;
    theme::text_direction::align_where(&mut galley, (width - inset).max(24.0), |range| {
        let start = byte_index(source, range.start);
        let end = byte_index(source, range.end);
        !presentation
            .blocks
            .iter()
            .any(|block| block.start < end && start < block.end)
    });
    galley
}

pub(super) fn list_at_cursor(ctx: &egui::Context, source: &str) -> bool {
    let Some(selection) = selection(ctx, source) else {
        return false;
    };
    let start = source[..selection.start].rfind('\n').map_or(0, |i| i + 1);
    Presentation::new(source, None)
        .lists
        .iter()
        .any(|r| r.contains(&start))
}

pub(super) fn decorate(
    ui: &egui::Ui,
    placeholder: egui::layers::ShapeIdx,
    output: &TextEditOutput,
    source: &str,
) {
    let presentation = Presentation::new(source, None);
    let colors = theme::palette(ui.ctx());
    let mut shapes = Vec::new();
    for block in &presentation.blocks {
        let start = source[..block.start].chars().count();
        let end = source[..block.end].chars().count();
        let mut offset = 0;
        let mut rect = egui::Rect::NOTHING;
        for row in &output.galley.rows {
            let next = offset + row.char_count_including_newline();
            let visible_end = offset + row.char_count_excluding_newline();
            if offset < end && visible_end > start {
                rect = rect.union(row.rect());
            }
            offset = next;
        }
        if rect.is_finite() {
            rect.min.x = 4.0;
            rect.max.x = output.response.rect.width() - 4.0;
            let rect = rect.translate(output.galley_pos.to_vec2());
            shapes.push(egui::Shape::rect_filled(rect, 6, colors.code_bg));
            shapes.push(egui::Shape::rect_stroke(
                rect,
                6,
                Stroke::new(1.0, colors.border),
                egui::StrokeKind::Inside,
            ));
        }
    }
    ui.painter().set(placeholder, egui::Shape::Vec(shapes));
}

impl Pui {
    pub(super) fn composer_formatting_available(&self, ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.has_focus(composer_text::widget_id(ctx, "composer")))
            && !self.settings_open
            && !self.kanban.open
            && !self.automation.open
            && !self.search_open
            && !self.model_picker_open
            && !self.effort_picker_open
            && !self.menu.about
            && !self.updates.open
            && !self.agents.close_prompt
            && !self.deletion.confirming()
            && !self.section_view.confirming()
            && !self.history_view.is_open()
            && self.state.dialogs.is_empty()
            && !self.transcript.gallery.is_open()
            && !self.draft_images.gallery.is_open()
    }
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
    letter: Option<u8>,
}
impl<'a> ListPrefix<'a> {
    fn parse(line: &'a str) -> Option<Self> {
        let trimmed = line.trim_start_matches([' ', '\t']);
        let indent = &line[..line.len() - trimmed.len()];
        let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
        let letter = (trimmed
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
            && matches!(trimmed.as_bytes().get(1), Some(b'.' | b')')))
        .then(|| trimmed.as_bytes()[0]);
        let (marker_len, number) = if letter.is_some() {
            (2, None)
        } else if digits > 0 && digits <= 9 {
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
            letter,
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
        } else if let Some(letter) = self.letter {
            let base = if letter.is_ascii_uppercase() {
                b'A'
            } else {
                b'a'
            };
            let next = (base + (letter - base + 1) % 26) as char;
            format!(
                "{}{next}{} ",
                self.indent,
                self.marker.chars().last().unwrap()
            )
        } else {
            format!("{}{} ", self.indent, self.marker)
        }
    }
}

// CommonMark has no lettered lists. Substitute an equal-length numeric marker
// for parsing only; every stored/displayed/copied character remains untouched.
fn parse_source(source: &str) -> std::borrow::Cow<'_, str> {
    let mut parsed = std::borrow::Cow::Borrowed(source);
    let fenced: Vec<_> = Parser::new(source)
        .into_offset_iter()
        .filter_map(|(event, range)| {
            matches!(
                event,
                Event::Start(Tag::CodeBlock(pulldown_cmark::CodeBlockKind::Fenced(_)))
            )
            .then_some(range)
        })
        .collect();
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        if let Some(prefix) =
            ListPrefix::parse(line).filter(|_| !fenced.iter().any(|r| r.contains(&offset)))
        {
            // A nested item remains a list while editing even before a parent
            // exists. Quotes preserve byte offsets without CommonMark mistaking
            // four-space list indentation for an indented code block.
            for index in 0..prefix.indent.len() {
                parsed
                    .to_mut()
                    .replace_range(offset + index..offset + index + 1, ">");
            }
            if prefix.letter.is_some() {
                let start = offset + prefix.indent.len();
                parsed.to_mut().replace_range(start..start + 1, "1");
            }
        }
        offset += line.len();
    }
    parsed
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
            let parsed = parse_source(self.text);
            let literal = Parser::new(&parsed).into_offset_iter().any(|(e, r)| {
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

mod commands;
pub(super) use commands::{Format, format, keys};

#[cfg(test)]
mod editing_tests;
#[cfg(test)]
mod scroll_tests;
#[cfg(test)]
mod tests;
