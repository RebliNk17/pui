//! Inline PR capsules share the paragraph's selectable label and link hit target.
//! Reserve readable, non-breaking text in the galley, then paint within its slot.
use super::*;
use crate::icons::{self, Icon};

const HEIGHT: f32 = 22.0;

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(super) struct Capsule {
    pull: crate::pulls::Pull,
    status: Option<crate::pulls::Status>,
    number: Arc<egui::Galley>,
    ci: Arc<egui::Galley>,
    tracking: f32,
    state_color: Color32,
    ci_color: Color32,
}

impl Capsule {
    pub(super) fn new(ui: &Ui, url: &str) -> Option<Self> {
        let (pull, status) = crate::pulls::reference(ui.ctx(), url)?;
        let colors = theme::palette(ui.ctx());
        let state_color = match status.as_ref().map(|s| s.state.as_str()) {
            Some("Merged") => colors.accent,
            Some("Open") => colors.green,
            Some("Closed") => colors.red,
            _ => colors.muted,
        };
        let (ci, ci_color) = match status.as_ref().map(|s| s.checks.as_str()) {
            Some("Checks passed") => ("CI passed", colors.green),
            Some("Checks failed") => ("CI failed", colors.red),
            Some("Checks running") => ("CI running", colors.amber),
            Some("No checks") => ("No checks", colors.muted),
            _ => ("CI unknown", colors.muted),
        };
        let number = ui.painter().layout_no_wrap(
            format!("#{}", pull.number),
            FontId::new(12.0, egui::FontFamily::Name("body-bold".into())),
            colors.text,
        );
        let ci = ui
            .painter()
            .layout_no_wrap(ci.into(), FontId::proportional(11.0), ci_color);
        let width =
            8.0 + 14.0 + 6.0 + number.size().x + 8.0 + 1.0 + 7.0 + 12.0 + 4.0 + ci.size().x + 8.0;
        let label = format!(
            "#{}\u{a0}{}",
            pull.number,
            ci.job.text.replace(' ', "\u{a0}")
        );
        let measured = ui
            .painter()
            .layout_no_wrap(
                label.clone(),
                FontId::proportional(14.0),
                Color32::TRANSPARENT,
            )
            .size()
            .x;
        let tracking =
            (width - measured).max(0.0) / label.chars().count().saturating_sub(1).max(1) as f32;
        Some(Self {
            pull,
            status,
            number,
            ci,
            tracking,
            state_color,
            ci_color,
        })
    }

    pub(super) fn append(self, job: &mut LayoutJob, links: &mut Vec<Link>, format: TextFormat) {
        let start = job.text.len();
        let label = format!(
            "#{}\u{a0}{}",
            self.pull.number,
            self.ci.job.text.replace(' ', "\u{a0}")
        );
        job.append(
            &label,
            0.0,
            TextFormat {
                color: Color32::TRANSPARENT,
                background: Color32::TRANSPARENT,
                underline: egui::Stroke::NONE,
                strikethrough: egui::Stroke::NONE,
                italics: false,
                font_id: FontId::proportional(14.0),
                line_height: Some(HEIGHT),
                extra_letter_spacing: self.tracking,
                ..format
            },
        );
        links.push(Link {
            range: start..job.text.len(),
            url: self.pull.url.clone(),
            capsule: Some(self),
        });
    }

    fn tooltip(&self) -> String {
        let mut detail = format!("{} #{}", self.pull.repo, self.pull.number);
        if let Some(status) = &self.status {
            detail.push_str(&format!("\n{} · {}", status.state, status.checks));
            if !status.review.is_empty() {
                detail.push_str(&format!("\n{}", status.review));
            }
            if let Some(updated) = status
                .updated_at
                .and_then(chrono::DateTime::from_timestamp_millis)
            {
                detail.push_str(&format!(
                    "\nPR updated {}",
                    updated.format("%Y-%m-%d %H:%M UTC")
                ));
            }
            if let Some(error) = &status.error {
                detail.push_str(&format!("\n{error}"));
            }
        } else {
            detail.push_str("\nFetching PR and CI status…");
        }
        detail
    }

    fn paint(&self, ui: &Ui, rect: egui::Rect, hovered: bool) {
        let colors = theme::palette(ui.ctx());
        let painter = ui.painter();
        painter.rect(
            rect,
            6,
            if hovered {
                colors.hover
            } else {
                colors.surface
            },
            egui::Stroke::new(1.0, colors.border),
            egui::StrokeKind::Inside,
        );
        let center = rect.left_center();
        let state = self.status.as_ref().map(|s| s.state.as_str());
        let icon =
            egui::Rect::from_center_size(center + egui::vec2(15.0, 0.0), egui::Vec2::splat(14.0));
        if state == Some("Merged") {
            icons::draw(ui, icon, Icon::Branch, self.state_color);
        } else {
            // Pull-request path, kept distinct from the merge icon.
            let c = icon.center();
            let stroke = egui::Stroke::new(1.3, self.state_color);
            painter.line_segment(
                [c + egui::vec2(-4.0, -3.0), c + egui::vec2(-4.0, 3.0)],
                stroke,
            );
            painter.line_segment(
                [c + egui::vec2(1.0, -4.0), c + egui::vec2(4.0, -4.0)],
                stroke,
            );
            painter.line_segment(
                [c + egui::vec2(4.0, -4.0), c + egui::vec2(4.0, 3.0)],
                stroke,
            );
            for delta in [
                egui::vec2(-4.0, -5.0),
                egui::vec2(-4.0, 5.0),
                egui::vec2(4.0, 5.0),
            ] {
                painter.circle_stroke(c + delta, 1.7, stroke);
            }
        }
        let number_pos = center + egui::vec2(28.0, -self.number.size().y / 2.0);
        painter.galley(number_pos, self.number.clone(), colors.text);
        let divider = number_pos.x + self.number.size().x + 8.0;
        painter.line_segment(
            [
                egui::pos2(divider, rect.top() + 5.0),
                egui::pos2(divider, rect.bottom() - 5.0),
            ],
            egui::Stroke::new(1.0, colors.border),
        );
        let ci_icon = egui::Rect::from_center_size(
            egui::pos2(divider + 14.0, center.y),
            egui::Vec2::splat(12.0),
        );
        let icon = match self.status.as_ref().map(|s| s.checks.as_str()) {
            Some("Checks passed") => Some(Icon::Check),
            Some("Checks failed") => Some(Icon::Close),
            Some("Checks running") => Some(Icon::Clock),
            _ => None,
        };
        if let Some(icon) = icon {
            icons::draw(ui, ci_icon, icon, self.ci_color);
        } else {
            painter.line_segment(
                [
                    ci_icon.center() - egui::vec2(3.0, 0.0),
                    ci_icon.center() + egui::vec2(3.0, 0.0),
                ],
                egui::Stroke::new(1.2, self.ci_color),
            );
        }
        painter.galley(
            egui::pos2(divider + 24.0, center.y - self.ci.size().y / 2.0),
            self.ci.clone(),
            self.ci_color,
        );
    }
}

/// Measure before wrapping, including table cells. Readable text with nonbreaking
/// spaces preserves PR identity when selected/copied, with normal font metrics
/// and extra tracking reserving room for the icon, divider and capsule padding.
pub(super) fn layout(ui: &Ui, job: LayoutJob, links: &[Link]) -> Arc<egui::Galley> {
    let mut job = job;
    // Find highlights must not split a capsule's measurement run: tracking is
    // applied between glyphs in each section, so splitting would shrink it.
    for link in links.iter().filter(|link| link.capsule.is_some()) {
        let mut sections = Vec::with_capacity(job.sections.len());
        let mut capsule_section: Option<egui::text::LayoutSection> = None;
        for section in job.sections.drain(..) {
            if section.byte_range.start >= link.range.start
                && section.byte_range.end <= link.range.end
            {
                if let Some(merged) = &mut capsule_section {
                    merged.byte_range.end = section.byte_range.end;
                    if section.format.background != Color32::TRANSPARENT {
                        merged.format.background = section.format.background;
                    }
                } else {
                    capsule_section = Some(section);
                }
            } else {
                if let Some(mut merged) = capsule_section.take() {
                    merged.format.color = Color32::TRANSPARENT;
                    sections.push(merged);
                }
                sections.push(section);
            }
        }
        if let Some(mut merged) = capsule_section {
            merged.format.color = Color32::TRANSPARENT;
            sections.push(merged);
        }
        job.sections = sections;
    }
    ui.fonts(|fonts| fonts.layout_job(job))
}

pub(super) fn paint(
    ui: &Ui,
    galley: &egui::Galley,
    origin: egui::Pos2,
    links: &[Link],
    hovered: Option<egui::Pos2>,
) {
    let mut byte = 0;
    for row in &galley.rows {
        let row_start = byte;
        for (index, glyph) in row.glyphs.iter().enumerate() {
            if let Some(link) = links.iter().find(|link| {
                link.capsule.is_some()
                    && (link.range.start == byte || (index == 0 && link.range.contains(&row_start)))
            }) {
                let capsule = link.capsule.as_ref().unwrap();
                let count = galley.job.text[byte..link.range.end].chars().count();
                let glyphs = &row.glyphs[index..(index + count).min(row.glyphs.len())];
                let left = glyphs.iter().map(|g| g.pos.x).fold(f32::INFINITY, f32::min);
                let right = glyphs
                    .iter()
                    .map(|g| g.max_x())
                    .fold(f32::NEG_INFINITY, f32::max);
                let rect = egui::Rect::from_center_size(
                    origin + row.pos.to_vec2() + egui::vec2((left + right) / 2.0, row.size.y / 2.0),
                    egui::vec2(right - left, HEIGHT),
                );
                capsule.paint(ui, rect, hovered.is_some_and(|p| rect.contains(p)));
                #[cfg(test)]
                ui.ctx().data_mut(|data| {
                    let id = egui::Id::new(("pr-capsule-rects", &link.url));
                    let rects = data.get_temp_mut_or_default::<Vec<egui::Rect>>(id);
                    rects.push(rect);
                });
            }
            byte += glyph.chr.len_utf8();
        }
        if row.ends_with_newline {
            byte += 1;
        }
    }
}

/// Bare PR URLs get the same capsule; other text and link targets stay intact.
pub(super) fn append_bare(
    ui: &Ui,
    job: &mut LayoutJob,
    links: &mut Vec<Link>,
    text: &str,
    format: TextFormat,
) -> bool {
    let words: Vec<_> = text.split_inclusive(char::is_whitespace).collect();
    if !words.iter().any(|word| {
        let url = word.trim().trim_end_matches(['.', ',', ')', ']', ';']);
        crate::pulls::reference(ui.ctx(), url).is_some()
    }) {
        return false;
    }
    for word in words {
        let url = word.trim().trim_end_matches(['.', ',', ')', ']', ';']);
        if let Some(capsule) = Capsule::new(ui, url) {
            capsule.append(job, links, format.clone());
            job.append(&word[url.len()..], 0.0, format.clone());
        } else {
            let start = job.text.len();
            job.append(word, 0.0, format.clone());
            if url.starts_with("https://")
                || url.starts_with("http://")
                || url.starts_with("mailto:")
                || local_file(url)
            {
                links.push(Link {
                    range: start..start + url.len(),
                    url: url.into(),
                    capsule: None,
                });
            }
        }
    }
    true
}

pub(super) fn tooltip(link: &Link) -> Option<String> {
    link.capsule.as_ref().map(Capsule::tooltip)
}
