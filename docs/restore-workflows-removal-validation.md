# Restore scheduled deletion and Workflows-button removal to current remote

## Integration

The original `00194b25` was successfully pushed on 2026-11-18 17:17 +0200, but subsequent remote history replacement removed it. Its identical local replacement `a8672742` was not an ancestor of remote `master` (`e219d64e`). Remote source once again rendered `session_workflows_button()`; local source did not.

Restored the complete missing patch in isolated worktree `restore-scheduled-delete-workflow-removal`, based on `e219d64e`, as `3d6fef69`. Cherry-pick applied without conflict. This preserves the latest remote reasoning/updater changes and reintroduces both orphan scheduled-job deletion and removal of the duplicate Workflows button. No force push, original-checkout edits, stashing, application installation or packaging. Concurrent original-checkout changes remain untouched.

The collapsible Workflow inspector card and its Open action remain; no Workflows button renderer or toggle helper remains. See `docs/scheduled-job-delete-validation.md` for behavior and historical feature checks.

## Current combined validation

Dedicated `/tmp/pui-restore-workflows-validation`, Rust 1.88, pinned cached CEF/CMake/Ninja, Python 3.13.5, offline/locked:

- Exact button-removal regression passed in both themes and zoom levels; all **19 workflow/chat tests** passed.
- **4 scheduled deletion tests** passed, including real authenticated loopback routing without creator sessions and owner-close takeover. `/tmp/pui-restore-workflows-focused.log`.
- Full `scripts/test-rust.sh` bulk: **1,747 passed, 2 failed, 2 ignored**; stopped on Kanban scrollbar-track interaction and a 3-second Git index-preview deadline. `/tmp/pui-restore-workflows-full-rust.log`. Failures are outside changed code, not established as baseline failures by a separate baseline run. No failed tests retried.
- All **181 serial-group tests** still unrun after that stop were executed once with the checked-in filters and serial scheduling; all passed, including 32 window-sync and SVG lifecycle tests. `/tmp/pui-restore-workflows-serial-tail.log`. Combined Rust main binary: **1,928 passed / 2 failed / 2 ignored**, all 1,932 accounted for. This is not a clean complete-runner pass.
- **3 prompt-design tests** passed explicitly. No prompt guard weakened.
- JS: **97 passed / 1 skipped**, plus **1 streaming encoder/reducer passed**. `/tmp/pui-restore-workflows-js.log`, `/tmp/pui-restore-workflows-streaming.log`.
- Python mock/build checks: **41 passed**. `/tmp/pui-restore-workflows-python.log`. Mock release messages are not real publications.
- Scoped rustfmt and `git diff --check` passed.
- Strict Clippy failed on three diagnostics in unchanged `src/app/updates.rs`: unused `show`, complex receiver type, collapsible `if`. `/tmp/pui-restore-workflows-lint-stream.log`. Confirmed updater and failed-test source files identical to remote base; no unrelated fixes or lint suppressions added.

The user explicitly requested re-integration, validation and normal push after finding the prior push absent. Prior approval to push these features with recorded full-suite caveats is retained; this restoration documents its own current failures rather than claiming green validation. No new app bundle has been produced.
