# Four parallel feature slices — integrated validation

Base `256003a8ca8773396d75ed48a0aedd87c72812dd` includes upstream integration `c55c172a` and the separately reviewed Git conflict-resolution feature. Work performed in isolated `parallel-features-integration`, leaving original checkout/concurrent task files alone. User explicitly requested three workers, then a fourth for chat naming. Workers delivered uncommitted isolated diffs; parent inspected, requested repairs, combined and validated them before making scoped local feature commits.

## Local feature commits

- `51b1c3e`: bounded native browser selector inspection. Separate DevTools retained, typed/bounded read-only direct-text/style/geometry snapshots, navigation/request/owner lifecycle cancellation. **Actual CEF transport/isolated-world runtime acceptance remains unverified**: needs matching bundle/helper/framework; no package/install or installed-app restart authorized. Deterministic DOM stand-ins are not security/runtime evidence. Click picking, frames/shadow roots and draft handoff deferred.
- `861de70`: immutable scheduled/Test once execution provenance, legacy unknown origin, bounded latest+3 history, explicit cached current-identity comparison. Parent rejected initial per-frame filesystem canonicalization/hashing; worker repaired it with instrumented zero-collection render tests. Explicit comparison still synchronously canonicalizes on the UI event turn and may wait on filesystem access. No per-check delivery receipt/authenticated audit/task-verification claim.
- `48f2813`: explicit loaded Markdown Choose blocks → Review → Add to draft. Activation-time exact owner/document/draft, bounded provenance/whole-or-reject/exactly-once/no-send. Loaded preview, not external-disk watcher; transcript substring/multi-message/PDF/Mermaid/media gaps remain.
- `e71fbea`: short (48 Unicode scalars) first-request automatic title, immutable display assignment across compaction/restart/metadata; explicit user rename remains authoritative. Inert SDK extension marker persists together with first user entry, not independently at command acceptance; real offline RPC ordering and temporary SessionManager compaction/reopen tested, no provider calls. Unknown/legacy titles retained, not bulk-migrated.

## Review/integration corrections

- Automation inspector no longer hashes commands/canonicalizes paths every frame; only explicit comparison collects and exact input changes invalidate cache.
- Session discovery validates saved automatic markers as strictly as live titles, including cap/digest/control/format boundaries. Added actual offline RPC sequencing rather than relying only on stub command order.
- Markdown capture freezes draft at pointer/keyboard activation before later frame edits; retains explicit fresh document identity and offscreen/multipass coverage.
- Browser hides cancel data without dropping editor keyboard ownership. Untrusted controls/bidi escaped; ordinary RTL retained. Runtime acceptance not fabricated from older bundles.
- Integrated `cargo fmt --check` required automation-view formatting. Strict Clippy found two test-only style issues: redundant let-return in Markdown fixture and field-reassign-with-default in browser fixture. Fixed styles, no assertion/deadline/scheduling changes. Failed pre-suite Clippy attempts are not green runs. Complete suite below ran once after those fixes.

## Complete combined-source validation

Source content now recorded by `e71fbea` (per-feature validation-doc additions afterward were documentation only). All four slices were present for the run; no claim of separate full-suite execution at each intermediate feature commit.

- `./scripts/test-rust.sh`: **2,032 binary + 10 integration passed**, 2 ignored, zero failures. Bulk1842 + all189serialized tests, each once; integrations4horizontal+3vertical+3prompt. `/tmp/pui-parallel-rust.log`.
- `cargo clippy --offline --locked --all-targets -- -D warnings`: passed. `/tmp/pui-parallel-clippy.log`.
- `cargo fmt --check`, `git diff --check`, `bash -n scripts/bundle-macos.sh`: passed. `/tmp/pui-parallel-fmt.log`.
- `node --test tests/*.test.mjs`: **114 passed, 1 skipped**. `/tmp/pui-parallel-js.log`.
- `/opt/homebrew/bin/python3.13 -m unittest discover -s tests -p 'test_*.py' -v`: **41 passed**. `/tmp/pui-parallel-python.log`. Printed release-publication messages are mocks, not deployments.

Environment: resolved CLT Git/compiler, explicit SDKROOT from xcrun, stable arm64 Rust, existing pinned CEF distribution, recovery Ninja/CMake, Homebrew Python3.13, isolated global/system Git config; `CARGO_TARGET_DIR=/tmp/pui-job-delete-validation`, build jobs2, SDK1.5.2 via `PUI_PI_SDK`. Existing external target lock was waited on, not killed. Worker original-source hashes/paths retained in `/tmp/pui-parallel-integration-sources.json` (local evidence only).

No push, package/build-release, installation, app restart, hosted CI claim, live provider/site/job execution or application profiling. Native injection/transport fixtures are not physical user acceptance. AccessKit remains disabled. Remaining scope recorded in each feature validation note and Git checklist; this wave does not finish the broader roadmap.
