# Discard recovery-copy restore

> **Historical report; conflicting safety/completion claims are superseded by `git-safety-corrections.md`.** See that report for actual corrected behavior, evidence and remaining work.

## Behavior and limits

Discard already retained a path-bound recovery copy. Now the Changes panel offers **Restore discarded content** for the last completed discard while the copy exists. It confirms the source path and recovery location, then copies the recovery file back to the working file, overwriting its current content. Staged changes, HEAD and the recovery copy are unchanged; no commit or push. It is transient, session-local intent (not persisted/restored as executable state) and requires a tracked unstaged status at discard time.

Restore reads the recovery copy under the bounded regular-file/no-symlink/8 MiB guard, requires the target to be a bounded regular file, takes a cooperative `index.lock`, writes a private temp, preserves the recovery file mode, and renames into place. Cancellation/hidden view/owner change reset the affordance and cancel the worker; late results are discarded. No index/HEAD mutation. Recovery-copy retention/cleanup policy (no quota UI) is unchanged; there is no auto-deletion.

## Evidence

- Backend tests: restore returns the original working content and keeps the copy; missing copy and non-discard states refuse without changing the file.
- Native UI test: restore confirmation renders light/dark at 0.8/1.4 zoom, Escape cancels, no write/worker. 8 index_actions UI tests passed.
- Full suite: **1,947 Rust binary + integration passed**, 2 ignored, zero failures. JS 98 passed/1 skipped, Python 41 passed, strict Clippy, rustfmt, diff checks.
- Logs `/tmp/pui-recovery-{focused,ui,clippy,full,js,python}.log`.

Committed locally; no push/install. Same full-suite environment notes as prior Git features apply.
