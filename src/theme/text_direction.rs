//! Natural paragraph alignment, on top of the renderer's Unicode bidi ordering.
use eframe::egui::{self, Galley};
use std::sync::Arc;

/// UAX #9 first-strong direction, ignoring numbers, punctuation and isolated runs.
/// `None` means neutral text, not an instruction to reverse its characters.
pub(crate) fn rtl(text: &str) -> Option<bool> {
    match unicode_bidi::get_base_direction(text) {
        unicode_bidi::Direction::Rtl => Some(true),
        unicode_bidi::Direction::Ltr => Some(false),
        unicode_bidi::Direction::Mixed => None,
    }
}

/// Right-align each RTL paragraph's wrapped rows within the available width.
/// Keep the galley left-anchored: selection, links and find use the same origin.
/// Call only for natural prose, never explicitly aligned tables or code blocks.
pub(crate) fn align(galley: &mut Arc<Galley>, width: f32) {
    align_where(galley, width, |_| true);
}

/// The composer excludes fenced source rows while retaining their bidi ordering.
pub(crate) fn align_where(
    galley: &mut Arc<Galley>,
    width: f32,
    mut include: impl FnMut(std::ops::Range<usize>) -> bool,
) {
    if !width.is_finite()
        || !galley
            .rows
            .iter()
            .any(|row| row.bidi.as_ref().is_some_and(|bidi| bidi.base_rtl))
    {
        return;
    }
    let galley = Arc::make_mut(galley);
    let mut index = 0;
    for row in &mut galley.rows {
        let end = index + row.char_count_including_newline();
        if include(index..end) && row.bidi.as_ref().is_some_and(|bidi| bidi.base_rtl) {
            row.pos.x = (width - row.size.x).max(0.0);
        }
        index = end;
    }
    update_bounds(galley);
    galley.rect.max.x = galley.rect.max.x.max(width);
}

pub(crate) fn update_bounds(galley: &mut Galley) {
    galley.rect = egui::Rect::ZERO;
    galley.mesh_bounds = egui::Rect::NOTHING;
    for row in &galley.rows {
        galley.rect |= row.rect();
        galley.mesh_bounds |= row.visuals.mesh_bounds.translate(row.pos.to_vec2());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_direction_uses_unicode_first_strong_not_presence_or_majority() {
        for text in [
            "שלום English",
            "123. (שלום)",
            "مرحبا بالعالم",
            "فارسی",
            "اردو",
        ] {
            assert_eq!(rtl(text), Some(true), "{text}");
        }
        for text in ["English שלום שלום", "123 English", "中文", "Ελληνικά"] {
            assert_eq!(rtl(text), Some(false), "{text}");
        }
        assert_eq!(rtl("123 … 👍"), None);
        assert_eq!(rtl("\u{2066}English\u{2069} שלום"), Some(true));
        assert_eq!(rtl("\u{2067}שלום\u{2069} English"), Some(false));
    }

    #[test]
    fn natural_alignment_is_per_paragraph_and_keeps_ltr_cached() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let layout = |source: &str| {
                ctx.fonts(|f| {
                    f.layout(
                        source.into(),
                        egui::FontId::proportional(14.0),
                        egui::Color32::WHITE,
                        125.0,
                    )
                })
            };
            let original = layout("English only");
            let mut ltr = original.clone();
            align(&mut ltr, 125.0);
            assert!(Arc::ptr_eq(&original, &ltr));
            let source = "שלום עולם שלום עולם שלום עולם\nEnglish 123\nمرحبا بالعالم";
            let original = layout(source);
            let mut natural = original.clone();
            align(&mut natural, 125.0);
            assert_eq!(natural.job.text, source);
            assert!(Arc::ptr_eq(&natural.job, &original.job));
            assert!(natural.rows.len() > 3);
            for (row, before) in natural.rows.iter().zip(&original.rows) {
                assert!(Arc::ptr_eq(&row.row, &before.row));
                if row.bidi.as_ref().is_some_and(|bidi| bidi.base_rtl) {
                    assert!((row.rect().right() - 125.0).abs() < 0.01);
                } else {
                    assert_eq!(row.pos, before.pos);
                }
                assert_eq!(before.pos.x, 0.0, "Do not mutate the font cache");
            }
        });
    }
}
