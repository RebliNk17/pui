//! Selectable CommonMark with one consistent baseline and real link hit targets.
use crate::theme;
use eframe::egui::{self, Color32, FontId, RichText, TextFormat, Ui, text::LayoutJob};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::sync::Arc;
#[derive(Default)]
struct ParseMarkdown;
impl egui::cache::ComputerMut<&str, Arc<Vec<Event<'static>>>> for ParseMarkdown {
    fn compute(&mut self, source: &str) -> Arc<Vec<Event<'static>>> {
        Arc::new(Parser::new(source).map(Event::into_static).collect())
    }
}
type MarkdownCache = egui::cache::FrameCache<Arc<Vec<Event<'static>>>, ParseMarkdown>;
struct Link {
    range: std::ops::Range<usize>,
    url: String,
}
fn local_file(value: &str) -> bool {
    !value.chars().any(char::is_whitespace)
        && (value.starts_with('/') || value.starts_with("./") || value.starts_with("~/"))
        && std::path::Path::new(value).extension().is_some()
}
fn safe_url(url: &str) -> bool {
    url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("mailto:")
        || url.starts_with("file://")
        || local_file(url)
        || (!url.contains(':')
            && !url.starts_with('#')
            && std::path::Path::new(url).extension().is_some())
}

pub fn show_file(ui: &mut Ui, source: &str, path: &std::path::Path) {
    let id = egui::Id::new("markdown-file-base");
    ui.ctx()
        .data_mut(|data| data.insert_temp(id, path.parent().unwrap_or(path).to_path_buf()));
    show(ui, source);
    ui.ctx()
        .data_mut(|data| data.remove::<std::path::PathBuf>(id));
}
fn flush(ui: &mut Ui, job: &mut LayoutJob, links: &mut Vec<Link>) {
    if job.text.is_empty() {
        return;
    }
    job.wrap.max_width = ui.available_width();
    let galley = ui.fonts(|f| f.layout_job(std::mem::take(job)));
    let response = ui.add(egui::Label::new(galley.clone()).wrap().selectable(true));
    if let Some(pos) = response.hover_pos() {
        let cursor = galley.cursor_from_pos(pos - response.rect.min);
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
    links.clear();
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
    TextFormat {
        font_id: if code {
            FontId::monospace(12.0)
        } else {
            FontId::new(
                if heading { 17.0 } else { 14.0 },
                if bold || heading {
                    egui::FontFamily::Name("body-bold".into())
                } else {
                    egui::FontFamily::Proportional
                },
            )
        },
        color: if link { colors.accent } else { colors.text },
        background: if code {
            colors.surface
        } else {
            Color32::TRANSPARENT
        },
        italics: italic,
        line_height: Some(24.0),
        valign: egui::Align::Center,
        ..Default::default()
    }
}
pub fn show(ui: &mut Ui, source: &str) {
    let colors = crate::theme::palette(ui.ctx());
    let mut job = LayoutJob::default();
    let mut links = Vec::new();
    let (mut bold, mut italic, mut heading) = (false, false, false);
    let mut link: Option<String> = None;
    let mut code = false;
    let mut code_text = String::new();
    let mut language = String::new();
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut in_item = false;
    let events = ui
        .ctx()
        .memory_mut(|m| m.caches.cache::<MarkdownCache>().get(source));
    for event in events.iter().cloned() {
        match event {
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
                    .fill(colors.sidebar)
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
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(code_text.trim_end())
                                            .font(FontId::monospace(12.0))
                                            .color(colors.text),
                                    )
                                    .selectable(true),
                                );
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
                    format(ui.ctx(), false, false, false, false, false),
                );
            }
            Event::End(TagEnd::Item) => {
                flush(ui, &mut job, &mut links);
                in_item = false;
                ui.add_space(2.0);
            }
            Event::End(TagEnd::Paragraph) => {
                if !in_item {
                    flush(ui, &mut job, &mut links);
                    ui.add_space(6.0);
                }
            }
            Event::Text(text) if code => code_text.push_str(&text),
            Event::Code(text) => {
                let start = job.text.len();
                job.append(
                    &text,
                    0.0,
                    format(
                        ui.ctx(),
                        false,
                        false,
                        false,
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
                    format(ui.ctx(), bold, italic, heading, false, link.is_some()),
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
                format(ui.ctx(), false, false, false, false, false),
            ),
            Event::HardBreak => job.append(
                "\n",
                0.0,
                format(ui.ctx(), false, false, false, false, false),
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
    let background = ui.painter().add(egui::Shape::Noop);
    let response = ui.add(egui::Label::new(job).selectable(true).extend());
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
        assert!(jobs[1].0 - jobs[0].0 < 36.0, "Rows too tall: {jobs:?}");
        for (_, job) in jobs {
            for section in &job.sections {
                assert_eq!(section.format.line_height, Some(24.0));
                assert_eq!(section.format.valign, egui::Align::Center);
                assert_eq!(section.format.underline, egui::Stroke::NONE);
            }
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
}
