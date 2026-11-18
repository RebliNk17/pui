# Concise action summaries instead of raw thinking

## Behavior

Normal transcript view hides provider-labeled thinking, including legacy separate thinking fields. Hidden thinking no longer splits contiguous tool activity into repeated rows: the existing factual Summary renderer groups recorded reads, commands, edits and writes. Ordinary assistant progress updates, final replies, question/answer cards, failure counts and tool disclosures remain unchanged. The live progress/status row still works during thinking-only responses.

Verbose view keeps the existing labeled reasoning renderer with a bounded preview and Show more/Show less. Switching views retains independent disclosure state and reuses the unchanged source history. No protocol filtering, prose heuristics, provider-specific classification, extra AI/model call, reasoning summarization, permissions change or reasoning-effort change. Action counts describe recorded tool calls, not successful validation or unique changed paths; failure counts remain explicit. UI menu hover text explains the distinction. This supersedes the Normal-view behavior described in `docs/bounded-reasoning-validation.md`; its Verbose behavior remains.

## Validation

Isolated worktree `concise-action-summary-transcript`, based on remote `ebafae22`, using dedicated `/tmp/pui-concise-transcript-validation`, Rust 1.88, cached pinned CEF/CMake/Ninja and installed Python 3.13.5. Concurrent original-checkout edits were not changed.

- Transcript focused: **34 passed, 1 existing benchmark ignored**. Includes Normal→Verbose→Normal toggles, structured/legacy reasoning, chronology in Verbose, action grouping across hidden thinking, provider-independent histories, long Unicode reasoning, short ordinary progress text, final answers, visible failures, thinking-only streaming and unchanged retained source. Existing light/dark and narrow/wide renderer checks pass. `/tmp/pui-concise-transcript-focused.log`.
- Complete `scripts/test-rust.sh` with supported `PUI_TEST_THREADS=1`: **1,943 main-binary tests + 10 integration tests passed**, 2 existing ignored, zero failures; all bulk and serial groups included. `/tmp/pui-concise-transcript-full-rust-sdk.log`. No omitted groups, deadline/assertion changes, or failed-test retries.
- First complete-run attempt failed before tests while linking (`libSystem`/`objc` not found). Resolved the toolchain environment by explicitly setting `SDKROOT=$(xcrun --show-sdk-path)`; no repository or production workaround. `/tmp/pui-concise-transcript-full-rust.log` retains that failure.
- Strict all-target Clippy, full `cargo fmt --all -- --check`, and diff whitespace checks passed. `/tmp/pui-concise-transcript-lint-stream.log`.
- JS **100 passed, 1 skipped**, plus **1 streaming encoder/reducer test passed**. `/tmp/pui-concise-transcript-js.log`, `/tmp/pui-concise-transcript-lint-stream.log`.
- Python mock/build checks **41 passed**; no actual release publication. `/tmp/pui-concise-transcript-python.log`.

No app packaging, installation, deployment or native screenshot/manual acceptance claimed. User explicitly requested pushing to master once complete; verify the remote ref after a normal fast-forward push.
