# Merged PR CI refresh

## Fix

A merged PR's first status snapshot was previously immutable in memory and on
disk, and both automatic and manual refresh skipped it. If CI was still running
at merge time, the spinner could persist forever, including after restart.

- Protect the merged lifecycle state without freezing its CI results.
- Continue 30-second polling for merged PRs with running, unknown or unavailable
  checks, or transient refresh errors. Stop automatic polling once checks reach
  a terminal result; manual Refresh can discover reruns for any referenced PR.
- Permit newer merged snapshots in the private disk cache while rejecting stale
  timestamps and any attempt to overwrite merged state with non-merged state.
- Keep last known state/checks on transient failures.
- Aggregate mixed failed/running jobs as running until every job completes;
  merging a PR never fabricates passing CI.
- Update `docs/reference.md` to describe the actual 30-second interval and new
  refresh behavior.

## Verification

- Focused `pulls::tests::`: **22 passed**.
- Complete `bash scripts/test-rust.sh`: **2,358 passed, 0 failed, 2 existing
  ignored**, all groups included once. `/tmp/pui-merged-ci-full-rust.log`.
- `cargo clippy --offline --locked --all-targets`: completed with existing
  cloud/remote warnings. `/tmp/pui-merged-ci-clippy.log`.
- Targeted rustfmt and `git diff --check`: passed.

New tests cover pending merged CI through disk restart, expiry/deadline/schedule,
offline refresh, completion and persisted update, stale cross-window writes,
manual refresh discovering a rerun, unknown/terminal CI states, and mixed jobs.
Existing tests continue to protect merge permanence, bounded/deduplicated workers,
cache permissions, demo isolation, link actions and compact rows in both themes.

Tests use fixture data, not live GitHub calls. The actual CI state of the PR in
the screenshot was not independently queried. No installation, restart or release
publication was performed; the installed app needs a new build for this fix.
