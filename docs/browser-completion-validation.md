# Browser completion — integrated validation and remaining gate

## Status

Implemented in isolated `browser-completion`, based on explicitly pulled master `7a8b47c8b` (1.0.1). **Browser-focused checks and disposable local CEF checks pass; the repository suite does not. This is not a validated release or an all-Git/all-roadmap completion claim.** No push, install, installed-app restart, provider request or application profiling occurred. After reviewing failure ownership, the user explicitly authorized committing this browser feature separately from pre-existing/concurrent Git work, with the non-green repository gate disclosed.

## Integrated behavior

- Explicit bounded descendant-text reading, snapshot-unique generated selectors, same-origin frame inventory and one selected open shadow root. Cross-origin/ambiguous/closed/nested scopes refuse. Picked-node execution is bound to the selected isolated context. Native picker retained.
- Element **Review for chat → Add to draft** freezes literal page/title/scope/selector/facts and exact root/session/draft/attachment revision. Separate page incarnation/navigation identity survives inspection-only cancellation but changes on **any frame navigation**. Final collection rejects stale/hidden/suspended/foreign-owner results and appends once without sending.
- Screenshot **disclosure → capture viewport → frozen crop/rectangle/arrow editing → Review → Add image to draft**. Dedicated raw-CDP transport, worker decode/render/encode, shared preview budget, 1 MP output/5 MiB PNG bounds. Own modal may hide native Chromium without self-cancelling capture. Cancellation drops only its own draft token; old utilities/results cannot consume a newer review.
- One **More browser tools** slot retains DevTools and groups screenshot, visited history and ephemeral groups. Editable popup stays open; whole menu scrolls within the viewport. Frame/shadow controls are collapsed. Quiet Mac confirmations/modal primitives and existing native CEF masking remain in use.
- Visited history is owner/group-scoped, memory-only, bounded to 100 URLs of 4096 bytes; labels omit queries/fragments. Default tabs use distinct in-memory contexts; up to eight named groups explicitly share one. Confirmed group changes stop affected tabs without reopening them. **Restored HTTP tabs now stay stopped until Resume**, and parked chat layouts do not retain pending browser approvals.
- Existing automatic inline preview callbacks are gated by live tab/document identity. Close/group disposal/owner parking clears cached preview title/pixels; replacement cache revisions cannot reuse stale GPU textures. This is not a global secure-erase claim.

## Final-source identity and environment

Private target `/tmp/pui-browser-completion-owned.hav6QQ`; test binary `debug/deps/pui-37472ebda07c3894`. Its dep-info contains `CARGO_MANIFEST_DIR=/Users/shai.cohen/personal/pui/.worktrees/browser-completion`; `--list` contains the new element capture, screenshot handoff, privacy integration and Files restore tests. Seeded dependency caches were retained, while package fingerprints/CEF CMake caches were forced to rebuild from this worktree before validation. No shared-checkout test binary was counted.

Stable Rust; CLT compiler/Git with SDKROOT, existing Android SDK CMake/Ninja 3.22.1, pinned CEF 156, Homebrew Python 3.13, Node 22.19.0; global/system Git config disabled. Commands/environment recorded in `/tmp/pui-browser-completion-env.sh`.

## Passing checks

- Final browser Rust: **99 passed** (`/tmp/pui-browser-completion-final-focused.log`). Includes actual egui Review/Add, literal provenance, keyboard/pointer, same-pass edits, child navigation, cancellation, screenshot UI/worker-to-parent exact PNG ingress, no RPC/duplicate send, real More popup typing, suspended dialogs, disabled/modal/theme/narrow/zoom tests.
- Draft owner/attachment bounds **3 passed**, restored Files web layout **1 passed**, inline preview replacement cache **1 passed** (`...-final-{draft,restore,cache}.log`). Changed-owner/draft/image tests seed an otherwise valid visible page rather than relying on hidden-page rejection.
- Strict `cargo clippy --offline --locked --all-targets -- -D warnings`: passed (`...-final-clippy.log`). `cargo fmt --all -- --check` and `git diff --check`: passed. Earlier Clippy timeout and ordinary lint failures retained in earlier logs, repaired without lint suppression.
- Full Node: **173 passed / 1 skipped**, 174 total (`...-full-js.log`); reader-specific **8 passed** (`...-reader-js.log`).
- Python: **41 passed** (`...-full-python.log`); both new launcher scripts parse. Mock release-test output saying “Published GitLab release v1.2.3” is not a deployment.
- All three Rust integration suites: **10 passed** (`...-final-tail.log`), including prompt-design guards.

## Actual disposable Chromium acceptance

User explicitly approved disposable debug/local testing. Built current source `pui` and `pui-browser-helper`, copied the pinned CEF via existing bundler, and ad-hoc signed `/tmp/pui-browser-completion-runtime-jlk_l6i7/Pui.app`. This browser-only demo bundle intentionally omits Pi/npm runtime packaging; no runtime install/download or provider launch was needed. Final signed executable SHA256: `e34e3e8105eb4580811f4ae3664c688941ab5318da12d412d8cb01e25d24d9c5`.

Both launchers use an isolated HOME/config and owned loopback fixture. They terminate only their own child. The existing installed application and other chats' builds/processes were untouched.

1. `scripts/smoke-browser-completion-macos.py`: actual isolated top/child/shadow reader, descendant exclusions, generated tag-minimal selectors, password refusal and cross-origin inventory exclusion passed. Native viewport capture while the native view was hidden decoded a bounded **423×522 PNG**, with exact black/white reference pixels. Evidence: `/var/folders/_f/77x7f3v56_zcs3szhzzvn_rc0000gp/T/pui-completion-acceptance-y5ixnoja/acceptance.json`; log `...-final-runtime-reader.log`.
2. `scripts/smoke-browser-privacy-macos.py`: actual RequestContext native equality/sharing, non-global/empty cache paths, Work sharing of HttpOnly cookies/localStorage/IndexedDB/CacheStorage, default/Other isolation, Work-only restart, Other retention, stopped Work peer, no server requests during the explicit Resume pause, and asynchronous browser teardown to zero all passed. Evidence: `/var/folders/_f/77x7f3v56_zcs3szhzzvn_rc0000gp/T/pui-privacy-acceptance-2b7ljqh3/acceptance.json`; log `...-final-runtime-privacy.log`.

Initial screenshot runtime attempts failed an exact CSS-red assertion: measured pixels `[202,59,66,255]` differed from CSS `[220,40,60,255]`, consistent with display-profile conversion. Failed evidence is retained (`.../pui-completion-acceptance-_id8eqq3`, `...-vsmr_14d`). Replaced the color-dependent fixture assumption with black/white viewport reference strips and kept exact pixel assertions. No production screenshot pixel conversion or decoder bound was weakened. The privacy probe also uses raw CDP IDs to avoid the already established reentrant ExecuteDevToolsMethod ID-rewrite problem.

These are real engine/transport/storage checks, not physical-pointer, screen-reader, full interactive macOS UI, live-provider, hostile-renderer or universal isolation proofs. Crop/annotation/draft handoff is covered by deterministic UI + real image-worker tests, not a physical end-to-end pointer session. Browser-visible ordinary text/identities/pixels can be sensitive; there is no secret detector or automatic redaction guarantee.

## Repository suite remains non-green

First complete-run attempt (`./scripts/test-rust.sh`, four bulk threads, `...-full-rust.log`) stopped after bulk **2062 passed / 7 failed / 2 ignored**. One failure was our stale toolbar expectation (“Open Chrome DevTools” instead of More); repaired and focused-tested. Six Git failures hit existing fixture/operation deadlines. No production deadline or assertion was relaxed.

After batched repairs, the runner's supported `PUI_TEST_THREADS=1` setting was used for a second final-source attempt (`...-final-rust.log`). Bulk: **2061 passed / 8 failed / 2 ignored**. All never-started serial groups and integration suites were then executed explicitly, preserving the failed bulk result (`...-final-remaining.log`, manifest `/tmp/pui-browser-completion-remaining.sh`). The 600-second outer tool bound interrupted tasks::apply after 16 completed passes. `/tmp/pui-browser-completion-tail.sh` excluded exactly those 16 completed tests and ran the six uncompleted tests, the two lock groups and all integration suites. The interrupted in-progress test was executed fresh; it is not counted as a completed first execution. No failed assertion was retried into a passing result. Logs/manifests retained.

Combined final coverage: **2250 binary tests passed / 11 failed / 2 ignored**, plus **10 integration tests passed**. Every group was exercised; this is **not a green full-suite run**.

Final failing tests:

- `app::changes::branches::tests::explicit_real_local_worker_completion_and_close_are_bounded` — fixture clone deadline.
- `workspace::branches::create::tests::target_submodule_refuses_without_checkout` — expected submodule diagnostic absent; exact returned error not exposed by assertion, cause unproven.
- `workspace::index_action::conflict_resolution::tests::exact_empty_no_newline_and_unicode_paths_stage_without_interpreting_pathspecs` — conflict inspection deadline.
- `workspace::index_action::conflict_resolution::tests::linked_worktree_resolution_uses_its_own_index` — conflict inspection deadline.
- `workspace::index_action::conflict_resolution::tests::stale_file_index_head_operation_foreign_lock_and_cancel_never_publish` — index inspection deadline.
- `workspace::index_action::discard::tests::restore_recovery_puts_back_original_working_content_and_keeps_copy` — conversion configuration deadline.
- `workspace::index_action::hunks::lines::tests::line_selection_refuses_invalid_ordinal_and_inactive` — current branch deadline.
- `workspace::index_action::hunks::lines::tests::unstage_selected_addition_only_preserves_other_staged_changes` — index entry deadline.
- `app::changes::commit::tests::native_commit_dialog_escape_themes_zoom_preserves_message_and_never_executes` — fixture clone deadline.
- `app::changes::git_files::modes::tests::real_mode_replacement_retains_old_body_until_success_and_filters_files` — index inspection deadline.
- `app::changes::index_actions::tests::restore_dialog_themes_zoom_escape_no_write` — index inspection deadline.

Other worktrees' Cargo/Git jobs were observed during validation but never stopped. Contention is not proven to explain failures. No untouched-baseline reproduction was performed for these current failures, and historical baseline results do not establish current causality.

## Failure ownership and explicit local commit scope

The user requested checking whether failures belong to this work or another agent, fixing ours, and otherwise committing only ours. The browser toolbar expectation was ours and was repaired. All final 11 failures are in pre-existing Git paths: `git diff 7a8b47c8b -- src/workspace src/app/changes scripts/test-rust.sh` is empty. Byte comparisons of the failing test files, relevant branch/conflict/discard implementation and shared Git fixture against the pulled baseline all match; SHA-256 evidence is retained in `/tmp/pui-browser-completion-failure-attribution.json`. No browser test, shared image cache test, draft handoff or restored browser test failed in final-source validation.

A separate `validate-conversational-workflows` worktree, also based on `7a8b47c8b`, currently modifies the Git fixture, commit/conflict/index/recovery code and related tests. A separate `git-activity-summaries` worktree modifies activity/transcript code. Those edits are not present in this browser branch, have not been copied, and will not be included in this local commit. This establishes ownership boundaries, not proof that another agent caused the deadline failures. Some Git implementation originated in older completed work by this chat; it is unchanged committed baseline, not this browser wave. No speculative Git repair or production timeout relaxation was made here.

Commit scope is the browser feature, its necessary draft/Files/session/preview integration, browser-only test expectation correction, debug acceptance probes and validation documentation. The repository failures remain disclosed; this is a user-authorized feature-only local commit, not a green release. No push/install/release authority is inferred. Broader Git completion and application profiling remain outside this browser wave; profiling stays deferred. Persistent browser profiles, closed/cross-origin reads, universal browser isolation, and physical/accessibility acceptance are not claimed.
