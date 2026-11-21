//! Selectable Markdown with tables, one consistent baseline and real link hit targets.
mod capture;
mod reply;
#[cfg(test)]
mod reply_tests;
mod table;
use crate::theme;
use eframe::egui::{self, Color32, FontId, RichText, TextFormat, Ui, text::LayoutJob};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone)]
pub struct FindHighlight {
    pub query: String,
    pub active_in_scope: Option<usize>,
    pub seen: Arc<AtomicUsize>,
    pub active_rect: Arc<Mutex<Option<egui::Rect>>>,
}

fn find_highlight_id() -> egui::Id {
    egui::Id::new("pui-markdown-find-highlight")
}

pub fn set_find_highlight(ctx: &egui::Context, highlight: Option<FindHighlight>) {
    ctx.data_mut(|d| {
        if let Some(h) = highlight {
            d.insert_temp(find_highlight_id(), h);
        } else {
            d.remove::<FindHighlight>(find_highlight_id());
        }
    });
}

pub fn find_case_insensitive_ranges(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    if query.is_empty() || text.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if query.is_ascii() {
        let qlen = query.len();
        let mut i = 0;
        while i + qlen <= text.len() {
            if let Some(slice) = text.get(i..i + qlen)
                && slice.eq_ignore_ascii_case(query)
            {
                out.push(i..i + qlen);
                i += qlen;
                continue;
            }
            if let Some(ch) = text[i..].chars().next() {
                i += ch.len_utf8();
            } else {
                break;
            }
        }
        return out;
    }
    let q_lower = query.to_lowercase();
    let mut lowered = String::with_capacity(text.len());
    let mut byte_map = Vec::with_capacity(text.len());
    for (orig_idx, ch) in text.char_indices() {
        for lc in ch.to_lowercase() {
            let start = lowered.len();
            lowered.push(lc);
            for _ in start..lowered.len() {
                byte_map.push(orig_idx);
            }
        }
    }
    byte_map.push(text.len());
    let mut pos = 0;
    while let Some(found) = lowered[pos..].find(&q_lower) {
        let l_start = pos + found;
        let l_end = l_start + q_lower.len();
        out.push(byte_map[l_start]..byte_map[l_end]);
        pos = l_end;
    }
    out
}

/// Apply active chat-find highlights to `job`, returning the byte offset of the active match if present.
pub fn highlight_job(ui: &Ui, job: &mut LayoutJob) -> Option<usize> {
    let state = ui
        .ctx()
        .data(|d| d.get_temp::<FindHighlight>(find_highlight_id()))?;
    let ranges = find_case_insensitive_ranges(&job.text, &state.query);
    if ranges.is_empty() {
        return None;
    }
    let dark = ui.ctx().style().visuals.dark_mode;
    let active_bg = Color32::from_rgb(234, 138, 30);
    let active_fg = Color32::WHITE;
    let other_bg = if dark {
        Color32::from_rgba_unmultiplied(234, 179, 8, 95)
    } else {
        Color32::from_rgba_unmultiplied(250, 204, 21, 145)
    };
    let other_fg = if dark {
        Color32::from_rgb(255, 240, 180)
    } else {
        Color32::BLACK
    };
    let mut active_byte = None;
    let tagged: Vec<(std::ops::Range<usize>, bool)> = ranges
        .into_iter()
        .map(|r| {
            let idx = state.seen.fetch_add(1, Ordering::Relaxed);
            let is_active = state.active_in_scope == Some(idx);
            if is_active {
                active_byte = Some(r.start);
            }
            (r, is_active)
        })
        .collect();
    let old_sections = std::mem::take(&mut job.sections);
    for section in old_sections {
        let s_start = section.byte_range.start;
        let s_end = section.byte_range.end;
        let mut cursor = s_start;
        for (r, is_active) in &tagged {
            if r.end <= cursor || r.start >= s_end {
                continue;
            }
            let o_start = r.start.max(cursor);
            let o_end = r.end.min(s_end);
            if o_start > cursor {
                let mut prefix = section.clone();
                prefix.byte_range = cursor..o_start;
                job.sections.push(prefix);
            }
            let mut matched = section.clone();
            matched.byte_range = o_start..o_end;
            matched.format.background = if *is_active { active_bg } else { other_bg };
            matched.format.color = if *is_active { active_fg } else { other_fg };
            job.sections.push(matched);
            cursor = o_end;
        }
        if cursor < s_end {
            let mut tail = section;
            tail.byte_range = cursor..s_end;
            job.sections.push(tail);
        }
    }
    active_byte
}

pub fn record_find_match_rect(
    ui: &Ui,
    galley: &egui::Galley,
    origin: egui::Pos2,
    byte_start: usize,
) {
    let Some(state) = ui
        .ctx()
        .data(|d| d.get_temp::<FindHighlight>(find_highlight_id()))
    else {
        return;
    };
    let char_idx = galley
        .job
        .text
        .get(..byte_start)
        .map_or(0, |s| s.chars().count());
    let local = galley.pos_from_cursor(egui::text::CCursor::new(char_idx));
    let rect = local.translate(origin.to_vec2());
    if let Ok(mut slot) = state.active_rect.lock() {
        *slot = Some(rect);
    }
}
use table::Table;
#[derive(Default)]
struct ParseMarkdown;
impl egui::cache::ComputerMut<&str, Arc<Vec<Event<'static>>>> for ParseMarkdown {
    fn compute(&mut self, source: &str) -> Arc<Vec<Event<'static>>> {
        Arc::new(
            Parser::new_ext(source, Options::ENABLE_TABLES | Options::ENABLE_GFM)
                .map(Event::into_static)
                .collect(),
        )
    }
}
type MarkdownCache = egui::cache::FrameCache<Arc<Vec<Event<'static>>>, ParseMarkdown>;
struct Link {
    range: std::ops::Range<usize>,
    url: String,
}
/// Splits a local file link into `(path, optional_1_based_line)`.
/// Supports `path:42`, `path:42:10`, `path#L42`, `path#L42-L60`, and `path#42`.
pub fn split_file_line(value: &str) -> (&str, Option<usize>) {
    if let Some((base, frag)) = value.split_once('#') {
        let trimmed = frag
            .strip_prefix('L')
            .or_else(|| frag.strip_prefix('l'))
            .unwrap_or(frag);
        let digits = trimmed.split('-').next().unwrap_or("").trim();
        if let Ok(line) = digits.parse::<usize>()
            && line > 0
        {
            return (base, Some(line));
        }
    }
    if let Some((prefix, last)) = value.rsplit_once(':')
        && let Ok(num) = last.parse::<usize>()
        && num > 0
    {
        // Handle `path:line:col` as well as `path:line`.
        if let Some((path, line_str)) = prefix.rsplit_once(':')
            && let Ok(line) = line_str.parse::<usize>()
            && line > 0
        {
            return (path, Some(line));
        }
        return (prefix, Some(num));
    }
    (value, None)
}

fn known_bare_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "rs" | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "mjs"
            | "cjs"
            | "py"
            | "md"
            | "markdown"
            | "json"
            | "toml"
            | "yaml"
            | "yml"
            | "html"
            | "htm"
            | "css"
            | "scss"
            | "svg"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "sh"
            | "zsh"
            | "bash"
            | "txt"
            | "csv"
            | "sql"
            | "xml"
            | "c"
            | "h"
            | "cc"
            | "cpp"
            | "hpp"
            | "go"
            | "java"
            | "kt"
            | "swift"
            | "rb"
            | "php"
            | "lock"
    )
}

fn local_file(value: &str) -> bool {
    if value.is_empty()
        || value.chars().any(char::is_whitespace)
        || value.contains("://")
        || value.starts_with('#')
    {
        return false;
    }
    let (base, _) = split_file_line(value);
    if base.is_empty() || base.contains(':') {
        return false;
    }
    let Some(ext) = std::path::Path::new(base)
        .extension()
        .and_then(|e| e.to_str())
    else {
        return false;
    };
    if base.starts_with('/') || base.starts_with("./") || base.starts_with("~/") {
        return !ext.is_empty()
            && ext.len() <= 12
            && ext.chars().all(|c| c.is_ascii_alphanumeric());
    }
    if base.contains('/') {
        return !base.starts_with("http")
            && !ext.is_empty()
            && ext.len() <= 12
            && ext.chars().all(|c| c.is_ascii_alphanumeric());
    }
    known_bare_extension(ext)
}

fn safe_url(url: &str) -> bool {
    url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("mailto:")
        || url.starts_with("file://")
        || local_file(url)
        || {
            let (base, _) = split_file_line(url);
            !base.contains(':')
                && !base.starts_with('#')
                && std::path::Path::new(base).extension().is_some()
        }
}

pub fn show_file(ui: &mut Ui, source: &str, path: &std::path::Path) {
    let id = egui::Id::new("markdown-file-base");
    ui.ctx()
        .data_mut(|data| data.insert_temp(id, path.parent().unwrap_or(path).to_path_buf()));
    capture::scope(
        ui,
        ("markdown-file", path),
        format!("Markdown preview {}", path.display()),
        |ui| show(ui, source),
    );
    ui.ctx()
        .data_mut(|data| data.remove::<std::path::PathBuf>(id));
}

pub fn show_reply(ui: &mut Ui, source: &str, identity: impl std::hash::Hash) {
    capture::scope(
        ui,
        ("assistant-reply", identity),
        "Assistant reply".into(),
        |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let events = ui
                .ctx()
                .memory_mut(|m| m.caches.cache::<MarkdownCache>().get(source));
            show_events(ui, &events, true, None);
        },
    );
}
fn flush(ui: &mut Ui, job: &mut LayoutJob, links: &mut Vec<Link>) {
    if job.text.is_empty() {
        return;
    }
    job.wrap.max_width = ui.available_width();
    job.align_baseline = true;
    let active = highlight_job(ui, job);
    let galley = ui.fonts(|f| f.layout_job(std::mem::take(job)));
    show_galley_with_find(ui, galley, links, active);
    links.clear();
}
fn show_galley(ui: &mut Ui, galley: Arc<egui::Galley>, links: &[Link]) {
    show_galley_with_find(ui, galley, links, None);
}
fn show_galley_with_find(
    ui: &mut Ui,
    galley: Arc<egui::Galley>,
    links: &[Link],
    active_byte: Option<usize>,
) {
    let response = ui.add(egui::Label::new(galley.clone()).wrap().selectable(true));
    capture::offer(ui, &response, &galley.job.text);
    let origin = match galley.job.halign {
        egui::Align::Min => response.rect.left_top(),
        egui::Align::Center => response.rect.center_top(),
        egui::Align::Max => response.rect.right_top(),
    };
    if let Some(byte_start) = active_byte {
        record_find_match_rect(ui, &galley, origin, byte_start);
    }
    if let Some(pos) = response.hover_pos() {
        let cursor = galley.cursor_from_pos(pos - origin);
        let byte = galley
            .job
            .text
            .char_indices()
            .nth(cursor.index)
            .map(|(i, _)| i)
            .unwrap_or(galley.job.text.len());
        if let Some(link) = links
            .iter()
            .find(|l| l.range.contains(&byte) && safe_url(&l.url))
        {
            response
                .clone()
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(format!(
                    "{}{}",
                    link.url,
                    crate::pulls::inline(ui.ctx(), &link.url).unwrap_or_default()
                ));
            if response.clicked() {
                let mut target = link.url.clone();
                if !target.contains(':') {
                    let base = ui.ctx().data(|data| {
                        data.get_temp::<std::path::PathBuf>(egui::Id::new("markdown-file-base"))
                    });
                    if let Some(base) = base {
                        target = url::Url::from_file_path(base.join(&target))
                            .map(|url| url.to_string())
                            .unwrap_or(target);
                    }
                }
                ui.ctx().open_url(egui::OpenUrl::new_tab(target));
            }
        }
    }
}
fn format(
    ctx: &egui::Context,
    bold: bool,
    italic: bool,
    heading: bool,
    code: bool,
    link: bool,
) -> TextFormat {
    let colors = crate::theme::palette(ctx);
    // Inline code shares the surrounding text's size; a smaller monospace font
    // has a shorter ascent and looks raised even with the same line height.
    let size = if heading { 17.0 } else { 14.0 };
    TextFormat {
        font_id: if code {
            FontId::monospace(size)
        } else {
            FontId::new(
                size,
                if bold || heading {
                    egui::FontFamily::Name("body-bold".into())
                } else {
                    egui::FontFamily::Proportional
                },
            )
        },
        color: if link {
            colors.accent
        } else if code {
            colors.inline_code_text
        } else {
            colors.text
        },
        background: if code {
            colors.inline_code_bg
        } else {
            Color32::TRANSPARENT
        },
        italics: italic,
        line_height: Some(22.0),
        valign: egui::Align::Center,
        ..Default::default()
    }
}
pub fn show(ui: &mut Ui, source: &str) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        show_inner(ui, source);
    });
}

fn show_inner(ui: &mut Ui, source: &str) {
    let events = ui
        .ctx()
        .memory_mut(|m| m.caches.cache::<MarkdownCache>().get(source));
    show_events(ui, &events, false, None);
}

fn show_events(ui: &mut Ui, events: &[Event<'static>], reply: bool, mut item: Option<reply::Item>) {
    let colors = crate::theme::palette(ui.ctx());
    let text_format = |ctx: &egui::Context, bold, italic, heading, code, link| {
        let mut format = format(ctx, bold, italic, heading, code, link);
        if reply {
            format.line_height = Some(21.0);
            if code {
                // A neutral wash, not a second card or an accent-colored pill.
                format.background = if ctx.style().visuals.dark_mode {
                    Color32::from_white_alpha(6)
                } else {
                    Color32::from_black_alpha(8)
                };
            }
        }
        format
    };
    let top_level = item.is_none();
    let mut flush = |ui: &mut Ui, job: &mut LayoutJob, links: &mut Vec<Link>| {
        if let Some(item) = &mut item {
            item.flush(ui, job, links);
        } else {
            flush(ui, job, links);
        }
    };
    let mut job = LayoutJob::default();
    let mut links = Vec::new();
    let (mut bold, mut italic, mut heading) = (false, false, false);
    let mut link: Option<String> = None;
    let mut code = false;
    let mut code_text = String::new();
    let mut language = String::new();
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut in_item = false;
    let mut table: Option<Table> = None;
    let mut table_header = false;
    let mut event_index = 0;
    while event_index < events.len() {
        let event = events[event_index].clone();
        event_index += 1;
        if reply && matches!(event, Event::Start(Tag::List(_))) {
            flush(ui, &mut job, &mut links);
            let start = event_index - 1;
            let mut depth = 1;
            while event_index < events.len() {
                match events[event_index] {
                    Event::Start(Tag::List(_)) => depth += 1,
                    Event::End(TagEnd::List(_)) => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                event_index += 1;
            }
            reply::list(ui, &events[start..event_index], top_level);
            event_index = (event_index + 1).min(events.len());
            continue;
        }
        if let Event::Start(Tag::BlockQuote(Some(kind))) = event {
            flush(ui, &mut job, &mut links);
            let start = event_index;
            let mut depth = 1;
            while event_index < events.len() {
                match &events[event_index] {
                    Event::Start(Tag::BlockQuote(_)) => depth += 1,
                    Event::End(TagEnd::BlockQuote(_)) => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                event_index += 1;
            }
            let body = &events[start..event_index];
            alert(ui, kind, body);
            event_index = (event_index + 1).min(events.len());
            continue;
        }
        match event {
            Event::Start(Tag::Table(alignments)) => {
                flush(ui, &mut job, &mut links);
                table = Some(Table::new(alignments));
            }
            Event::Start(Tag::TableHead) => table_header = true,
            Event::End(TagEnd::TableHead) => table_header = false,
            Event::Start(Tag::TableRow) => {
                if let Some(table) = &mut table {
                    table.start_row();
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(table) = &mut table {
                    table.push_cell(std::mem::take(&mut job), std::mem::take(&mut links));
                }
            }
            Event::End(TagEnd::Table) => {
                if let Some(table) = table.take() {
                    table.show(ui);
                    ui.add_space(6.0);
                }
            }
            Event::Start(Tag::Strong) => bold = true,
            Event::End(TagEnd::Strong) => bold = false,
            Event::Start(Tag::Emphasis) => italic = true,
            Event::End(TagEnd::Emphasis) => italic = false,
            Event::Start(Tag::Heading { .. }) => {
                flush(ui, &mut job, &mut links);
                heading = true;
            }
            Event::End(TagEnd::Heading(_)) => {
                flush(ui, &mut job, &mut links);
                heading = false;
                ui.add_space(4.0);
            }
            Event::Start(Tag::Link { dest_url, .. }) => link = Some(dest_url.into_string()),
            Event::End(TagEnd::Link) => link = None,
            Event::Start(Tag::CodeBlock(kind)) => {
                flush(ui, &mut job, &mut links);
                code = true;
                language = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(l) => l.to_string(),
                    _ => String::new(),
                };
            }
            Event::End(TagEnd::CodeBlock) => {
                egui::Frame::new()
                    .fill(colors.code_bg)
                    .stroke(egui::Stroke::new(1.0, colors.border))
                    .corner_radius(8)
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(theme::small(ui, &language));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if crate::icons::button(
                                        ui,
                                        crate::icons::Icon::Copy,
                                        "Copy code",
                                    )
                                    .clicked()
                                    {
                                        ui.ctx().copy_text(code_text.clone());
                                    }
                                },
                            );
                        });
                        egui::ScrollArea::horizontal()
                            .id_salt(ui.next_auto_id())
                            .show(ui, |ui| {
                                let text = code_text.trim_end();
                                let response = ui.add(
                                    egui::Label::new(
                                        RichText::new(text)
                                            .font(FontId::monospace(12.0))
                                            .color(colors.code_text),
                                    )
                                    .selectable(true),
                                );
                                capture::offer(ui, &response, text);
                            });
                    });
                code_text.clear();
                code = false;
                ui.add_space(4.0);
            }
            Event::Start(Tag::List(start)) => {
                flush(ui, &mut job, &mut links);
                lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                lists.pop();
                in_item = false;
                ui.add_space(4.0);
            }
            Event::Start(Tag::Item) => {
                flush(ui, &mut job, &mut links);
                in_item = true;
                let marker = if let Some(Some(number)) = lists.last_mut() {
                    let s = format!("{number}. ");
                    *number += 1;
                    s
                } else {
                    "•  ".into()
                };
                job.append(
                    &marker,
                    0.0,
                    text_format(ui.ctx(), false, false, false, false, false),
                );
            }
            Event::End(TagEnd::Item) => {
                flush(ui, &mut job, &mut links);
                in_item = false;
            }
            Event::End(TagEnd::Paragraph) => {
                if !in_item {
                    flush(ui, &mut job, &mut links);
                    ui.add_space(4.0);
                }
            }
            Event::Text(text) if code => code_text.push_str(&text),
            Event::Code(text) => {
                let start = job.text.len();
                job.append(
                    &text,
                    0.0,
                    text_format(
                        ui.ctx(),
                        false,
                        false,
                        heading,
                        true,
                        link.is_some() || local_file(&text),
                    ),
                );
                if let Some(url) = link
                    .as_deref()
                    .or_else(|| local_file(&text).then_some(text.as_ref()))
                {
                    links.push(Link {
                        range: start..job.text.len(),
                        url: url.into(),
                    });
                }
            }
            Event::Text(mut text) => {
                if let Some(url) = &link {
                    if let Some(symbol) = crate::pulls::symbol(ui.ctx(), url) {
                        text = pulldown_cmark::CowStr::from(format!("{text} {symbol}"));
                    }
                }
                let start = job.text.len();
                job.append(
                    &text,
                    0.0,
                    text_format(
                        ui.ctx(),
                        bold || table_header,
                        italic,
                        heading,
                        false,
                        link.is_some(),
                    ),
                );
                if let Some(url) = &link {
                    links.push(Link {
                        range: start..job.text.len(),
                        url: url.clone(),
                    });
                } else {
                    for (offset, word) in
                        text.split_inclusive(char::is_whitespace)
                            .scan(0, |offset, word| {
                                let o = *offset;
                                *offset += word.len();
                                Some((o, word))
                            })
                    {
                        let url = word.trim().trim_end_matches(['.', ',', ')', ']', ';']);
                        if url.starts_with("https://")
                            || url.starts_with("http://")
                            || url.starts_with("mailto:")
                            || local_file(url)
                        {
                            links.push(Link {
                                range: start + offset..start + offset + url.len(),
                                url: url.into(),
                            });
                        }
                    }
                }
            }
            Event::SoftBreak => job.append(
                " ",
                0.0,
                text_format(ui.ctx(), bold, italic, heading, false, link.is_some()),
            ),
            Event::HardBreak => job.append(
                "\n",
                0.0,
                text_format(ui.ctx(), bold, italic, heading, false, link.is_some()),
            ),
            Event::Rule => {
                flush(ui, &mut job, &mut links);
                ui.separator();
            }
            _ => {}
        }
    }
    flush(ui, &mut job, &mut links);
}

fn alert(ui: &mut Ui, kind: pulldown_cmark::BlockQuoteKind, events: &[Event<'static>]) {
    use pulldown_cmark::BlockQuoteKind;
    let colors = theme::palette(ui.ctx());
    let (title, tint) = match kind {
        BlockQuoteKind::Note => ("Note", colors.blue),
        BlockQuoteKind::Tip => ("Tip", colors.green),
        BlockQuoteKind::Important => ("Important", colors.accent),
        BlockQuoteKind::Warning => ("Warning", colors.amber),
        BlockQuoteKind::Caution => ("Caution", colors.red),
    };
    egui::Frame::new()
        .fill(tint.gamma_multiply(0.08))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.set_width((ui.available_width() - 2.0).max(0.0));
            ui.label(RichText::new(title).size(12.0).strong().color(tint));
            let mut job = LayoutJob::default();
            let mut links = Vec::new();
            let (mut bold, mut italic) = (false, false);
            let mut link = None;
            for event in events {
                match event {
                    Event::Start(Tag::Strong) => bold = true,
                    Event::End(TagEnd::Strong) => bold = false,
                    Event::Start(Tag::Emphasis) => italic = true,
                    Event::End(TagEnd::Emphasis) => italic = false,
                    Event::Start(Tag::Link { dest_url, .. }) => link = Some(dest_url.to_string()),
                    Event::End(TagEnd::Link) => link = None,
                    Event::Text(text) | Event::Code(text) => {
                        let start = job.text.len();
                        job.append(
                            text,
                            0.0,
                            format(
                                ui.ctx(),
                                bold,
                                italic,
                                false,
                                matches!(event, Event::Code(_)),
                                link.is_some(),
                            ),
                        );
                        if let Some(url) = &link {
                            links.push(Link {
                                range: start..job.text.len(),
                                url: url.clone(),
                            });
                        }
                    }
                    Event::SoftBreak => job.append(
                        " ",
                        0.0,
                        format(ui.ctx(), false, false, false, false, false),
                    ),
                    Event::HardBreak | Event::End(TagEnd::Paragraph) => {
                        flush(ui, &mut job, &mut links);
                    }
                    _ => {}
                }
            }
            flush(ui, &mut job, &mut links);
        });
    ui.add_space(4.0);
}

pub fn diff(ui: &mut Ui, source: &str) {
    let colors = crate::theme::palette(ui.ctx());
    let mut job = LayoutJob::default();
    let mut backgrounds = Vec::new();
    for line in source.split_inclusive('\n') {
        backgrounds.push(if line.starts_with('+') {
            Some(colors.diff_add_bg)
        } else if line.starts_with('-') {
            Some(colors.diff_remove_bg)
        } else {
            None
        });
        let color = if line.starts_with('+') {
            colors.green
        } else if line.starts_with('-') {
            colors.red
        } else if line.starts_with("@@") {
            colors.accent
        } else {
            colors.diff_text
        };
        job.append(
            &format!(
                " {}",
                if line.starts_with("@@ recorded replacement") {
                    "…\n"
                } else {
                    line
                }
            ),
            0.0,
            TextFormat {
                font_id: FontId::monospace(12.0),
                color,
                line_height: Some(19.0),
                ..Default::default()
            },
        );
    }
    let active = highlight_job(ui, &mut job);
    let background = ui.painter().add(egui::Shape::Noop);
    let galley = ui.fonts(|f| f.layout_job(job));
    let response = ui.add(egui::Label::new(galley.clone()).selectable(true).extend());
    if let Some(byte_start) = active {
        record_find_match_rect(ui, &galley, response.rect.left_top(), byte_start);
    }
    let width = response.rect.width().max(ui.available_width());
    let shapes = backgrounds
        .into_iter()
        .enumerate()
        .filter_map(|(index, color)| {
            color.map(|color| {
                egui::Shape::rect_filled(
                    egui::Rect::from_min_size(
                        response.rect.min + egui::vec2(0.0, index as f32 * 19.0),
                        egui::vec2(width, 19.0),
                    ),
                    0,
                    color,
                )
            })
        })
        .collect();
    ui.painter().set(background, egui::Shape::Vec(shapes));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::cache::ComputerMut;
    #[test]
    fn github_callouts_render_titles_without_raw_markers_or_execution() {
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            mode.apply(&ctx);
            for (marker, title) in [
                ("NOTE", "Note"),
                ("TIP", "Tip"),
                ("IMPORTANT", "Important"),
                ("WARNING", "Warning"),
                ("CAUTION", "Caution"),
            ] {
                let source = format!(
                    "> [!{marker}]\n> Keep **important** text and `code` readable.\n\nAfter alert."
                );
                let (texts, _) = render(&ctx, &source, 260.0);
                let combined = texts
                    .iter()
                    .map(|t| t.galley.job.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                assert!(combined.contains(title));
                assert!(
                    combined.contains("important")
                        && combined.contains("code")
                        && combined.contains("After alert.")
                );
                assert!(!combined.contains("[!"));
            }
        }
    }
    #[test]
    fn inline_code_and_prose_share_a_baseline() {
        for system_fonts in [false, true] {
            for scale in [1.0, 2.0] {
                let ctx = egui::Context::default();
                if system_fonts {
                    theme::install(&ctx);
                } else {
                    let mut fonts = egui::FontDefinitions::default();
                    fonts.families.insert(
                        egui::FontFamily::Name("body-bold".into()),
                        fonts.families[&egui::FontFamily::Proportional].clone(),
                    );
                    ctx.set_fonts(fonts);
                }
                ctx.set_pixels_per_point(scale);
                for source in [
                    "Switch to `master`, delete `main`, and keep other branches unchanged.",
                    "- `/Users/user/personal/pui` is now on `master`.",
                    "## Switch to `master` now",
                    "**Switch to `master` now**",
                ] {
                    for width in [230.0, 700.0] {
                        let (texts, _) = render(&ctx, source, width);
                        for text in texts {
                            for row in &text.galley.rows {
                                let baseline = row.glyphs[0].pos.y;
                                for glyph in &row.glyphs {
                                    assert!(
                                        (glyph.pos.y - baseline).abs() <= 1.0,
                                        "Inline glyph {:?} must share the prose baseline: {} != {baseline} ({source}, system={system_fonts}, scale={scale})",
                                        glyph.chr,
                                        glyph.pos.y,
                                    );
                                }
                                assert_eq!(row.height(), 22.0);
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn bold_links_italics_and_code_share_exact_baselines() {
        let source = "**Status:** [#69](https://example.com/pr/69) is merged, but this linked scan still contains the finding. It does **not prove an exploitable bypass**, nor does \"Passed\" prove resolution. The post-merge release is queued; *successful deployment* is not yet `verified`.";
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            ctx.set_pixels_per_point(scale);
            for markdown in [
                source.to_owned(),
                format!("- {source}"),
                format!("## {source}"),
                format!("| Result |\n| --- |\n| {source} |"),
                "שלום **מודגש** [קישור](https://example.com) *נטוי* `code` English".into(),
            ] {
                for width in [260.0, 760.0] {
                    let (texts, _) = render(&ctx, &markdown, width);
                    assert!(!texts.is_empty());
                    for text in texts {
                        for row in &text.galley.rows {
                            let Some(first) = row.glyphs.first() else {
                                continue;
                            };
                            for glyph in &row.glyphs {
                                assert_eq!(
                                    glyph.logical_rect().y_range(),
                                    first.logical_rect().y_range(),
                                    "Mixed style backgrounds and selection must share a row"
                                );
                                assert!(
                                    (glyph.pos.y - first.pos.y).abs() < 0.01,
                                    "Mixed-format glyph {:?} baseline {} != {} at scale {scale}, width {width}: {markdown}",
                                    glyph.chr,
                                    glyph.pos.y,
                                    first.pos.y
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn baseline_flag_is_cached_and_retained_when_paragraphs_are_split() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let mut job = LayoutJob::default();
            for _ in 0..3 {
                job.append(
                    "Status ",
                    0.0,
                    format(ctx, false, false, false, false, false),
                );
                job.append("bold ", 0.0, format(ctx, true, false, false, false, false));
                job.append("code\n", 0.0, format(ctx, false, false, false, true, false));
            }
            let original = ctx.fonts(|f| f.layout_job(job.clone()));
            job.align_baseline = true;
            let aligned = ctx.fonts(|f| f.layout_job(job.clone()));
            let cached = ctx.fonts(|f| f.layout_job(job));
            assert!(
                !Arc::ptr_eq(&original, &aligned),
                "Baseline mode must affect the layout cache key"
            );
            assert!(Arc::ptr_eq(&aligned, &cached));
            for row in &aligned.rows {
                for glyph in &row.glyphs {
                    assert_eq!(glyph.pos.y, row.glyphs[0].pos.y);
                    assert_eq!(glyph.logical_rect().top(), 0.0);
                    assert_eq!(glyph.logical_rect().bottom(), row.height());
                }
            }
        });
    }
    #[test]
    fn list_markers_inline_code_and_body_share_rows_without_double_spacing() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut jobs = Vec::new();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                show(
                    ui,
                    "- Removes dependency on `gh`.\n\n- Preserves checks.\n\n- **42 tests** pass.",
                )
            });
        });
        fn collect(shape: &egui::Shape, jobs: &mut Vec<(f32, Arc<LayoutJob>)>) {
            match shape {
                egui::Shape::Text(t) => jobs.push((t.pos.y, t.galley.job.clone())),
                egui::Shape::Vec(v) => {
                    for s in v {
                        collect(s, jobs)
                    }
                }
                _ => {}
            }
        }
        for s in output.shapes {
            collect(&s.shape, &mut jobs);
        }
        assert_eq!(jobs.len(), 3);
        assert!(jobs[1].0 - jobs[0].0 <= 27.0, "Rows too tall: {jobs:?}");
        for (_, job) in jobs {
            for section in &job.sections {
                assert_eq!(section.format.line_height, Some(22.0));
                assert_eq!(section.format.valign, egui::Align::Center);
                assert_eq!(section.format.underline, egui::Stroke::NONE);
            }
        }
    }
    fn text_shapes(shape: &egui::Shape, texts: &mut Vec<egui::epaint::TextShape>) {
        match shape {
            egui::Shape::Text(text) => texts.push(text.clone()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    text_shapes(shape, texts);
                }
            }
            _ => {}
        }
    }

    fn render(
        ctx: &egui::Context,
        source: &str,
        width: f32,
    ) -> (Vec<egui::epaint::TextShape>, egui::Rect) {
        let mut rect = egui::Rect::NOTHING;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 2000.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    rect = ui.scope(|ui| show(ui, source)).response.rect;
                });
            },
        );
        let mut texts = Vec::new();
        for shape in output.shapes {
            text_shapes(&shape.shape, &mut texts);
        }
        (texts, rect)
    }

    #[test]
    fn hebrew_markdown_preserves_source_and_has_glyphs_in_all_formats() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        for mode in [theme::Theme::Dark, theme::Theme::Light] {
            mode.apply(&ctx);
            let (texts, _) = render(
                &ctx,
                "שלום English 123\\n\\n**עברית מודגשת**\\n\\n`קוד בעברית`",
                500.0,
            );
            for expected in ["שלום English 123", "עברית מודגשת", "קוד בעברית"]
            {
                let text = texts
                    .iter()
                    .find(|t| t.galley.job.text.contains(expected))
                    .unwrap_or_else(|| {
                        panic!(
                            "Missing {expected:?}: {:?}",
                            texts.iter().map(|t| &t.galley.job.text).collect::<Vec<_>>()
                        )
                    });
                for section in &text.galley.job.sections {
                    ctx.fonts(|fonts| {
                        assert!(fonts.has_glyphs(
                            &section.format.font_id,
                            &text.galley.job.text[section.byte_range.clone()]
                        ))
                    });
                }
            }
        }
    }
    #[test]
    fn tables_render_rows_alignment_inline_styles_and_surrounding_prose() {
        let source = "Before.\n\n| Usage | Key | Entries |\n| :--- | :---: | ---: |\n| **Backend** | `mock-key` | *One* and [two](https://example.com) |\n| Tools | a \\| b | |\n\nAfter.";
        let ctx = egui::Context::default();
        theme::install(&ctx);
        for theme in [theme::Theme::Light, theme::Theme::Dark] {
            theme.apply(&ctx);
            let (texts, _) = render(&ctx, source, 700.0);
            let text = |value: &str| {
                texts
                    .iter()
                    .find(|text| text.galley.job.text == value)
                    .unwrap_or_else(|| panic!("Missing {value:?}: {texts:?}"))
            };
            assert_eq!(texts.len(), 11); // Includes the empty cell, but no table syntax.
            let headers = [text("Usage"), text("Key"), text("Entries")];
            for header in headers {
                assert_eq!(header.pos.y, headers[0].pos.y);
                assert_eq!(
                    header.galley.job.sections[0].format.font_id.family,
                    egui::FontFamily::Name("body-bold".into())
                );
                assert_eq!(
                    header.galley.job.sections[0].format.color,
                    theme.palette().text
                );
            }
            assert_eq!(text("Backend").pos.y, text("mock-key").pos.y);
            assert_eq!(text("Backend").pos.y, text("One and two").pos.y);
            assert!(text("Before.").pos.y < headers[0].pos.y);
            assert!(text("After.").pos.y > text("Tools").pos.y);
            assert_eq!(text("Usage").galley.job.halign, egui::Align::Min);
            assert_eq!(text("mock-key").galley.job.halign, egui::Align::Center);
            assert_eq!(text("One and two").galley.job.halign, egui::Align::Max);
            assert_eq!(text("a | b").galley.job.halign, egui::Align::Center);
            assert_eq!(
                text("mock-key").galley.job.sections[0]
                    .format
                    .font_id
                    .family,
                egui::FontFamily::Monospace
            );
            let inline = &text("One and two").galley.job;
            assert!(inline.sections[0].format.italics);
            assert_eq!(
                inline.sections.last().unwrap().format.color,
                theme.palette().accent
            );
            assert_eq!(
                text("After.").galley.job.sections[0].format.font_id.family,
                egui::FontFamily::Proportional
            );
        }
    }

    #[test]
    fn table_wrapping_and_overflow_keep_stable_bounded_geometry() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let source = format!(
            "| Usage | Key | Entries |\n| --- | --- | --- |\n| Backend | `{}` | {} |\n\nAfter.\n\n| Another | Table |\n| --- | --- |\n| A | B |",
            "mock-key-".repeat(20),
            "Long entries wrap without overlapping the next row. ".repeat(10)
        );
        for width in [240.0, 700.0] {
            let mut geometry = Vec::new();
            for _ in 0..4 {
                let (texts, rect) = render(&ctx, &source, width);
                assert!(
                    rect.width() <= width,
                    "Table widened the transcript: {rect:?}"
                );
                assert!(rect.height().is_finite());
                let wrapped = texts
                    .iter()
                    .find(|text| text.galley.job.text.starts_with("Long entries"))
                    .unwrap();
                assert!(wrapped.galley.rows.len() > 1);
                assert!(!wrapped.galley.elided);
                let after = texts
                    .iter()
                    .find(|text| text.galley.job.text == "After.")
                    .unwrap();
                assert!(after.pos.y > wrapped.pos.y + wrapped.galley.size().y);
                geometry.push((
                    rect,
                    texts
                        .iter()
                        .map(|text| (text.pos, text.galley.size()))
                        .collect::<Vec<_>>(),
                ));
            }
            assert!(
                geometry.windows(2).all(|pair| pair[0] == pair[1]),
                "Unstable table layout: {geometry:?}"
            );
        }
    }

    #[test]
    fn incomplete_streamed_tables_and_literal_pipes_render_without_panicking() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let source = "| Usage | Key |\n| --- | --- |\n| Backend | `clé` |\n| Tools |";
        for end in source
            .char_indices()
            .map(|(index, _)| index)
            .chain([source.len()])
        {
            let (_, rect) = render(&ctx, &source[..end], 500.0);
            assert!(rect.is_finite());
        }
        for literal in ["a | b", "`a | b`", "```\n| A | B |\n| --- | --- |\n```"] {
            let events = ParseMarkdown.compute(literal);
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, Event::Start(Tag::Table(_))))
            );
        }
    }

    #[test]
    fn table_links_support_alignment_unicode_and_inline_code_but_block_unsafe_urls() {
        for (alignment, label, url) in [
            (":---:", "clé", "https://example.com"),
            ("---:", "`code`", "https://example.com/code"),
            ("---:", "unsafe", "javascript:bad"),
        ] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let source = format!("| Link |\n| {alignment} |\n| [{label}]({url}) |");
            let (texts, _) = render(&ctx, &source, 500.0);
            let text = texts
                .iter()
                .find(|text| text.galley.job.text == label.trim_matches('`'))
                .unwrap();
            let pos = text.pos + text.galley.rect.center().to_vec2();
            let mut commands = Vec::new();
            for pressed in [true, false] {
                let output = ctx.run(
                    egui::RawInput {
                        events: vec![
                            egui::Event::PointerMoved(pos),
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.scope(|ui| show(ui, &source));
                        });
                    },
                );
                commands.extend(output.platform_output.commands);
            }
            let opened = commands.iter().any(
                |command| matches!(command, egui::OutputCommand::OpenUrl(open) if open.url == url),
            );
            assert_eq!(
                opened,
                safe_url(url),
                "Unexpected navigation for {label}: {commands:?}"
            );
        }
    }

    #[test]
    fn inline_report_paths_and_document_relative_links_are_openable() {
        for (source, base, expected) in [
            ("`/tmp/review.md`", None, "/tmp/review.md"),
            (
                "[Notes](notes.md)",
                Some("/tmp/report.md"),
                "file:///tmp/notes.md",
            ),
        ] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let pos = egui::pos2(35.0, 18.0);
            let mut commands = Vec::new();
            for pressed in [None, Some(true), Some(false)] {
                let output = ctx.run(
                    egui::RawInput {
                        events: pressed
                            .map(|pressed| {
                                vec![
                                    egui::Event::PointerMoved(pos),
                                    egui::Event::PointerButton {
                                        pos,
                                        button: egui::PointerButton::Primary,
                                        pressed,
                                        modifiers: egui::Modifiers::NONE,
                                    },
                                ]
                            })
                            .unwrap_or_default(),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            if let Some(path) = base {
                                show_file(ui, source, std::path::Path::new(path));
                            } else {
                                show(ui, source);
                            }
                        });
                    },
                );
                commands.extend(output.platform_output.commands);
            }
            assert!(
                commands
                    .iter()
                    .any(|c| matches!(c, egui::OutputCommand::OpenUrl(url) if url.url == expected)),
                "{source}"
            );
        }
    }

    #[test]
    fn links_open_browser_only_on_click_not_selection_drag() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let pos = egui::pos2(35.0, 18.0);
        let mut commands = Vec::new();
        for pressed in [None, Some(true), Some(false)] {
            let output = ctx.run(
                egui::RawInput {
                    events: pressed
                        .map(|pressed| {
                            vec![
                                egui::Event::PointerMoved(pos),
                                egui::Event::PointerButton {
                                    pos,
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ]
                        })
                        .unwrap_or_default(),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        show(ui, "[PR #59](https://github.com/a/b/pull/59)")
                    });
                },
            );
            commands.extend(output.platform_output.commands);
        }
        assert!(commands.iter().any(|c|matches!(c,egui::OutputCommand::OpenUrl(u) if u.url=="https://github.com/a/b/pull/59")));
        assert!(!safe_url("javascript:alert(1)"));
    }

    #[test]
    fn split_file_line_and_local_file_handle_relative_paths_and_line_anchors() {
        assert_eq!(split_file_line("src/app.rs:120"), ("src/app.rs", Some(120)));
        assert_eq!(
            split_file_line("src/app.rs:120:15"),
            ("src/app.rs", Some(120))
        );
        assert_eq!(
            split_file_line("src/app.rs#L42-L60"),
            ("src/app.rs", Some(42))
        );
        assert_eq!(split_file_line("ASSETS.md"), ("ASSETS.md", None));

        assert!(local_file("src/app.rs:120"));
        assert!(local_file("./README.md#L10"));
        assert!(local_file("ASSETS.md"));
        assert!(local_file("/tmp/demo.rs:42"));
        assert!(safe_url("src/app.rs:120"));
        assert!(!local_file("self.state"));
        assert!(!local_file("https://example.com/a.rs"));
    }
}
