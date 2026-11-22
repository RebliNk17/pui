# Scheduled check provenance (bounded feature slice)

## Behavior and limits

- Scheduled monitor checks and explicit **Test once** capture launch-request identity before spawning: exact command-byte SHA-256, timeout, job UUID, creator's then-current agent ID, workspace/session/store path identities and launch-request UTC timestamp. The worker retains that snapshot; completion never reconstructs it from edited job settings. Session path, not the window-local agent ID, is the durable creator identity.
- Historical command text is **not duplicated**. The inspector shows a current command only if its exact digest, job UUID and store identity match the recorded claims. Changed/unavailable identities are explicit, not silently attributed to current settings. Path identities hash full native path bytes after canonicalization when available; failures explicitly retain the supplied path identity rather than claiming successful resolution. Excerpts are UTF-8-safe, capped at 128 bytes and marked when shortened. Paths are not filesystem-content snapshots; hashes and saved metadata are not authenticated/tamper-proof proof. Launch/completion wall clocks can move backwards; process elapsed time remains separately observed.
- Inline latest/prior check inspection is read-only. It distinguishes explicit origins from legacy unknown provenance; legacy `diagnostic: true` remains only an unbound legacy diagnostic flag. Missing exit codes no longer imply exit 0 or 1. Output/error truncation and legacy unknown completeness are explicit. Current pending/in-flight/error/acknowledgement totals are separate from historical checks and cannot establish delivery of an individual check or task verification.
- Existing latest + three prior checks remain the retention bound. Each provenance object is structurally validated and capped at 4 KiB serialized, at most 16 KiB/job added provenance (plus small evidence-field overhead); output remains 4,000 bytes/check and error 512 bytes/check. Existing 128-job and 4 MiB whole-store limits are unchanged. Whole-store overflow fails without evicting jobs or overwriting the durable store; existing scheduler persistence-failure behavior remains fail-closed. No new command/output capture policy, execution control, modal or transport action.
- Additive optional `provenance` and `error_truncated` fields keep `automation-v1/jobs.json`; no schema migration/version bump or legacy provenance fabrication. Older data defaults to unknown. Present malformed/over-bound provenance rejects store loading under the existing validation path. Old binaries may ignore/drop additive metadata on rewriting; this is not forward-version durable evidence. Restore/load/view cannot replay executable intent.

## Focused verification

Environment:

```sh
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/tmp/pui-ninja-recovery/bin:/tmp/pui-cmake-recovery/cmake/data/bin:/opt/homebrew/bin:$PATH"
export CEF_PATH=/Users/shai.cohen/personal/pui/.worktrees/in-app-file-browser/target/cef-download/cef_binary_156.0.26+g8ea0c73+chromium-156.0.8078.88_macosarm64_minimal
export CARGO_TARGET_DIR=/tmp/pui-audit-agent219-target CARGO_BUILD_JOBS=2
cargo --config 'http.proxy=""' test --offline --locked --bin pui app::automation:: -- --test-threads=1
cargo --config 'http.proxy=""' test --offline --locked --test prompt_design
```

- Final repaired automation: **61 passed**, 0 failed (1,940 filtered out). Log: `/tmp/pui-audit-agent219-repair.log`. Initial completed slice passed 60/60 in `/tmp/pui-audit-agent219-focused.log` before parent review required the render-path repair below.
- Native prompt guard: **3 passed**, 0 failed. Logs: `/tmp/pui-audit-agent219-prompts.log` (initial) and `/tmp/pui-audit-agent219-repair-prompts.log` (repair).
- Targeted `rustfmt --check --edition 2024 --config skip_children=true` and `git diff --check`: passed. `view.rs` contains only scoped comparison action/invalidation wiring and the call-site edit, preserving existing shared-file formatting.
- New deterministic coverage: immutable latest/prior snapshots through content/command/timing edits, restart round-trip and creator-ID remapping; exact-byte command mismatch and wrong-job/store refusal to display current command; legacy import/defaults and malformed import rejection; canonicalization failure and path/serialized bounds; overflow preserves previous store; observer inspection under a second window's held OS lock; literal rendering in light/dark, 320-point narrow width, 0.8/1.4 zoom, enabled/disabled; stale diagnostic completion discarded; current delivery changes leave historical provenance unchanged. Existing tests cover Test once never-delivery, cancellation, permissions/demo, store lock/restart uncertainty, and native cancellation/modal controls.

## Parent review repair: no per-frame identity collection

The initial inspector incorrectly recaptured current identity on every expanded frame. Replaced it with one bounded, nonserialized `Comparison` snapshot. **Compare current identities** explicitly collects once after render closures in the same UI event turn; opening/idle rendering does not canonicalize or hash anything. This explicit collection is synchronous on the UI thread, not a worker/nonblocking operation: canonicalization can wait on an ordinary userspace filesystem (including unavailable/network mounts). No latency guarantee is claimed; moving this optional comparison action to a bounded worker is a remaining UI-responsiveness improvement. Rendering uses only cached captured values and exact input comparisons. Command, timeout, job ID, creator ID, workspace/session path, store path or OS-lock ownership changes invalidate the cached comparison; recomputation requires another explicit action. No timer, filesystem watch, execution or delivery is introduced. Stored historical evidence is immutable. The labels explicitly say **last explicit comparison**, include its timestamp, and disclose that external filesystem changes require refresh.

New deterministic regression instruments path collection and digest calls: one explicit comparison performs three path collections/four hashes with a store; six repeated direct inspector frames and six expanded-history frames perform **zero**. Each identity/store/owner edit produces unavailable/invalidated state without collection or displaying a stale match/current command; only explicit collection recomputes. The UI remains available to read-only scheduler observers. One current command copy (maximum 16 KiB) exists only in the single in-memory comparison for exact invalidation; it is never persisted.

## Parent integrated validation

Reviewed with all four parallel slices on base `256003a8` in `parallel-features-integration`. Complete combined validation: **2,032 Rust binary + 10 integration tests passed**,2ignored/zero failures; **114 JS passed/1skipped**, **41 Python passed**, strict all-target Clippy, formatting, shell syntax and diff checks passed. Logs `/tmp/pui-parallel-{rust,js,python,clippy,fmt}.log`. Two test-only Clippy style fixes and automation-view formatting were made during integration; no assertions/deadlines/scheduling weakened. Evidence applies to the complete combined source, not a separate full-suite run per intermediate feature commit. See `parallel-features-integration-validation.md` for integration identity and remaining acceptance gates.

## Earlier failures and recovery (not green runs)

- Initial Cargo invocation failed before compilation: missing `http.proxy` configuration; explicit command-local empty proxy resolved this.
- Seeding the separate target via `cp -cR` timed out on the large tree. It left a partial independent target, not modified shared build files. The `debug/build` subtree was then copied separately.
- The seeded CEF CMake cache named the original target and failed configuration. Only the **new isolated target's** `out/build` was moved to `out/build-seed-cache`; clean CEF configuration/build then completed. A bounded 240-second build call timed out; subsequent build completed.
- First code compile found the test helper using `FnOnce` inside egui's `FnMut` frame callback; corrected the helper to `FnMut`.
- First executing run: **57 passed / 1 failed**. Overflow test data was below 4 MiB, so expecting refusal was wrong; corrected the fixture with valid maximum-size command/prompt data, preserving the production cap/assertion. Next run passed 58/58, then two additional deterministic tests produced the final 60/60.

No full suite, profiling, app restart, live user-job execution, provider calls, commit or cleanup performed. Parent owns integrated full validation and commits. Remaining gaps: named timezone/DST scheduling, authenticated append-only audit, per-check delivery receipts and deeper task/result verification are not implemented. Hashes identify captured path/command claims, not contents, shell environment or race-free filesystem identity.
