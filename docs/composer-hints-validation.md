# Composer hints: rotating placeholder + quiet inline action

## Scope

Based on `dd1f049f` after a fast-forward pull of master.

- Remove the complete permissions/shortcut footer; leave the existing permission picker and confirmation behavior unchanged.
- Paint an empty-input placeholder at left and a static Enter keycap/action at right. No draft width is reserved for either overlay.
- Rotate secondary hints every five seconds while empty, unfocused and in an active window. Pause on focus, while drafting, in ordinary demo mode, or when egui animations are disabled. Resume with a full interval, without replaying missed rotations. Reset on session/action changes.
- Reflect idle Send and busy Steer/Queue settings, including the opposite delivery shortcut and platform-specific Command/Control labels.
- Retain shortcut help in the send tooltip and empty-editor widget metadata. Preserve the editor's input, Markdown, recall, scrolling, completion and send handlers.
- `--demo-composer-hints` shows the actual rendering safely in demo mode, with sending disabled and no Pi client/provider calls.

## Passing validation

- Focused composer regression filter: **59 passed**, zero failed, including ten new hint tests. `/tmp/pui-composer-regression.log`.
- Existing inline approval placement/click test passed after adapting its placeholder expectation to the new busy-state text (also included in the composer filter).
- Complete runner's bulk phase: **2,008 passed, zero failed, two existing ignored**, plus **10 integration tests passed**. `/tmp/pui-composer-full-rust.log`. This was the final implementation, including the dedicated native demo and widget metadata.
- Strict all-target Clippy passed on the initial implementation before the dedicated demo/widget metadata addition: `/tmp/pui-composer-clippy.log`. The final attempt did not complete; do not interpret the earlier pass as final-source strict lint acceptance.
- Final `cargo fmt --check` and `git diff --check` passed.
- Python tests: **41 passed**. `/tmp/pui-composer-python.log`. Release publication output is mocked; no release was published.
- Native debug build succeeded and actual egui screenshots were captured and inspected in both themes: `/tmp/pui-composer-native-dark.png`, `/tmp/pui-composer-native-light.png`. No app installation, bundle replacement, live-agent requests or profiling performed.

## Incomplete/unavailable validation — not a full-suite green result

- The complete Rust runner exceeded its 600-second command bound in the first serial watch-scope test, after the passing bulk/integration phases. Later attempts, including a 1,200-second full invocation, stalled during toolchain execution before tests. An additional direct invocation of the already-built test binary also exceeded its bound before printing tests. These are recorded validation blockers, not proven feature defects or successful runs. No assertions, production deadlines or test groups were weakened or removed.
- JavaScript bounded full attempt: **168 passed, zero failed assertions, one existing skip, one cancelled file**. `tests/streaming.test.mjs` exceeded 60 seconds while spawning Cargo to replay a wire fixture. `/tmp/pui-composer-js-final.log`. The first unbounded JS invocation also exceeded its outer command bound. This is not a passing complete JS suite.
- Final strict Clippy exceeded its command bound without output: `/tmp/pui-composer-clippy-final.log`.
- Independent managed review could not start: Pui reported its retained-agent-session limit. No unmanaged agent fallback was used. The implementation diff and native screenshots were reviewed directly.

The feature has focused and native UI evidence, but complete Rust/JS validation and final-source strict lint acceptance remain outstanding. Keep these limitations attached to the source change; do not describe it as a fully validated release.
