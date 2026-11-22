# One-time chat titles — focused validation

## Scope and root causes

Worktree: `chat-title-once`, based on `256003a8`. No commits, integration, app restart, installations, live user-data reads or provider calls performed by this worker.

Previous display naming conflated mutable Pi `sessionName` with an assigned title; every `get_state`/`session_info_changed` could replace it. Fallback inspected projected history, which can begin with a different user request after compaction. Restoring root agents discarded their saved name. The old fallback flattened 100 characters of context into sidebar labels.

New display assignment is separate from raw protocol metadata. `State.title` is session-path bound; `ChatPrefs.initial_title` pins preserved legacy/automatic titles separately from existing explicit `ChatPrefs.title` renames. Existing root/child names are retained. Unknown/legacy sessions are never enrolled in new generation based only on marker absence. Existing legacy prompt fallbacks retain their 100-character policy; existing/manual names are not truncated or rewritten.

## New automatic assignment

Only direct first composer submission in a newly allocated, empty, unnamed chat prepares the existing extension command `/pui-initial-title`. It sends compact title, exact session path/id, and SHA-256 of exact outgoing prompt plus image count (no private full prompt in marker). The next raw RPC input must match. The command is synchronous, inert, and cannot rename any assigned/named/existing/forked session. Inputs are not transformed, resent, delayed awaiting model work, or given new provider credentials. Scheduled/queued/extension/child followups do not prepare titles. Rust accepts fresh status only for its expected digest/path/id; assigned display titles cannot be overwritten by late status.

Titles use the first meaningful request paragraph (soft-wrapped lines joined), excluding captured `[Context: …]` blocks and refusing known agent/compaction/code-only boilerplate. Links reduce to host or GitHub owner/repo. Cap is **48 Unicode scalar characters**, preferring a word boundary if useful; long unbroken input cannot bypass it. Controls, Unicode format characters (including bidi/zero-width), and line/paragraph separators cannot enter automatic title metadata. Format/separator-containing requests refuse generation consistently; original prompt is unchanged. Generic greetings/continuation-only requests remain unassigned rather than inventing an irrelevant title. Attachment-only first requests use `Image attachment`.

Missing/rejected setup/status does **not** mark the placeholder assigned and does not block/drop/retry the user's prompt. During that connection, unconfirmed projected/discovered text cannot silently declare assignment. Reopening an unknown legacy session conservatively preserves its existing discovery title rather than broadly regenerating. Explicit inline rename remains uncapped and takes precedence, including after late initial status. Explicit cross-window renames have a separate action/snapshot field; harness metadata is never treated as explicit rename. No new modal or per-frame title hashing.

## Durability boundary

`scripts/titles.ts` uses only public `pi.appendEntry` and session-manager readers. Marker type `pui.initial-title.v1` is inert `custom` session data, not model context. SDK implementation inspected: setup-only entries remain in memory until a user/assistant conversation entry exists; the first user append flushes setup+user entries synchronously. Append failure rolls back the appended entry. **Command acceptance/status alone is not independent disk durability.** If the process dies before the first user entry persists, no new chat file exists; no accepted prompt is replayed. A real temp-file SessionManager test confirms marker absent before first user append and present after user append, compaction and reopen. This does not claim fsync/power-loss guarantees beyond the SDK.

Discovery validates marker version/title cap/control and format exclusion/bounded session id/request digest; malformed markers fall back conservatively to existing session name/prompt. No JSONL rewrite, locking or migration of existing files.

## Focused evidence

Build environment: private `/tmp/pui-chat-title-target`, `CARGO_BUILD_JOBS=2`, configured CEF from parent's existing cache; CLT Git/compiler, stable Rust, recovery Ninja/CMake; `SDKROOT="$(/usr/bin/xcrun --show-sdk-path)"`, `CARGO_HTTP_PROXY_CAINFO` unset.

- `cargo test --offline --locked --bin pui title -- --test-threads=1`: **38 passed**. `/tmp/pui-chat-title-rust.log`.
- `cargo test --offline --locked --bin pui rename -- --test-threads=1`: **13 passed**, including existing Enter-save/Escape/focus-loss cancellation tests and explicit cross-window rename versus late automatic metadata. `/tmp/pui-chat-title-rename.log`.
- `cargo test --offline --locked --bin pui saved_tree_restores -- --test-threads=1`: **1 passed**. `/tmp/pui-chat-title-restore.log`.
- `cargo test --offline --locked --bin pui fork_during_poll -- --test-threads=1`: **1 passed**. `/tmp/pui-chat-title-fork.log`.
- `PUI_PI_SDK=/Applications/Pui.app/Contents/Resources/runtime/pi/node_modules/@earendil-works/pi-coding-agent/dist/index.js node --test tests/titles.test.mjs tests/ultracode.test.mjs`: **20 passed**, no skips. `/tmp/pui-chat-title-js.log`.
- Actual offline RPC subprocess fixture uses isolated HOME/agent-dir, disables discovered extensions/skills/templates, and handles actual input before provider preflight. Back-to-back setup+user records yield bound title status before exactly one raw user input; setup/bad setup never becomes user/model input, and no `agent_start` occurs.
- `/opt/homebrew/bin/python3.13 -m unittest discover -s tests -p test_runtime_bundle.py`: **8 passed**. `/tmp/pui-chat-title-packaging.log`.
- `cargo clippy --offline --locked --bin pui --tests -- -D warnings`: passed. `/tmp/pui-chat-title-clippy.log`.
- `cargo fmt --all -- --check`, `git diff --check`, `bash -n scripts/bundle-macos.sh`: passed.

Deterministic relevance fixture uses the actual user request about `onorca.dev` and `github.com/stablyai/orca`, requiring `onorca.dev stablyai/orca lets research this app`. Tests cover Unicode/unbroken caps, context/summary refusal, mutable get_state/event metadata, restart serialization, stale discovery/live precedence, duplicate roots/child isolation, session reuse/reconnect, late status versus manual rename, fork ids, exact setup/input ordering, append failure and pre-user shutdown.

## Parent integrated validation

Reviewed with all four parallel slices on base `256003a8` in `parallel-features-integration`. Complete combined validation: **2,032 Rust binary + 10 integration tests passed**,2ignored/zero failures; **114 JS passed/1skipped**, **41 Python passed**, strict all-target Clippy, formatting, shell syntax and diff checks passed. Logs `/tmp/pui-parallel-{rust,js,python,clippy,fmt}.log`. Two test-only Clippy style fixes and automation-view formatting were made during integration; no assertions/deadlines/scheduling weakened. Evidence applies to the complete combined source, not a separate full-suite run per intermediate feature commit. See `parallel-features-integration-validation.md` for integration identity and remaining acceptance gates.

## Earlier failures and limitations

- First Cargo attempt hit inherited `http.proxy` configuration; subsequent proper environment unsets proxy CA override.
- Initial linker attempt failed `library 'System' not found`; explicit correct SDKROOT/CLT fixed linking.
- Copy-on-write private target inherited an absolute CMake cache. That private cache directory was moved aside; CEF rebuilt privately. One bounded 240s build timed out; resumed build completed. No shared cache/build was killed or modified.
- First focused execution exposed stale discovery incorrectly winning over live title; reconciliation was fixed and regression retained.
- Initial switch lifecycle fixture took a fresh-client branch without including the target in discovery; fixture now explicitly forces the reused-client case and tests reconnect separately.
- Existing restore fixture gave root and child the same path and used an unnamed root. It now supplies distinct root/child paths and an assigned root name, asserting both preserved names.
- Initial strict Clippy dependency/CEF build exceeded 240s; resumed private build passed with warnings denied.

Full suite, packaged runtime execution, external-provider behavior and live app verification are intentionally deferred to parent integration. No live-provider quality acceptance is claimed. New named/unknown sessions are not retroactively shortened; this is preservation, not a bulk naming migration.

## Shared-file integration hunks

`src/app.rs` only:
1. `connect`: pin discovered name immediately after seeding `session_name`.
2. reused `open_chat` switch: clear expected title request/session id and pin target name before late events.
3. `submit_mode`: call `prepare_initial_title(&message)` immediately before existing `deliver_message`.

Packaging adds `titles.ts` to existing adapter copy/smoke lists; `ultracode.ts` has one import and one install call. Other source changes are naming/protocol/sidebar/sync/agent-restore and focused tests; browser/context/tasks implementation files are untouched.
