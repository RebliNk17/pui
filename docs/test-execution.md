# Test execution

## Development policy

- During implementation: run relevant focused tests (for example `cargo test --offline --locked --bin pui chat_drag::`). Batch related changes.
- At feature completion / before integration or packaging: run the complete suite once, plus the applicable lint/JS/Python/UI checks.
- Do not rerun the full suite or rebuild packages after every minor change or documentation/test-label adjustment. A failure requires targeted investigation; rerun affected checks and final full validation after repairs are batched.
- Preserve test assertions, production deadlines, real process/reaping checks and resource limits. Do not turn failures into retries or silently skip coverage.

## Faster complete Rust run

Configure the existing toolchain/CEF and target directory, then:

```sh
./scripts/test-rust.sh
```

Runs bulk tests with four threads, then the contention-sensitive groups (task watch-scope, task refresh, session watcher, watcher lifecycle, cross-window sync, table projection, table export and the groups below) in single-threaded processes; see “Serial phase” below. Real OS watcher setup previously timed out under bulk application-context registration load (`src/file_watch.rs:72`); watcher groups now run once serially with unchanged setup deadlines and assertions. This is scheduling mitigation, not proof of an OS-service repair or zero flake rate. Export tests also use the projection worker permit; a later full run exposed that contention, so the entire export group is included serially rather than retried or omitted. All groups are included exactly once; existing ignored tests remain ignored. Hermetic Git config prevents developer-global filters from changing fixtures. This is a local validation runner, not a change to application Git permissions or installed configuration. Task ownership-lock handoff and the exact automation restart/OS-lock regression also run in separate serial invocations after process-spawning groups, preserving upstream fork/exec lock-inheritance protections.

### Serial phase: exclusive groups and parallel lanes

After the bulk pass, `scripts/test-rust.sh` runs the contention-sensitive groups in three phases. Each group is still its own `cargo test --bin pui GROUP -- --test-threads=1` process and runs exactly once; the bulk `--skip` list is generated from the same group arrays, so a group cannot be scheduled without being excluded from bulk.

1. **Exclusive, first:** the four real OS watcher groups, with no other test process running.
2. **Four concurrent lanes** for the remaining groups. The recorded failures were in-process: Git fixture deadlines and Python helper startup under four-thread bulk load, the process-global projection permit, and fork/exec inheriting ownership-lock descriptors from sibling test threads. One test per process at a time preserves all of these; descriptor inheritance and permits are per process. `tasks::apply::tests::` is split into its `additional::` submodule and the rest (two processes, same tests). Lanes are balanced by duration measured with 3 free cores; `PUI_SERIAL_LANES=1` runs them sequentially.
3. **Exclusive, last:** the exact automation restart/OS-lock and owner-exit lock takeover tests.

The cross-window `shared_queue_edit_remove_and_stale_revision_are_authoritative` test now waits, while both windows are still polled, for the mirror to finish fetching the owner's post-edit revision. Previously that fetch was often still in flight at teardown and the dropped mirror waited out the owner's unchanged 8s acknowledgement deadline (8.1s → 0.17s; 20/20 repeated runs passed).

Measured serial-phase wall time (after bulk, binary already built, same 192 tests; dev laptop with 14 cores):

| Condition | Before | After |
|---|---|---|
| Idle | 88.2s | 24.4–33.6s (3 runs) |
| 11 CPU burners (≈3 free cores, CI-like) | 128.9s | 37.6–41.1s (3 runs) |
| 13 CPU burners (≈1 free core) | — | 48.4–50.2s (2 runs) |
| `PUI_SERIAL_LANES=1`, idle | 88.2s | 65.0s (sync fix only) |

Zero failures in all 9 lane runs. The test set equals the previous runner's serial set, and bulk + serial cover all 2,342 listed binary tests exactly once. On CI the slowest lane bounds the phase (about `tasks::apply` halves + one more group). This is scheduling, not a deadline or assertion change, and not a proof of zero flake rate.

### Index-action UI fixture scheduling

The unified release run recorded a3s Git preview deadline in the index-action Escape/theme fixture under bulk concurrency. The six `app::changes::index_actions::tests::` tests now run once serially, excluded from bulk. Assertions and production deadlines are unchanged; no retries or omitted coverage. Complete subsequent validation passed. See `docs/unified-master-release-validation.md`.

### Additional process-sensitive serial groups

The History→branch validation run recorded two existing Git fixture 3-second deadline failures and two SVG Python helper protocol-start failures during four-thread bulk execution. The commit-dialog, Git full-review-mode and SVG loader test groups now run once serially, excluded from bulk, before ownership-lock groups. Assertions, deadlines, Python child protocol and real reaping checks are unchanged. This is a conservative scheduling mitigation, not established root-cause attribution or a production timeout/helper repair. No failed test is automatically retried.

### Measured on current dirty checkout

- Previous serial full run: **1,658 Rust + 3 prompt passed**, 2 ignored, **492.97s** Rust-body time (`/tmp/pui-chat-resource-full.log`).
- Four-thread unsplit experiment: **147.07s** Rust-body / **158.01s** wall, but **6 failures** (5 sync timeout cases, 1 projection assertion). Not an accepted run. `/tmp/pui-suite-four-threads.log`.
- Split run: **1,614 bulk + 28 sync + 18 projection = 1,660 Rust**, plus **3 prompt tests**, 2 ignored, **171.58s total wall**. Bulk129.40s, sync34.66s, projection4.26s. **Zero failures.** `/tmp/pui-suite-split.log`.

About **65% shorter / 2.9× faster** than the prior ~493-second serial body, despite comparing total split wall against serial test-body time. Compilation/cache/load affect timings; not a guaranteed CI duration or repeated flake-rate proof. The split command was measured directly; the checked-in runner contains the same commands and passed `bash -n`.

Remaining optimization candidates from read-only audit: repeated per-context font parsing, Git fixture setup, and excessive UI settling frames. These are hypotheses, not measured attribution; profile before changing them. Production shutdown deadlines account for some necessary test time and should not be shortened for test speed.

## CI: optimized dependencies and affected suites

CI installs `scripts/ci/cargo-config.toml` as `$CARGO_HOME/config.toml`: dependencies build at
`opt-level = 2` (the `pui` crate stays unoptimized) with line-table debug info. CPU-bound UI,
render and SVG tests run ~2.4× faster. Locally, the complete `scripts/test-rust.sh` under this
config passed twice: 2,350 passed, 0 failed (2m31s including compile, then 1m17s). Settle tests
that raced background thumbnail decodes now wait via `media::wait_decodes`. Developer builds
are unchanged.

Pull requests and `master` pushes run only affected suites (`scripts/ci/affected.py`, diffing
against the PR base or the previous head). Only release tags and manual runs build the app bundle,
together with every suite. The always-reporting **CI result** job is the single required check for `master`.
