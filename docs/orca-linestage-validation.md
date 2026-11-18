# Reviewed per-line stage/unstage

> **Historical report; conflicting safety/completion claims are superseded by `git-safety-corrections.md`.** See that report for actual corrected behavior, evidence and remaining work.

## Behavior and limits

Index confirmation adds **Select individual lines**: instead of whole hunks, pick specific target-side changed lines (added for Stage, removed for Unstage). Context and the opposing side are always preserved so each hunk remains a valid patch. Only hunks that select at least one target line are emitted; unselected hunks are skipped. A synthesized selection must apply cleanly to the private index (`git apply --cached --check --recount`); otherwise the whole action is refused with no index change. This dry-run gate makes line-stage fail closed rather than risk index corruption.

Line ordinals are per-hunk stable ordinals over target-side changes only. Selection toggling resets hunk selection and vice versa. Inactive/empty selections are ignored (whole file). Unsupported statuses, mode/rename/binary diffs, and discard still refuse line-stage. Existing whole-file and whole-hunk stage/unstage remain unchanged. Same private-index transaction, lock, cancellation and owner/session guards as other index actions.

## Evidence

- Backend tests: staging only one selected addition updates the index for that line and leaves the worktree/HEAD unchanged; invalid ordinal and inactive selection refuse without index change; real apply verified via `git show :file`.
- Native UI test: line-selection confirmation renders in light/dark at 0.8/1.4 zoom, Escape cancels with unchanged index.
- 27 `index_action::` tests and 7 `index_actions::tests` passed. Strict Clippy, rustfmt and diff checks pass.
- Logs `/tmp/pui-linestage-{focused,index,ui,clippy}.log`.

Committed locally as `8a180f8` (also includes a formatting-only fix to the prior clone feature). No push/install.
