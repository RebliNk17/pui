# `/btw` split side chat and narrow inspector

## Change

- `/btw [question]` opens an independent interactive chat below the current right-hand Details, Files, or Browser view; the main transcript, running RPC, requests and workspace remain selected. The split is resizable; narrow windows retain a stacked fallback.
- Creation counts live RPC connections/setup reservations, not retained session history. The separate 16-companion limit remains. Refusals identify the actual condition and preserve the command.
- Side Send uses that chat's transport, identity, inherited ordinary permission and delivery receipts. Bypass is never inherited silently. Escape and editor input cannot abort, approve or send in the main chat. Requests are reviewed through Open in the full conversation. No automatic context merging or question replay.
- Runtime companions are protected from content eviction. Dismiss removes the pairing, not either chat or run. Nested side chats retain the original return link and expose the new dock.
- Connector actions reserve a fixed right-aligned column; long names elide without overlapping or moving the action. Inspector section bodies use the same right edge. Output filenames reserve measured count widths.
- Fully visible grouped report titles, inspector section titles and output filenames no longer repeat themselves in tooltips. Elided labels retain full-text hover; meaningful action/error explanations remain.

## Validation

Using existing stable arm64 Rust, pinned CEF, Android SDK CMake/Ninja, explicit SDKROOT and isolated `/tmp/pui-btw-clean-target`; no toolchain installation or paid provider calls.

- **31 side-chat tests passed**, including real deterministic child-process transport, 136 retained sessions, 127/128 connection boundary, split Details/Files/Browser geometry across themes/zoom, side-only Send, Escape ownership, nested docks and non-inherited Bypass.
- **21 inspector tests passed**, including fixed action/name bounds at 180/240/360 points, both themes/zoom, shared right edge and no overlap.
- **4 grouped-disclosure tests passed**, including actual hover output: full title has no duplicate tooltip, truncated title does.
- **15 completion tests passed**, with `/btw` updated to assert the dock stays beside the selected main chat and `/main` still works after opening the side conversation full-size.
- Final focused results: `/tmp/pui-btw-accepted-focused.log`. Strict offline locked binary Clippy passed: `/tmp/pui-btw-clippy-accepted.log`. A feature-only index snapshot at `/tmp/pui-btw-feature.PPNqnG` (excluding concurrent reconnect/message-rendering edits) independently passed the same **71 focused tests** and strict binary Clippy: `/tmp/pui-btw-feature-only-checks.log`. Changed feature files pass rustfmt; `git diff --check` passed.
- Development build passed. Native static dark screenshot generated and inspected: `/tmp/pui-btw-split-dark.png`; preview queued for the user. This is not paid-model/manual runtime acceptance. Installed application unchanged.

## Full-suite limits (not a clean complete-suite result)

Complete validation was attempted with `scripts/test-rust.sh`, without modifying its groups, deadlines or assertions. Early attempts encountered concurrent unrelated incomplete source/test edits and shared-target contention. An isolated four-thread bulk completed **2,050 passed / 16 failed / 2 ignored**: 15 Git-fixture deadline failures and one old completion expectation that `/btw` changes the selected chat. The completion fixture was corrected to the requested dock behavior; all 15 completion tests subsequently passed.

After that repair, a supported `PUI_TEST_THREADS=1 scripts/test-rust.sh` attempt reached its 900-second command bound during bulk Git tests. Its log records unrelated Git failures and a concurrent user-message rendering test failure; no complete result or later serial-group success is claimed. Logs: `/tmp/pui-btw-full-rust-final.log`, `/tmp/pui-btw-full-rust-serial.log`. The unrelated fixtures/features were not changed, skipped or repeatedly retried to manufacture a passing suite.

The feature-only snapshot also ran the unchanged complete runner. Bulk completed **2,046 passed / 13 failed / 2 ignored**, with all side-chat/completion/inspector/disclosure feature tests passing. Twelve failures report Git fixture/operation deadlines; one smudge-filter test failed its expected-error assertion. Later serial groups were not reached because the runner stops on failure. `/tmp/pui-btw-feature-only-full.log`. This is not a full-suite pass, and no assertion/deadline was weakened.

Concurrent per-connector reconnect and user-message rendering work is preserved and excluded from this feature's commit. No push, installation, deployment or application restart.
