# Reviewed text conflict resolution

Built in isolated `git-reviewed-conflict-resolution` worktree after `git pull --ff-only origin master` confirmed base `c55c172ad6a111755859b06a3798769953316cf9` current. Original checkout's concurrent untracked task files are untouched. Local feature commit authorized; this task does not authorize another push/install.

## Behavior

Unmerged file rows retain the Base/Ours/Theirs inspector and add two explicit actions:

1. **Edit working resolution…** loads a frozen current working file into a native text form. **Save working file** preserves previous bytes in a create-only 0600 SHA-named recovery file beside the actual Git index, then replaces only the working file. It does not stage. Cancel discards the draft.
2. **Review mark resolved…** performs a new inspection and shows the exact current text, mode, byte count and final-newline state. **Mark resolved** replaces only that path's unmerged index stages with an ordinary stage-0 blob through a private index and an owned cooperative `index.lock`. It does not change working bytes, commit or continue/abort a Git operation.

Both actions freeze/revalidate root/path, entire bounded index and actual index location, HEAD/branch (including detached), ordinary file mode/full Unix permissions, working bytes and bounded operation control metadata. Save and stage each require a fresh review; saving cannot reuse an older staging authorization. Shared Git permit held through direct-child cleanup; worker result delivery occurs after release. No network, automatic reattempt, conversion/filter or implicit whole-repository staging.

## Deliberate restrictions and limits

- Existing both-present ordinary text conflicts with unchanged mode only. Missing-side deletion, special modes/type changes, split/sparse indexes and configured conversions/attributes refuse. Rebase/merge continuation and deletion resolution remain unfinished.
- Working text max64KiB/2048lines; index16MiB; operation files16KiB each over a fixed list; existing bounded conflict sides/read supervision retained. UTF-8 only; unsupported controls and conservative marker-like lines prevent save/stage. Empty text means an empty file, not deletion. Marker checks do not verify semantic correctness.
- Existing canonical roots, literal paths and non-symlink parents/targets required. Trusted Git config and concurrent external filesystem/config writers are not sandboxed; locks and revalidation cannot eliminate hostile races. Short local Git budgets are not process-tree/kernel deadlines.
- Failure/cancellation may leave a recovery copy, unreachable blob, partially created private temporary file on IO error, or already-completed working-file/index publication. No rollback or automatic retry is promised. Recovery discovery/retention/restore UX remains separate work.
- Shared Quiet Mac confirmations include safe initial focus, disabled decisions, Escape/backdrop cancellation, bounded detail scrolling, exact owner/permission/modal checks and keyboard-event isolation. Native minimum860×620;420pt synthetic stress is not physical macOS sizing acceptance. AccessKit remains disabled.

## Focused evidence

- Real disposable merge conflicts: save backup matches old text; original index unchanged until separate stage; published stage-0 equals exact reviewed bytes; other unresolved paths, HEAD, MERGE_HEAD and working bytes retained.
- Linked-worktree real index isolation; empty/no-final-newline content; Unicode/leading-dash/pathspec-looking filenames; stale file/index/HEAD/operation/permissions, foreign lock and pre-cancel refusal. Attributes/filters/binary/bounds/symlink refusals.
- Actual egui keyboard row activation and pointer confirmation perform save then separate fresh stage in a disposable repository. No implicit continue. Light/dark860pt0.8/1.4zoom prompts and420pt stress retain action visibility. Typing does not leak; editor-focused Escape, safe initial Enter, key-release suppression, disabled marker confirmation, hidden/owner/permission/foreign-modal cancellation covered.
- Initial strict Clippy found a complex operation return type; factored a type alias rather than suppressing the lint.

## Completion validation

On finished source based on `c55c172a`:

- `scripts/test-rust.sh`: **1,991 binary + 10 integration tests passed**, 2 ignored, zero failures; every serial group once, no retries or omitted groups. `/tmp/pui-conflict-resolution-full.log`.
- Strict all-target Clippy, `cargo fmt --check`, `git diff --check`: passed. `/tmp/pui-conflict-resolution-clippy.log`.
- JS: **102 passed, 1 skipped**. `/tmp/pui-conflict-resolution-js.log`.
- Python: **41 passed**. `/tmp/pui-conflict-resolution-python.log`. Release-publication messages are mocked, not deployments.
- Detached real rebase conflict regression confirms exact staging retains HEAD/REBASE_HEAD/rebase directory and other conflicts without advancing the operation.

Environment: resolved CommandLineTools Git, stable Rust, Homebrew Python, existing pinned CEF distribution, `/tmp/pui-job-delete-validation` target, isolated global/system Git config. No production deadline changes, assertions weakened, scheduling changes or application profiling. No physical-pointer, hosted-remote/provider, hardware-key or full all-Git completion claim.
