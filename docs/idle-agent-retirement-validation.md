# Settled-agent idle process retirement

Based on `6b73fd37` (latest-master validation repairs). User requested retiring finished agents after 5–10 minutes while keeping running agents and questions alive; implemented a fixed **10-minute** grace period.

## Behavior and safety

- Selected and parked regular agents/root chats qualify only after canonical `agent_settled`, connected/readiness checks and no live run/tool/retry/compaction. Activity/new work/input waits reset the countdown. Idle wakeups use egui deadlines, not recurring polling or model calls.
- Questions/approvals, loading/history/resource operations, pending/uncertain deliveries, unreported results, agent waits/mail, active descendants/dependents/workflows and enabled linked automation prevent retirement. New RPC records are drained and eligibility rechecked before shutting down.
- Graceful existing RPC shutdown/reaping executes off the UI thread. Single-writer ownership stays held until process exit; reconnect waits for that exit. Chats, in-memory drafts/images/history/results and worktrees are not deleted. Status is `Idle · process released`, distinct from explicit Stop.
- Selecting a dormant chat reconnects read-only; an explicit user Send is retained in the ordinary waiting queue. Managed followups use a bounded FIFO and keep completion-report correlation until actual RPC acknowledgment. Nothing is sent until history/state/permission-adapter readiness; questions/errors/uncertain state block dispatch. Failed/uncertain delivery is not automatically replayed. Explicit Stop cancels unsent reconnect mail and prevents navigation/managed-message restart; manual resume remains available.
- Conservatively defer retirement when other Pui windows are present or live ownership/presence is uncertain. Existing three-warm-root cache remains separate. Shutdown waits for tracked retirement workers; no unmanaged agent launcher introduced.
- Main implementation: `src/app/agents/idle.rs`; deterministic and real-process coverage: `src/app/agents/idle/tests.rs`. The existing deterministic Python transport adds its PID to `get_state` solely to verify real process reaping without provider calls.

## Validation

- Focused agent suite: **36 passed**, including **14 new idle tests**: precise deadline/reset, selected/parked/root handling, active workflows/dependencies, tools/retries/compaction/questions/approvals, pending work, cross-window uncertainty, drafts/results/worktree preservation, late question drain, explicit Stop, reconnect/adapter fencing, FIFO/single delivery and a real child reaping check. `/tmp/pui-idle-agents.log`.
- Final complete `PUI_TEST_THREADS=1 scripts/test-rust.sh`: **2,129 binary + 10 integration passed**, 2 existing ignored, zero failures; every group ran, 23 result groups. `/tmp/pui-idle-finished-rust.log`.
- Strict offline locked all-target Clippy and formatting: passed. `/tmp/pui-idle-finished-clippy.log`.
- Full JS (including nested Cargo wire replay with configured build environment): **148 passed, 1 existing skip**, zero failures. `/tmp/pui-idle-finished-js.log`.
- Python discovery: **41 passed**; fixture warnings only and release publications mocked. `/tmp/pui-idle-full-python.log`.
- Git whitespace checks passed. Direct diff review; no independent managed reviewer is claimed (installed host is still the old retained-session-limit build).

Earlier full run: bulk **1,935 passed, 2 failed, 2 ignored** at unchanged Git push inspection deadlines; all remaining groups were run once afterward (**191 binary + 10 integration passed**) without retrying failures. Initial remaining-command generation had a quoting error before those 18 groups ran; the corrected temporary driver executed each once. Logs: `/tmp/pui-idle-full-rust.log`, `/tmp/pui-idle-remaining-rust.log`, `/tmp/pui-idle-full-rust-tail.log`. To address excess subprocess work, Push routing now takes one bounded NUL-framed query rather than four under the same deadline. Exact branch/remote matching and duplicate/empty routing refusals are tested. Focused Push then passed 17 tests but encountered a separate fixture-clone timeout; not hidden or automatically retried. The final complete green run followed the concrete routing repair, with production deadlines, safety assertions, runner scheduling and ignored tests unchanged.

Used existing Rust/CEF/tool caches and an isolated cloned target `/tmp/pui-idle-timeout-build` to avoid concurrent-target interference. No profiling, provider requests, session/worktree deletion, installed-app replacement/restart, push or deployment. Remote tracking advanced during local work; concurrent upstream work was not automatically merged or overwritten. Packaging is recorded below.

## Release bundle

- Source: `28dc27ec043e661c973108b0427fa9628ef241bb`; clean tree at packaging. Release arm64 bundle built with `scripts/bundle-macos.sh` at `dist/Pui.app`. `/tmp/pui-idle-release-bundle.log`.
- Binary SHA-256: `169dcee5a0954fa084955f2ab035cb524bd9bffcd5c2a6b4d87525d308a73641`.
- Ad-hoc signature: `codesign --verify --deep --strict dist/Pui.app` passed. Not Developer ID signed or notarized.
- Fresh-HOME offline relocated runtime smoke passed: bundled Node 22.23.5, Pi 1.5.2, SDK/native/WASM/search tools, RPC adapter and resources. `/tmp/pui-idle-runtime-smoke.log`. No provider requests.
- Previous local bundle retained at `dist/Pui.previous.ZOlKLJwk/Pui.app`. `/Applications/Pui.app` and its November 18 running process were not replaced, stopped or restarted.
- At completion local `master` has the two task commits while `origin/master` advanced three commits during work. No automatic merge, push or claim of including those later upstream changes. A later documentation-only receipt commit does not change the packaged source.
