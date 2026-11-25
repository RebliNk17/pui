# Update refresh and composer placeholder fixes

Validated against `master` base `2ff97185`.

## Changes

### Downloaded-update refresh

`src/app/updates.rs` previously skipped manual checks when an update was staged
and hid refresh after finding an available update. Periodic checks could also
replace the metadata used to label the staged bundle.

- Opening Updates now checks again even with a staged download; Check again
  remains available for offered and staged updates.
- Downloaded version/notes are tracked separately from later check results.
- A strictly newer stable result discards the superseded staged download and
  offers the newer release without automatically downloading or installing it.
- Failed, equal-version and older results preserve the verified staged bundle;
  failed checks are disclosed alongside the staged-update information.
- Checks and downloads do not overlap. Download/install actions are disabled
  while checking; cancellation clears the download's metadata.
- Refresh uses its own shared Quiet Mac footer row so all actions fit narrow
  windows and higher zoom levels.

Four new regression tests cover replacement/cleanup, failed/equal/older checks,
check/download exclusion and cancellation, and light/dark narrow-window UI at
three zoom levels with disabled-install and Escape behavior.

### Composer placeholder alignment

`src/app.rs` now retains `TextEditOutput` for hint rendering.
`src/app/composer_hints.rs` anchors placeholder text and shortcut hints to the
editor's actual `galley_pos` and `text_clip_rect`, rather than its surrounding
scroll viewport. TextEdit retains text margins with `frame(false)`; ignoring
those margins put the placeholder above and left of the insertion point.

The new regression compares placeholder, caret and typed-text positions in
light/dark themes, at 0.8/1.0/1.5 zoom, for regular and side-chat widget scopes,
including demo placeholders. Existing rotation, truncation, focus, draft and
Shift+Enter tests remain unchanged and pass.

## Validation

- Focused composer hints: **11 passed**.
- Focused updates: **13 passed**.
- Complete `bash scripts/test-rust.sh`: **2,355 passed, 0 failed, 2 existing
  ignored tests** (2,345 application tests and 10 integration tests). All
  contention-sensitive groups ran serially as configured; no retries or skips
  were added. Log: `/tmp/pui-placeholder-updates-full-rust.log`.
- Targeted rustfmt check for composer hint source/tests and updates: passed.
  The existing large `src/app.rs` file received only three targeted edits, not
  an unrelated formatting rewrite.
- `git diff --check`: passed.
- `cargo clippy --offline --locked --all-targets`: completed successfully with
  existing warnings. Log: `/tmp/pui-placeholder-updates-clippy-default.log`.
- Strict Clippy (`-D warnings`) failed on existing cloud/remote code: identical
  conditional branches in `src/app/views.rs`, receiver type complexity in
  `src/app.rs`, argument count in `src/rpc.rs`, and unused functions in
  `src/remote/{cloud,ws}.rs`. These locations were verified in the base commit;
  no lint suppressions or unrelated repairs were added. Log:
  `/tmp/pui-placeholder-updates-clippy.log`.

## Earlier unsuccessful attempts

Initial attempts hit missing proxy configuration, a CEF certificate download
failure, absent CMake on PATH and unavailable Auto classifier checks. Using the
existing cached CEF runtime plus existing CMake/Ninja recovered validation;
no tools were installed and no certificate checks were disabled.

The first executable update test run exposed a narrow-footer overflow
(**12 passed, 1 failed**). Moving refresh into its own shared footer row repaired
it; focused tests and the complete final suite then passed without weakening
assertions. `/tmp/pui-update-refresh-focused.log` records that initial failure.

## Boundaries

No app installation/restart, release publication, version bump, live GitHub
release lookup, or live-app screenshot validation was performed. The installed
application is unchanged; these fixes require a subsequent build/release to
reach it. No JavaScript/Python production code was changed by these fixes.
