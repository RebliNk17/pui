# egui 0.32.3 text patches

These directories contain the published `egui` and `epaint` 0.32.3 sources, licensed under MIT OR Apache-2.0. The upstream licenses are alongside this file. `Cargo.toml` patches both crates together; there is no replacement application/editor implementation.

Upstream: https://github.com/emilk/egui/tree/0.32.3/crates

## Local changes

- `epaint/src/text/bidi.rs`: Unicode Bidirectional Algorithm (`unicode-bidi`), paragraph direction resolution and per-wrapped-line visual ordering before tessellation. Reorder grapheme clusters, mirror bracket glyphs (`unicode-bidi-mirroring`), never reorder source strings or logical glyph vectors. LTR/CJK layouts take the unchanged fast path.
- `epaint/src/text/text_layout.rs`: invoke bidi after logical wrapping/truncation; tessellate decorated glyph runs in visual order while preserving source/style indices. Opt-in `LayoutJob::align_baseline` aligns mixed fonts/fallbacks to one baseline per row before tessellation, centering the combined ascent/descent in the line height. Markdown opts in; ordinary labels/editors retain upstream placement.
- `epaint/src/text/text_layout_types.rs`: bidi row caret metadata, logical-to-visual hit testing, visual arrow navigation and potentially discontiguous selection rectangles; default-off `align_baseline` layout/cache flag.
- `epaint/src/text/fonts.rs`: preserve `align_baseline` when splitting/caching long paragraphs.
- `epaint/src/shapes/text_shape.rs`: account for the new row metadata field in destructuring.
- `egui/src/text_selection/visuals.rs`: paint discontiguous selected glyph ranges.
- `egui/src/text_selection/cursor_range.rs`: collapse selections to the requested visual endpoint.

Tests are in `src/theme/bidi_tests.rs` (including actual TextEdit copy/paste/insertion/backspace), the existing font/Markdown tests, and the application's regression suite. Previews: `cargo run -- --demo-languages` and `cargo run -- --demo-markdown` (mixed-style baselines).

## Boundaries

This adds bidirectional ordering and basic Hebrew editing, not an OpenType shaping engine. Arabic joining, Indic shaping, exact positioning of Hebrew niqqud/other combining marks and full split-caret affinity at bidi boundaries are not implemented. Word-modified movement retains egui's logical word-boundary behavior. No UI translation or forced global RTL layout. Pure RTL paragraphs retain their widget's existing alignment.

When upgrading egui, either rebase these focused patches as a pair and rerun the tests, or remove both patches if upstream provides equivalent bidi support. Original upstream files remain unchanged except the files listed above and epaint's added dependencies. Font assets and licenses live under `assets/fonts`.
