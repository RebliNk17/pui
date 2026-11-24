# Automatic RTL paragraph layout

## Behavior

- Natural Unicode first-strong direction per paragraph; numbers, punctuation and inline formatting do not act as language switches. No model/network detection or global UI mirroring.
- Right-aligned RTL prose in assistant replies, rendered Markdown, user bubbles and composer prose. Wrapped lines follow the same right edge. English-first paragraphs remain left-aligned.
- Assistant list markers and hanging gutters follow each item's direction. Nested lists retain independent left/right ancestor insets, including LTR→RTL and RTL→LTR transitions. Direction detection assembles inline events so Unicode isolates survive formatting boundaries.
- Source strings, logical glyph order, saved/sent/copied text, links and find offsets are unchanged. Cached galleys/rows are not mutated. Fenced code and explicit table alignment are not overridden.

## Validation

Final focused compiled-test run (`/tmp/pui-rtl-direct-final.log`):

| Check | Result |
| --- | --- |
| `markdown::` | 26 passed |
| `theme::` | 28 passed |
| `app::composer_text::tests::rtl_composer` | 1 passed |
| `transcript::rtl_tests::` | 1 passed |

These are direct invocations of `target/debug/deps/pui-346f211a160bfd41`, built by the preceding locked/offline Cargo test runs. No test assertions, production deadlines or suite groups were weakened/skipped to obtain these focused results. Earlier broader focused validation passed 23 composer and 51 transcript tests (1 existing ignored benchmark); later broad reruns timed out, so those earlier counts are not claimed as final full validation.

Coverage includes light/dark themes; 270/760-point viewports; 0.8/1.0/1.4 zoom; headings, paragraphs and lists; formatted/isolated text; mixed-direction siblings, hard breaks and nested wrapping; code/table regressions; RTL link clicks; native drag-selection/copy; find rectangles; and user bubbles expanded for image/footer width.

Independent read-only review identified mixed sibling alignment, split-isolate state and mixed-nesting ancestor inset bugs. All were repaired with regressions; final review found no concrete blocker. Native tests initially clicked a half-open right hitbox edge; fixture coordinates were moved inside the boundary and logical copy assertions now pass.

- Targeted rustfmt and `git diff --check`: passed.
- Production `cargo clippy --offline --locked --bin pui -- -D warnings`: passed before the final list/test repairs (`/tmp/pui-rtl-clippy.log`). Final all-target Clippy attempt timed out without a result (`/tmp/pui-rtl-final-clippy.log`); no final all-target pass is claimed.
- Complete `scripts/test-rust.sh`: attempted, but timed out after 900 seconds with the last reported test `app::agents::idle::tests::activity_in_selected_chat_resets_only_its_countdown`. No complete result, serial-group completion or full-suite pass is claimed (`/tmp/pui-rtl-full-rust.log`). Other concurrent builds/tests/edits were present; no root cause is established.
- Whole-checkout `cargo fmt --all -- --check` reported unrelated concurrent formatting differences in files/inspector/tooltip code; those files were not reformatted by this feature (`/tmp/pui-rtl-full-format.log`).

## Boundaries

This is direction-aware layout, not a new shaping engine. Existing Arabic contextual joining, Indic shaping, precise combining-mark placement and split-caret affinity limitations remain. No installed application was replaced, package built, push performed or live screenshot acceptance claimed. Unrelated concurrent edits are excluded from the feature commit.
