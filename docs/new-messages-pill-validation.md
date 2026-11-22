# New-message floating pill (option A)

## Behavior

A centered, neutral-surface `New messages` pill with a purple unread dot and vector down arrow floats 12 logical pixels above the transcript/composer boundary. It does not reserve space or alter message layout. Hover and keyboard focus use the active theme accent.

The pill appears when messages or visible live-tail content change while the reader is more than 32 logical pixels from the bottom. It never moves the reading position automatically. Clicking jumps to the current measured bottom (including streaming content and conversation footers), clears the notice and restores egui's existing sticky following. Manually returning to the bottom also clears it.

Initial history, older-history hydration, progress animation, disclosure/layout changes and hidden thinking-only updates do not trigger it. Rewinds, chat changes and transcript resets clear stale unread state. Detection examines the message count and live tail, not every historical message. The pill uses the transcript panel layer rather than a foreground overlay, preserving dialog/popover ordering, keyboard activation and disabled UI behavior. No protocol, permission or persistence changes.

## Validation

Validated against committed baseline `91d5e7bc` plus only this feature in retained isolated checkout `/tmp/pui-new-messages-validation`. Concurrent original-checkout changes in `src/app.rs`, `src/app/files.rs` and `src/app/files/question_tests.rs` were left out of validation and the feature commit. Initial original-checkout test attempts encountered unfinished private-method/missing-module errors in those concurrent files; no workaround was added to them.

- Four new egui tests pass: pointer jump and sticky following; streaming/tool arrivals versus progress/layout/thinking; history hydration/rewind/session/reset; keyboard activation and disabled state. The pointer/layout test covers light/dark themes, 320/800-pixel widths and 1x/1.5x zoom, and checks unchanged reading position and overlay geometry.
- Complete `scripts/test-rust.sh`: **2,023 passed, 0 failed, 2 existing ignored**, including all serial contention-sensitive groups and the integration/prompt design tests. No skipped runner groups, retries, timeout changes or weakened existing assertions. Log: `/tmp/pui-new-messages-full-rust.log`.
- Focused development tests caught a two-pixel gap caused by egui's cursor-aligned scrolling; the implementation instead applies an immediate bounded scroll delta based on the current content height. The final full run includes the repaired pointer/sticky-follow regression.
- Changed Rust files pass scoped rustfmt; `git diff --check` passes.
- Repository-wide `cargo fmt --all -- --check` remains blocked by existing formatting in Changes UI files. Log: `/tmp/pui-new-messages-fmt.log`. Unrelated formatting was not retained.
- Strict `cargo clippy --offline --locked --all-targets -- -D warnings` remains blocked by existing `too_many_arguments` in `src/app/changes.rs:1300` and unused `output` in `src/app/context_capture/diff/ui_tests.rs:275`. No feature-file lint diagnostic. Log: `/tmp/pui-new-messages-clippy.log`.

No application profiling, packaging, installation, deployment or native manual acceptance claimed. The three-option interactive HTML design preview was shown before development; the user selected A.
