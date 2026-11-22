# Idle-agent push integration and timeout investigation

User requested a normal push after idle-process retirement, then explicitly held publication to investigate a Git fixture timeout. No failed snapshot was pushed.

## Integrated upstream

Normal merges retained concurrent configured-provider filtering, transcript progress animation, native browser picker/CDP transport, workflow recipes/plan-revision/capacity reporting and the shared Git pipe readiness fix. Latest integrated upstream is `8c730b1e60b7e0efec1a58a5f99f21ae4e3869c3`; merge source is `3d712929b1e821651314435805558f3dcce415fa` plus the fixture repair below.

Resolved three overlapping UI repairs deliberately: retained the already-validated single SVG preview/source toggle and keyboard fencing, and retained bounded focusable context/finished-task disclosure rows. Incoming per-agent UI ID scopes, welcome-row focus repair, inspector order and workflow changes remain intact. No prompt safety tests or controls were weakened.

## Investigation and concrete repair

The held failure was `workspace::push::tests::custom_routing_multiple_endpoints_and_embedded_credentials_refuse`, but its panic was `workspace/tests.rs:110` during `fixture index`, before Push's assertions. The tiny immutable seed has only README.md and side.txt, yet each clone launched rev-parse/read-tree/ls-files/two update-index commands, with the ls-files and skip-worktree step sharing one deadline plus intervening filesystem work.

The test-only fixture now prepares HEAD/index bytes once with the immutable seed and copies independent writable refs/index bytes into each disposable Git clone. Real Git clone/object/index/status/transaction behavior remains exercised; no hardlinked writable index and no production changes. A new regression modifies one clone's index, branch ref and file, then verifies the other and later clones retain their original independent sparse state. Focused workspace tests9/9 and Push18/18 passed; broader focused index tests31/32 had a separate inspection deadline failure, retained in `/tmp/pui-fixture-repair-focused.log`.

This removed unnecessary setup work, not proof of a universal root cause. Subsequent complete validation before the upstream pipe fix still had three production index-inspection deadlines and one commit-check failure. A separate bounded disposable diagnostic measured ordinary nine-command inspection at 0.123s at that moment; it did not reproduce the stall or establish OS/endpoint-security attribution. Data retained at `/var/folders/_f/77x7f3v56_zcs3szhzzvn_rc0000gp/T/pui-index-deadline-diagnostic-ly4ppohx/timings.json`.

Upstream then supplied a production pipe-readiness fix: `workspace/read_only.rs` uses poll on stdout instead of blind10ms sleeps while waiting, retaining cancellation, byte bounds, deadline and direct-child reaping. Integrated that concrete repair rather than inventing another timeout/scheduling exception. No process sampling/profiling, timeout increases, assertions weakened, failed-test retry loop or omitted runner group.

## Final combined validation — green

- Complete `PUI_TEST_THREADS=1 CARGO_BUILD_JOBS=2 scripts/test-rust.sh`: **2,145 binary + 10 integration passed**, 2 existing ignored, zero failures; all23 result groups completed. `/tmp/pui-idle-integrated-rust.log`.
- Formatting and strict offline locked all-target Clippy passed. `/tmp/pui-idle-integrated-{fmt,clippy}.log`.
- Full JS: **150 passed, 1 existing skipped**, zero failures. `/tmp/pui-idle-integrated-js.log`.
- Python: **41 passed**, fixture ResourceWarnings only; printed release publications mocked. `/tmp/pui-idle-integrated-python.log`.
- Git working/index whitespace checks passed. Existing Rust/CEF caches, explicit CLT SDK/Git, isolated `/tmp/pui-idle-timeout-build`, two build jobs and single-threaded bulk; no new installation.

Earlier integration failures are retained rather than claimed green: initial merge fmt/Clippy and3bulk subprocess failures (`/tmp/pui-idle-push-*`); next merge1fixture failure (`/tmp/pui-idle-push-final-*`); fixture-only repair3bulk index +1serial commit failure (`/tmp/pui-idle-investigated-*`). Every stopped run's remaining18serial groups and10integration tests was executed once. The final complete green source contains the later concrete shared pipe repair; this is not a claim that machine-wide intermittent delays cannot recur.

## Publication/build scope

Pre-push fetch observed upstream unchanged at `8c730b1e`, an ancestor of the integrated branch. Push must remain a normal fast-forward; never force. This record is not proof of push completion; use the actual Git push result.

No install, running-app restart, provider call or deployment occurred. `dist/Pui.app` remains the previously built `28dc27ec` source bundle, not the subsequently merged tip. User data, retained worktrees and unrelated processes were not deleted or stopped. No new package was built for this push-only follow-up.
