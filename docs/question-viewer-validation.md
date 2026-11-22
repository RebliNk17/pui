# Browser visibility during inline agent questions

## Fix

Pending agent questions and permission cards were treated as global overlays in
`Pui::update`, which hid native Chromium and left the reserved browser/HTML
preview area empty. Agent prompts render inline next to the viewer.

The shared visibility predicate now permits inline prompts. All existing
workspace guards (settings, boards, theater, confirmations, search and section
confirmation) and the browser's geometric floating-surface overlap checks remain
in place. No permission scope, question submission or Chromium policy changed.

## Validation

Validated in the isolated `fix-question-viewer-blank` worktree from `91d5e7bc6`;
concurrent original-checkout transcript edits initially prevented compilation
and were neither changed nor included.

- Both new regression tests failed with the old pending-dialog guard, then
  passed after removing it.
- Full-update fixtures cover HTTP pages and local HTML, light/dark themes,
  1480px/100% and 860px/125% layouts, arriving questions and permissions,
  pending-answer disabled state, cancellation/denial, draft/selection retention
  and workspace-overlay guard preservation. These are headless UI/state tests,
  not a native CEF pixel or installed-app acceptance test.
- `scripts/test-rust.sh`: bulk **1,820 passed, 2 failed, 2 existing ignored**.
  Failures were outside changed files: Git hunk fixtures
  `explicit_hunk_only_updates_selected_index_change_worktree_unchanged` and
  `selected_addition_or_deletion_never_includes_opposing_replacement` hit
  existing 3-second status/preview deadlines. No baseline comparison was made;
  their underlying cause is not established.
- Ran the runner's remaining serial groups unchanged, in order, after its bulk
  failure: **189 passed**. No failed test was retried, no deadline/assertion was
  weakened, and every main-binary test was included once across this run.
  Overall main-binary result: **2,009 passed, 2 failed, 2 ignored** — not a clean
  complete-suite pass. Logs: `/tmp/pui-question-viewer-full.log` and
  `/tmp/pui-question-viewer-remaining.log`.
- Rust integration tests (`panel_keyboard`, `panel_vertical_keyboard`,
  `prompt_design`): **10 passed**.
- `node --test tests/questions.test.mjs tests/open.test.mjs`: **12 passed**.
- `cargo clippy --offline --locked --all-targets`: passed with existing warnings
  in `changes.rs` and `context_capture/diff/ui_tests.rs`.
- Changed Rust files pass targeted rustfmt checking; `git diff --check` passes.

Used cached Rust 1.88, pinned CEF, explicit macOS SDK and the existing
`/tmp/pui-job-delete-validation` target; offline/locked, with isolated Git
configuration and Homebrew Python. No install, packaging, app restart, provider
call or deployment was performed.
