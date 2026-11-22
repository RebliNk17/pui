# Stable running-agent sidebar ordering

## Behavior

- Running rows precede idle/completed rows within their existing sidebar section/tree.
- Running rows keep runtime ranks while activity timestamps, session discovery order and active work labels change.
- Newly running rows join after existing runners; finished/waiting/stopped rows release their ranks.
- Idle rows retain the selected date/name ordering and direction.
- Parent/child ownership, pins and custom sections remain intact. Running descendants promote their containing tree; project groups with runners are ordered by their earliest running rank.
- Activity timestamps continue updating; this is presentation-only ordering, not slower event processing.

## Validation

Passed:

- Focused ordering tests: **9 passed** (`/tmp/pui-stable-running-focused.log`).
- Sidebar-focused Rust tests: **159 passed**, including the new light/dark, zoomed Recent/Projects UI regression (`/tmp/pui-stable-running-sidebar.log`).
- `cargo fmt --check` and `git diff --check`.
- Final isolated ordering test invocation: **9 passed** (`/tmp/pui-stable-running-order-final.log`).

Incomplete / blocked; not reported as passing:

- `scripts/test-rust.sh`: bulk **1,911 passed, 2 ignored**, plus **4 horizontal keyboard**, **3 vertical keyboard** and **3 prompt design** tests passed. The command then exceeded its 900-second bound in the serial workflow watch-scope group, while `delayed_watch_setup_failure_revalidates_already_completed_evidence` was running. Later serial groups did not execute. Log: `/tmp/pui-stable-running-full.log`.
- Clippy did not complete within bounded invocations; the latest log stopped during dependency compilation (`/tmp/pui-stable-running-clippy.log`). No successful lint result is claimed.
- A later sidebar rerun was interrupted; a subsequent isolated UI rerun also stalled despite passing in the earlier focused run. Neither replaces the earlier successful run or establishes a cause for the later stalls.

Toolchain notes: Rust 1.88 is installed under `~/.rustup/toolchains/stable-aarch64-apple-darwin/bin`; the temporary CMake recovery installation was incomplete. The existing Android SDK CMake/Ninja installation allowed compilation and the successful focused tests without installing dependencies. No production timeout, assertion or test-runner scheduling was changed.

## Master integration

Merged current remote master without conflicts, preserving both existing local performance commits and the remote history. Integration review added the new fork `parent_session` metadata to preference non-emptiness and sidebar cache invalidation so fork nesting is neither pruned on save nor left stale. A regression test covers both cases. Post-merge focused Cargo validation also exceeded its 150-second bound without a test result (`/tmp/pui-stable-running-post-merge.log`); the merged tree and new integration regression are not claimed to have passed runtime validation.

After disclosure of these validation limits, the user explicitly requested pushing the feature to master. The feature is committed with the incomplete validation recorded here, not represented as a fully validated release. Concurrent unrelated changes/commits are preserved.
