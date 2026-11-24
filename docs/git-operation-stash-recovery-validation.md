# Git operation abort, stash & recovery manager — implementation and validation

Implemented in isolated worktree `git-workflow-next` based on `origin/master` (`1aac108c`).

## Delivered slices

1. **Coalesced Git metadata subprocess checks (3s deadline budget fix without weakening deadlines)**
   - `src/workspace/branches/create.rs`: coalesced five sequential `rev-parse --git-path` operation-marker spawns into one `rev-parse` invocation during branch-switch inspection, keeping the rest of the 3-second budget available for checkout tree and file-diff inspection (`target_submodule_refuses_without_checkout`).
   - `src/workspace/index_action.rs` and `src/workspace/index_action/conflict_resolution.rs`: coalesced `--show-toplevel`, `--git-path index`, and `--shared-index-path` into single `rev-parse` invocations.

2. **Reviewed Git Operation Abort (`src/workspace/operation.rs`, `src/app/changes/operation.rs`)**
   - Detects active `merge`, `rebase` (`rebase-merge` / `rebase-apply`), `cherry-pick`, or `revert` states and refuses if multiple operation families exist simultaneously or if operation directories are symlinked.
   - Freezes canonical repository root, operation `Kind`, current `branch` / `head`, informational target/onto ref, unmerged paths (`<= 256`), `index` bytes (`<= 16 MiB`), operation control files, porcelain status, and SHA-256 digests of every unmerged working file.
   - Re-verifies the exact frozen review before running hookless `git -c core.hooksPath=/dev/null -c submodule.recurse=false <op> --abort` under `GIT_NO_REPLACE_OBJECTS=1`, then confirms operation markers are cleared.
   - Exposed in Changes via both the top operation status bar when unmerged files are present and the Repository settings dropdown (`"Abort merge / rebase / cherry-pick…"`), using `confirmations::Dialog` (`destructive(true)`).

3. **Reviewed Git Stash (`src/workspace/stash.rs`, `src/app/changes/stash.rs`)**
   - `list`: reads up to 64 stash entries from `refs/stash` reflog with contiguous `stash@{N}` verification and sanitized summaries.
   - `inspect_push` / `apply_push`: validates optional message (`<= 256` bytes, no control characters), requires no active operation, no `index.lock`, no split index, no submodules, and checkout filters disabled; freezes tracked modified/added/deleted paths and their working-file SHA-256 digests; runs `git stash push --quiet --no-keep-index` so **untracked files are never stashed or swept**.
   - `inspect_entry` / `apply_entry`: separates `Apply` and `Drop` (never `pop`). Both pin the entry's immutable commit OID (`expected_oid`), reflog selector, and `top_oid` of `refs/stash` so a concurrent reflog shift refuses rather than applying or dropping the wrong stash. `Apply` requires a completely clean working tree and index and applies by immutable commit OID; `Drop` removes only the verified reflog slot.
   - Exposed in Changes via `RepoSheet::Stash` (`"Stashes…"`) and `confirmations::Dialog`.

4. **Persistent Recovery Copy Manager (`src/workspace/recovery_manager.rs`, `src/app/changes/recovery_manager.rs`)**
   - Scans the repository's Git index directory for regular `pui-discard-recovery-<64hex>` and `pui-conflict-recovery-<64hex>` files (bounded to 128 files / 8 MiB per file / 64 MiB total scan) and cryptographically matches them against tracked and recent historical paths.
   - `inspect_restore` / `apply_restore`: verifies the target path and saved bytes against the filename's SHA-256 digest before and after acquiring `index.lock`, backs up any existing working file into the index directory (`0o600`, `create_new`), atomically writes and renames the restored file, and retains the recovery copy.
   - `inspect_delete` / `apply_delete`: verifies exact SHA-256 and size before removing a single recovery file.
   - Exposed in Changes via `RepoSheet::Recovery` (`"Recovery copies…"`) and `confirmations::Dialog`.

## Validation evidence

Compiled in dedicated target `/tmp/pui-browser-completion-owned.hav6QQ` (`debug/deps/pui-346f211a160bfd41`), verified by `CARGO_MANIFEST_DIR=/Users/shai.cohen/personal/pui/.worktrees/git-workflow-next` in `pui-346f211a160bfd41.d` and `--list`.

- **Complete Rust suite (`./scripts/test-rust.sh`)**: **2,312 binary tests + 10 integration tests = 2,322 passed, 0 failed, 2 ignored** in 399s (`/tmp/pui-git-next-full-rust.log`). All 11 prior Git failures are resolved with zero assertion or deadline weakening.
- **Focused Git workspace & Changes suites**: `workspace::` **163 passed, 0 failed** (`/tmp/pui-git-next-workspace.log`); `app::changes::` **288 passed, 0 failed** (`/tmp/pui-git-next-changes.log`).
- **Strict Clippy & formatting**: `cargo clippy --offline --locked --all-targets -- -D warnings` passed (`/tmp/pui-git-next-clippy.log`); `cargo fmt --all -- --check` and `git diff --check` passed.
- **JavaScript suite**: **173 passed, 0 failed, 1 skipped** (`/tmp/pui-git-next-full-js.log`).
- **Python suite**: **44 passed, 0 failed** (`/tmp/pui-git-next-full-python.log`; release output is mocked).

## Merge with upstream `origin/master` (`abcd6290`)

Merged upstream `origin/master` (`abcd6290`, which includes `9b68946d` MCP connector reconnect and `abcd6290` task apply-back/Git path-batching updates). Resolved the two overlapping path-batching hunks in `src/workspace/index_action.rs` and `src/workspace/index_action/conflict_resolution.rs` in favor of upstream's identical `rev-parse --path-format=absolute --show-toplevel --git-path index ...` batching while retaining our `src/workspace/branches/create.rs` operation-path batching and the three new Git modules (`operation`, `stash`, `recovery_manager`).

Post-merge verification on the combined tree (`/tmp/pui-git-merge-slices.log` & `/tmp/pui-git-merge-clippy.log`):
- `workspace::operation::` (2 passed), `workspace::stash::` (2 passed), `workspace::recovery_manager::` (1 passed), `workspace::index_action::` (32 passed), `workspace::branches::create::` (7 passed)
- `app::changes::operation::` (1 passed), `app::changes::stash::` (1 passed), `app::changes::recovery_manager::` (1 passed), `tests/prompt_design.rs` (3 passed)
- `cargo clippy --offline --locked --all-targets -- -D warnings` passed.

## Honest limitations


- Operation support in this slice covers reviewed **abort** of in-progress `merge`, `rebase`, `cherry-pick`, and `revert`; initiating divergent merges/rebases or running `--continue` / `--skip` (which can invoke commit hooks/editors or replay arbitrary commits) remains separate future work.
- Stash push stashes tracked modifications only (`--no-include-untracked`), and stash apply requires a clean working tree and index.
- Recovery path auto-matching checks current tracked/untracked files and the last 32 commits on `HEAD`; older unreferenced paths require entering the relative path to verify against the cryptographic filename digest.
