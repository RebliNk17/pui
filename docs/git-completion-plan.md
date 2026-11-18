# Native Git completion checklist

User requested all Git workflows, including richer review and porcelain lifecycle. This is a product checklist, not a claim that every Git command can safely be exposed. Keep unsupported states explicit; no silent whole-file, unsigned, destructive, or conflict-resolution fallback. Local feature commits authorized; no new push/install authorization inferred. Development cron remains paused.

## Implemented with bounded automated evidence

- Repository init, identity creation/load/edit/inherited override, basic clone into a new child folder.
- Whole-file/hunk staging, bounded +/- line selection; empty/invalid active intent refuses. EOF-marker/mode/rename/conflict cases still restricted.
- Staged/initial commits, reviewed identity, optional signer/pre-commit/prepare-msg/commit-msg/post-commit, cancellation, diagnostics, failed-message retention.
- Create remote/save upstream/inspect remote, exact-ref Push, optional pre-push/signed-push request, timeouts and sensitive diagnostics.
- Branch create at HEAD/fullOID, clean existing-branch switch, History/list draft handoffs.
- History details/parents/files/text patches/merge-parent comparison, passive signature-presence indicator, bounded explicit before/after images.
- Last-discard restore with exact frozen review and backup of overwritten current content. Not a persistent recovery manager.
- **Explicit exact-branch Fetch:** review fetch URL/tracking-oldOID, observe remote head, download pinned object without refs/FETCH_HEAD, require tracking fast-forward, expected-oldOID publication; no checkout/merge/prune/tags/hooks. Timeout/cancel may retain objects or completed ref publication. Hosted remote may reject fetching observed OID if it moves; fail, never silently retarget.

`git-safety-corrections.md` supersedes earlier inaccurate signature/selection/image/timeout assertions. Full green suites do not replace direct semantic tests or physical acceptance.

## Still to implement/validate

- [x] Fast-forward-only local update from a fetched branch (no network hidden inside merge). Review freezes current branch/HEAD/tracking/target/changed paths, reuses clean/ignored/untracked/active-operation/filter/source-tree gates, rejects target submodules/custom mergeOptions, then pinned `merge --ff-only --no-autostash --no-verify-signatures` with hooks disabled. Three-second local Git budgets and external writer/config races remain; cancellation has no transactional rollback promise. Focused disposable test verifies checkout update and dirty/stale refusal, theme/zoom Escape starts no worker. `/tmp/pui-ff-full.log`:1955binary+10integration passed,2ignored/zero failures; JS98/1skipped,Python41,strictClippy/fmt/diff pass. No physical/manual/provider acceptance.
- [ ] Explicit merge/rebase, conflict editor/mark-resolved, abort/continue/skip lifecycle, dirty-state protection. **Conflict inspection substep done:** UU/AA/DD/AU/UD/UA/DU rows offer explicit Base/Ours/Theirs frozen index-stage text (64KiB/side,128KiBcombined,2048lines/side), missing sides honest, literal path and ordinary blob only, no filter/verifier/workfile IO or resolution/staging. Rebase-side semantics disclosed. Real merge fixture proves side bytes/index/HEAD/workfile unchanged; strict framing/type/duplicate tests; light/dark420pt0.8/1.4zoom Escape no worker. `/tmp/pui-conflict-full.log`:1958binary+10integration passed,2ignored/zero failures; JS98/1skipped,Python41,strictClippy/fmt/diff pass. Read-only inspection is not conflict resolution.
- [ ] Stash create/list/show/apply/pop/drop with collision/conflict and recovery behavior.
- [ ] Branch rename/delete (merged/unmerged), tags, worktree list/create/remove, remote URL editing/removal/upstream replacement, detached/unborn branch UX.
- [ ] Amend/revert/cherry-pick/reset/reflog recovery and force-with-lease only under explicit destructive confirmations and expected OID guards.
- [ ] General arbitrary-base/merge-base comparison (current comparison supports merge parents only), richer graph/log pagination, blame/file history.
- [ ] Explicit signature verification/trust, actual successful signed-push certificate tests (not only unsupported-server refusal), complete reference/checkout/commit hook lifecycle choices.
- [ ] Recovery-copy discovery/path mapping, inspect/export/restore/delete/retention/quota after restart; no auto-delete.
- [ ] Clone arbitrary parent picker/open project, safe filter/LFS/submodule handling; support empty/deleted/renamed/binary/executable/symlink/EOF staging cases without guessing.
- [ ] Image additions/deletions/merge-parent and multiwindow budget/error/cancel/permission matrices; large previews explain limits without partial output.
- [ ] Native pane layout/navigation review: accumulated transient forms must not crowd out usable Changes/chat.
- [ ] Security/permissions/error-output/cancellation/crash/concurrent-writer review across all operations. Cooperative Git locks/CAS are not hostile-config/filesystem sandboxes or process-tree deadlines.
- [ ] Live HTTPS/SSH helpers/provider auth, pinentry/hardware keys, hosted remote protection, physical light/dark/narrow/keyboard acceptance. Requires available devices/accounts and explicit runtime authorizations; do not fabricate evidence.

## Latest validation

Fetch slice: `/tmp/pui-fetch-full.log`: **1953 binary+10integration passed**,2ignored/zero failures; JS98passed/1skipped,Python41,strictClippy/fmt/diff passed. Focused tests cover absent tracking creation, normal advance, rewind refusal, unchanged dirty checkout/index/HEAD/noFETCH_HEAD, stale endpoint/pre-cancel and light/dark420pt0.8/1.4zoom Escape no worker. `/tmp/pui-fetch-{focused,clippy,js,python}.log`. No actual user-repository fetch/push, application profiling or hosted-provider acceptance.

Source-only additions since installed build1795015849 remain local. No all-Git completion claim.
