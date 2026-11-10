//! Pui patch: UAX #9 ordering after wrapping, before tessellation. Never mutate source text.
use super::text_layout_types::RowBidi;
use super::{FontsImpl, LayoutJob, PlacedRow};
use std::sync::Arc;
use unicode_bidi::{BidiInfo, Level};
use unicode_segmentation::UnicodeSegmentation;

pub(super) fn reorder(fonts: &mut FontsImpl, job: &LayoutJob, rows: &mut [PlacedRow]) {
    // The common Latin/CJK path avoids allocations and preserves original kerning/layout.
    if !job.text.chars().any(|c| {
        matches!(
            unicode_bidi::bidi_class(c),
            unicode_bidi::BidiClass::R
                | unicode_bidi::BidiClass::AL
                | unicode_bidi::BidiClass::RLE
                | unicode_bidi::BidiClass::RLO
                | unicode_bidi::BidiClass::RLI
                | unicode_bidi::BidiClass::FSI
        )
    }) {
        return;
    }
    let mut start = 0;
    while start < rows.len() {
        let end = rows[start..]
            .iter()
            .position(|r| r.ends_with_newline)
            .map_or(rows.len(), |n| start + n + 1);
        reorder_paragraph(fonts, job, &mut rows[start..end]);
        start = end;
    }
}

fn reorder_paragraph(fonts: &mut FontsImpl, job: &LayoutJob, rows: &mut [PlacedRow]) {
    let text: String = rows
        .iter()
        .flat_map(|r| r.glyphs.iter().map(|g| g.chr))
        .collect();
    let bidi = BidiInfo::new(&text, None);
    if !bidi.has_rtl() {
        return;
    }
    let mut byte_start = 0;
    for placed in rows {
        let row = Arc::make_mut(&mut placed.row);
        let len: usize = row.glyphs.iter().map(|g| g.chr.len_utf8()).sum();
        let byte_end = byte_start + len;
        let Some(para) = bidi
            .paragraphs
            .iter()
            .find(|p| p.range.contains(&byte_start))
        else {
            byte_start = byte_end;
            continue;
        };
        let levels = bidi.reordered_levels(para, byte_start..byte_end);
        let row_text = &text[byte_start..byte_end];
        // Reverse grapheme clusters, not marks within a Hebrew letter + niqqud.
        let mut clusters = Vec::new();
        let mut char_start = 0;
        for (byte, cluster) in row_text.grapheme_indices(true) {
            let count = cluster.chars().count();
            clusters.push((char_start..char_start + count, levels[byte_start + byte]));
            char_start += count;
        }
        let order = BidiInfo::reorder_visual(&clusters.iter().map(|c| c.1).collect::<Vec<Level>>());
        let n = row.glyphs.len();
        let mut rtl = vec![false; n];
        let mut caret_x = vec![0.0; n + 1];
        let mut x = row.glyphs.first().map_or(0.0, |g| g.pos.x);
        for cluster_index in order {
            let (range, level) = &clusters[cluster_index];
            let width: f32 = row.glyphs[range.clone()]
                .iter()
                .map(|g| g.advance_width)
                .sum();
            let mut cluster_x = x;
            for i in range.clone() {
                let glyph = &mut row.glyphs[i];
                rtl[i] = level.is_rtl();
                glyph.pos.x = cluster_x;
                cluster_x += glyph.advance_width;
                // Mirroring affects the painted glyph only (e.g. RTL parentheses).
                if level.is_rtl() {
                    if let Some(mirrored) = unicode_bidi_mirroring::get_mirrored(glyph.chr) {
                        let font =
                            fonts.font(&job.sections[glyph.section_index as usize].format.font_id);
                        glyph.uv_rect = font.font_impl_and_glyph_info(mirrored).1.uv_rect;
                    }
                }
                caret_x[i] = if level.is_rtl() {
                    x + width
                } else {
                    glyph.pos.x
                };
            }
            x += width;
        }
        if let Some(last) = row.glyphs.last() {
            caret_x[n] = if rtl[n - 1] { last.pos.x } else { last.max_x() };
        }
        row.bidi = Some(RowBidi {
            caret_x,
            rtl,
            base_rtl: para.level.is_rtl(),
        });
        byte_start = byte_end;
    }
}
