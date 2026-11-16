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

Runs bulk tests with four threads, then the task watch-scope, task refresh, session watcher, watcher lifecycle, cross-window sync, table projection and table export groups serially. Real OS watcher setup previously timed out under bulk application-context registration load (`src/file_watch.rs:72`); watcher groups now run once serially with unchanged setup deadlines and assertions. This is scheduling mitigation, not proof of an OS-service repair or zero flake rate. Export tests also use the projection worker permit; a later full run exposed that contention, so the entire export group is included serially rather than retried or omitted. All groups are included exactly once; existing ignored tests remain ignored. Hermetic Git config prevents developer-global filters from changing fixtures. This is a local validation runner, not a change to application Git permissions or installed configuration. Task ownership-lock handoff and the exact automation restart/OS-lock regression also run in separate serial invocations after process-spawning groups, preserving upstream fork/exec lock-inheritance protections.

### Measured on current dirty checkout

- Previous serial full run: **1,658 Rust + 3 prompt passed**, 2 ignored, **492.97s** Rust-body time (`/tmp/pui-chat-resource-full.log`).
- Four-thread unsplit experiment: **147.07s** Rust-body / **158.01s** wall, but **6 failures** (5 sync timeout cases, 1 projection assertion). Not an accepted run. `/tmp/pui-suite-four-threads.log`.
- Split run: **1,614 bulk + 28 sync + 18 projection = 1,660 Rust**, plus **3 prompt tests**, 2 ignored, **171.58s total wall**. Bulk129.40s, sync34.66s, projection4.26s. **Zero failures.** `/tmp/pui-suite-split.log`.

About **65% shorter / 2.9× faster** than the prior ~493-second serial body, despite comparing total split wall against serial test-body time. Compilation/cache/load affect timings; not a guaranteed CI duration or repeated flake-rate proof. The split command was measured directly; the checked-in runner contains the same commands and passed `bash -n`.

Remaining optimization candidates from read-only audit: repeated per-context font parsing, Git fixture setup, and excessive UI settling frames. These are hypotheses, not measured attribution; profile before changing them. Production shutdown deadlines account for some necessary test time and should not be shortened for test speed.
