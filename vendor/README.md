# egui 0.32.3 local patches

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
- `egui/src/containers/tooltip.rs`: opt-in `TextStyle::Name("Tooltip")` sets a tooltip-local font without changing body text, menus or dialogs. Upstream behavior is unchanged when the named style is absent.
- `egui/src/widgets/label.rs`: automatic elision tooltips honor that optional tooltip font, reset wrapping and keep full text available.
- `egui/src/memory/mod.rs`: explicit `request_focus_with_filter` cancels already-handled navigation, including pending Tab/Shift+Tab moves. This keeps composer list indentation from also moving focus on the first focused frame.

Tests are in `src/theme/bidi_tests.rs` (including actual TextEdit copy/paste/insertion/backspace), the existing font/Markdown tests, and the application's regression suite. Previews: `cargo run -- --demo-languages` and `cargo run -- --demo-markdown` (mixed-style baselines).

## Boundaries

This adds bidirectional ordering and basic Hebrew editing, not an OpenType shaping engine. Arabic joining, Indic shaping, exact positioning of Hebrew niqqud/other combining marks and full split-caret affinity at bidi boundaries are not implemented. Word-modified movement retains egui's logical word-boundary behavior. No UI translation or forced global RTL layout. Pure RTL paragraphs retain their widget's existing alignment.

`Memory::set_focus_lock_filter_current(id, filter)` updates only the current owner’s filter, even on its first pass, without cancelling pending native traversal. Maintenance callers use this instead of `request_focus_with_filter`, whose deliberate transfer semantics may cancel already-handled Tab navigation. This keeps reverse-Tab moves intact alongside the composer's separate handled-navigation fix. No ownership or consent is granted by installing a filter; foreign IDs are ignored. Direct tests cover forward/deferred reverse Tab and same-pass/foreign-owner Escape filters.

The `egui/src/memory/mod.rs` focus metadata additionally exposes `Memory::focus_lost_by_escape(id)`: one transient per-viewport ID records the exact native owner cleared by Escape after deferred reverse-Tab assignment. It resets each pass and preserves the first cleared owner across repeated Escape events. It changes no focus/filter/event-consumption behavior and is not persisted. App-owned early handlers still require a live enabled widget and their own visibility/layer/modal gates before consuming Escape. This avoids unbounded or inaccurate reconstructions of native focus order in virtual tables and inline review. `src/app/native_escape_tests.rs` and table/inline input regressions cover the accessor and callers.

When upgrading egui, either rebase these focused patches as a pair and rerun the tests, or remove both patches if upstream provides equivalent bidi support. Original upstream files remain unchanged except the files listed above and epaint's added dependencies. Font assets and licenses live under `assets/fonts`.
