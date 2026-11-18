# Scheduled-job deletion across windows and workflow navigation cleanup

## Change

Scheduled-task Delete is now independent of the creator chat and can be confirmed from a non-owner window. The existing authenticated loopback window-sync transport discovers the advertised scheduler store in live window metadata, sends one UUID-scoped delete request, and returns an acknowledgment/error. This path is window-scoped, not routed through `with_live_chat`, so unavailable creators and pathless windows work.

Only the process holding the existing scheduler OS lock can persist deletion. The owner validates store identity, UUID, writable state, demo/exit guards, persists the new job list before replacing live state, and cancels only that job's scheduled check/one-shot test and bookkeeping. Creator chats and already accepted messages stay untouched. Save failure leaves the live and durable job visible. The observer never writes the job store; it refreshes after acknowledgment. In-flight UI requests disable repeat deletion and display progress. Missing/older owners and transport errors surface an explicit, non-retrying failure; ordinary owner-close takeover still applies. Other job edits retain the owner-only restriction. Agent tool creator-scoping/permission policy is unchanged.

Removed the duplicate Workflows button above Session info sections and its now-unused toggle helper. The right-side collapsible Workflow card, Open action, card order/customization and session-scoped workflow navigation remain.

## Focused evidence

Isolated build target `/tmp/pui-job-delete-validation`, Rust 1.88, pinned cached CEF and existing cached CMake/Ninja, offline/locked:

- **52 automation tests passed**, including 4 new deletion backend/transport tests, observer Delete→Cancel/Confirm UI in light/dark at 1.25 zoom, and exact cancellation of a deleted job's one-shot diagnostic. `/tmp/pui-job-delete-automation-final.log`.
- **19 chat/workflow UI tests passed**, including replacement regression for a single collapsible Workflow entry without the extra button. Standalone no-workflow renderer regression also passed. `/tmp/pui-job-delete-repaired-checks.log`, `/tmp/pui-job-delete-tail-and-workflow.log`.
- Actual loopback and filesystem lock tests cover orphan owner with zero creator sessions, exact job removal, no non-owner writes, failure when owner disappears/unpublishes, and ordinary OS-lock takeover. Tests use temporary fixture jobs only; no real user job was deleted.
- Strict `cargo clippy --offline --locked --all-targets -- -D warnings` passed after removing the unused workflow navigation method and updating tests to exercise the retained card route. Scoped rustfmt and `git diff --check` passed.
- JS: **97 passed / 1 skipped**, plus **1 streaming encoder/reducer test passed** with the installed app SDK. `/tmp/pui-job-delete-js.log`, `/tmp/pui-job-delete-repaired-checks.log`.
- Python mock/build checks: **48 passed** with installed Python 3.13.5; no publication or deployment was performed by these mock tests. The initial system Python 3.9 invocation failed imports requiring tomllib; no dependencies installed. `/tmp/pui-job-delete-python-final.log`.
- Independent review worker unavailable: managed spawning refused at the 128 retained-session limit; no unmanaged fallback.

## Full validation caveats

First `scripts/test-rust.sh` attempt passed **1,788 bulk Rust tests**, 2 ignored, all 32 sync tests and every subsequent group through Git commit tests, plus 4 integration and 3 prompt-design tests. It stopped on existing SVG lifecycle fixture PID-file absence (`src/svg_preview/process/lifecycle_tests.rs:17`) after a 300ms helper startup window. This is an observed timeout-related test failure, not proof of its underlying cause or a pre-existing baseline comparison. `/tmp/pui-job-delete-full-rust.log`.

After the workflow dead-code repair, the complete runner was attempted serially using `PUI_TEST_THREADS=1`, with unchanged assertions/deadlines. It passed the same **1,788 bulk Rust tests**, all sync tests and groups through 13/14 Git commit tests, then failed the SSH-signing commit test's 3-second deadline. `/tmp/pui-job-delete-full-rust-final.log`. The attempted Python 3.13 PATH prefix did not resolve python3; this invocation still reported system Python 3.9.6. No clean full-suite claim. The SVG test was not rerun after the final attempt stopped. The final save-conflict and automation restart/lock groups were explicitly run separately and passed (2 + 1), without omitting those checks. `/tmp/pui-job-delete-tail-and-workflow.log`.

One early focused test process stalled before reporting any tests and was terminated by exact PID; a concurrent build also occupied/overwrote the old shared target. Final feature validation uses the dedicated target above, not the mixed shared binary. No failed assertion was retried alone to manufacture a clean full-suite result and no production/test timeout was relaxed.

The user explicitly chose **Commit and push with caveats** after these failures were disclosed. This commit is not a fully green suite or native user-acceptance claim. No app packaging, installation, restart, real scheduled-task deletion or deployment is part of this change; the previously built `dist/Pui.app` does not contain it.
