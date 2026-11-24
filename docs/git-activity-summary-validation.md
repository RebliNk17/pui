# Compact Git activity summaries

## Behavior

Normal transcript activity headers replace generic command counts for recognized completed literal Git actions. Examples: `Committed and pushed to master`, `Pushed to main`, `Fetched origin/master`, `Switched to develop`, `Push to master rejected`. Mixed activity remains on one line, such as `Pushed to main · ran 1 command`. Existing failure badges, expandable raw commands/results, one-row elision and full-label hover text are retained.

This is deterministic display-only parsing; it executes no Git commands and changes no permission behavior. It recognizes common commit/push/fetch/pull/merge/switch/checkout/cherry-pick/add forms, quoted arguments, `-C` directories, and literal shell chains. Command status and attributable Git output distinguish completion from failure; branches are never taken from the currently selected workspace. Matching successful adjacent commit/push actions combine only for compatible repositories, branch targets and push sources. Detached commits do not invent a branch.

Unsupported scripts, wrappers, PowerShell, dry-runs, ambiguous output attribution, expansion, pipelines, redirections, unsupported flags and uncertain directory transitions fall back to generic command counts. This intentionally favors incomplete descriptions over false success claims. Codemode children use their own recorded status; nested history without retained output can omit branch detail or use generic fallback for partially failed chains. These labels describe recorded commands, not independent proof of repository/remote state or which exact commit a server retained.

## Final focused validation

- `cargo test --offline --locked --bin pui activity::git:: -- --test-threads=1`: **11 passed**.
- `cargo test --offline --locked --bin pui transcript:: -- --test-threads=1`: **55 passed, 1 existing ignored**. Includes the new actual activity-header UI fixture in light/dark, 240/900 widths and 1x/1.5x zoom, single-row bounds and expand-to-raw-command interaction.
- `cargo test --offline --locked --test prompt_design`: **3 passed** (before the final parser-only barrier fix).
- `cargo clippy --offline --locked --all-targets -- -D warnings`: **passed** after the final fix.
- Targeted rustfmt and `git diff --check`: passed.
- Independent read-only review found output-attribution, directory identity, bundled dry-run, HEAD/detached attribution and combination-barrier edge cases. Fixed with explicit regressions; the final mixed-chain regression and all focused checks pass. Reviewer did not execute tests.

Logs: `/tmp/pui-git-summary-{final-focused,transcript,ui,prompts,clippy-final}.log`.

## Complete-suite limitations

`./scripts/test-rust.sh` attempted once: bulk **2,056 passed, 2 failed, 2 ignored**. The two push-hook fixtures timed out during destination inspection. All scheduled serial groups were then attempted in order without retrying failed tests; commit-dialog, Git mode and index-restore fixtures hit unchanged Git inspection deadlines. That continuation hit its 600-second tool limit during `tasks::apply::tests`; the final two never-started ownership-lock cases were subsequently run once and passed. The unfinished apply group was not retried. Logs `/tmp/pui-git-summary-{full,remaining,lock,orphan}.log`. No passing complete-suite result is claimed, and the whole suite was not repeated after parser repairs. No deadlines/assertions/production scheduling were weakened; no baseline comparison establishes root cause for the other failures.

Initial development commands timed out during dependency/CEF compilation and shared-cache contention. Final validation used a dedicated copy-on-write `/tmp/pui-git-summary-target` cache and the existing offline Rust/CEF/toolchain with explicit macOS SDK. No installations, provider requests, package build, native screenshot acceptance, push or deployment. Feature implementation used an isolated worktree to preserve concurrent edits.

## Integration into the working checkout

The feature patch applied cleanly onto local `master` after the independent BTW dock commit, preserving concurrent transcript message-rendering changes. An integration-focused compile is currently blocked by an unrelated concurrent connector test: `src/app/resource_reload/connector/tests.rs:329` calls `receipt(&app)` after a local `Value` shadows that function (`E0618`). No connector files were changed by this task. The passing isolated-worktree results above remain the feature evidence; no passing integrated-checkout test result is claimed. Log `/tmp/pui-git-summary-integrated-focused.log`.
