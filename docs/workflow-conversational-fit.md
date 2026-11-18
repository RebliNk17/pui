# Workflow fit: conversational development versus Claude Code dynamic workflows

## Scope and source

User selected all six desired behaviors: natural chat → plan → implement → verify; optional plan approval; coordinated agents with final review; automatic testing/bounded fixes; truthful progress/results; safe continuation.

Compared the actual Pui tool/permissions/supervisor/UI path with the current public [Claude Code dynamic-workflow documentation](https://code.claude.com/docs/en/workflows). The fetched Markdown distinguishes **a script that owns orchestration and intermediate results** from ordinary conversational work and lead-managed agent teams. It documents background runs, planned phases, script inspection, reusable commands, and same-session resumption. These are reference behaviors, not instructions to execute external scripts or copy Claude's permission policy.

Base for this change: fetched `origin/master` at `f7e9f8554`, isolated branch/worktree `validate-conversational-workflows`. The original checkout had a concurrent reset/cherry-pick and then diverged from a force-updated remote; it was not reset, merged or overwritten by this change. Current master includes local user workflow checkpoints and the newer session-owned navigation behavior, both preserved.

## Findings against the six expectations

| Expectation | Actual Pui behavior | Assessment |
|---|---|---|
| Natural chat → plan → implement → verify | Parent chat can inspect and coordinate; `pui_workflow` hands a bounded task to a fixed host pipeline and returns a result. Before this change it accepted only an objective/reason, not an implementation plan. | Partial; concrete reviewed-plan handoff improved here. The parent waits on the sequential workflow tool; this is not a background script runtime. |
| Plan review before implementation | Plan permission mode is read-only and blocks workflow execution. Previously even a plan already discussed in chat was replaced by a separate acceptance-proposal call after initial consent. | Improved: submit concrete steps and exact criteria/commands, review them inline, then implement that exact contract. Continued here: a concrete plan from Plan mode offers an explicit, conversation-scoped transition to Manual. It never silently enables edits, Auto or Bypass. |
| Coordinated agents and final review | Managed clickable subagents and codemode support lead-driven orchestration; the structured-task pipeline itself is fixed and disables builder delegation. | Not Claude dynamic-workflow equivalence. No reusable/saved workflow scripts, generic phase graph, script checkpoint/resume or integrated lead-managed fan-out within `pui_workflow`. |
| Automatic checks and bounded fixes | Separate host check execution, fresh verifier, at most two builder repairs; environment/uncertain-effect blockers do not blindly retry. | Implemented bounded path; provider-free tests do not prove real model quality or practical repair efficiency. |
| Clear progress, blockers and completion | Work-item tabs and evidence retain stages; terminal results previously returned `approved:true` without a machine-readable verified outcome. | Improved: explicit `verified`/`outcome`, current recovery-aware evidence, and application status returned to chat. Approval/tool completion is not success. |
| Safe continuation | Explicit same-candidate check reruns and never-started initial implementation continuation exist; uncertain commands/builders are not replayed. User workflow checkpoints are retained notes, not execution snapshots. | Safe supported subset, not arbitrary workflow-script pause/resume or daemon lifetime. |

## Implemented improvement

`pui_workflow` now accepts optional:

```json
{
  "plan": {
    "steps": ["Inspect the existing behavior", "Implement the fix and regression"],
    "checks": {
      "criteria": [
        {"id": "C1", "description": "Observable requested behavior", "command": "existing test command"}
      ]
    }
  }
}
```

- Lead-authored steps and exact checks are validated in both the tool and native host and displayed **before initial consent**.
- Invalid/oversized/inspection-only plans create no workflow and launch nothing. Extra fields such as `approved:true` are not authority.
- Positive native consent stores the plan on the workflow. The work-item overview and builder prompt retain the reviewed steps.
- Source inspection and normal safe preparation still occur. The exact reviewed checks become the frozen contract; no redundant proposal model replaces them. Altered checks fail before implementation.
- Existing no-plan calls remain compatible and explicitly explain that acceptance checks will be established first. Don't ask/read-only/mirror/ownership/model-policy/tool-permission boundaries remain enforced. Plan mode now permits only opening the concrete native plan review; the exact affirmative host choice is required to enter Manual.
- Terminal chat results explicitly distinguish `verified_candidate` from `not_verified`; latest recovery evidence supersedes historical passing evidence. Isolated candidate verification does not claim application, commit, merge or deployment.

This is a **conversational handoff improvement**, not an implementation of Claude's dynamic-workflow runtime.

## Verification

No provider request, dependency installation, live task-record edit, app replacement or push was performed.

- Composed provider-free regression follows actual native consent → source inspection → preparation → builder transport → checks → fresh verifier → fingerprint comparison → result returned to the creating chat. It confirms no redundant proposal run and explicit verified handoff. Model/RPC events are stubbed, not paid/model-quality evidence.
- Final focused app-task suite: **158 passed**, including readable inline plan/commands in light/dark/narrow/zoom, cancellation, Plan-mode rejection, legacy deserialization, changed-contract refusal and the composed chat-to-result regression.
- Domain plan/outcome tests cover missing evidence, terminal-but-unverified state, stale/failed recovery superseding old success, and bounded check output.
- Formatting, `git diff --check` and warnings-denied all-target Clippy pass.
- JavaScript: **100 passed, 0 failed, 1 pre-existing skipped**, including plan transport/native-review validation tests.
- Full Rust scheduling: invoked `scripts/test-rust.sh`; bulk stopped on three existing Git-hunk fixture deadline failures. Ran each previously unexecuted serial group once; two more existing Git/commit inspection deadline failures occurred. **1,925 passed, 5 failed, 2 ignored** across all native groups, with **3 prompt guards passed** separately. No failed test was retried, deadline extended, assertion weakened or group omitted. This is not a green full-suite claim.
- Failures: `workspace::index_action::hunks::tests::{invalid_hunk_and_changed_content_refuse_without_index_change,explicit_hunk_only_updates_selected_index_change_worktree_unchanged,multihunk_stage_applies_together_and_empty_masks_fail_closed}`, `app::changes::commit::tests::native_commit_dialog_escape_themes_zoom_preserves_message_and_never_executes`, `workspace::commit::tests::initial_commit_is_parentless_and_preserves_unstaged_content_and_index`. Errors were existing 3-second Git inspection/fixture deadlines; root-cause attribution to machine load is not established.
- Logs: `/tmp/pui-conversational-{full-rust,remaining-serial,final-serial,prompts,full-js,clippy-final,e2e}.log`.

New managed review could not start because Pui hit its 128 retained-session limit; old reviewer chats were stopped and could not be resumed by the parent. No invisible agent fallback was used. Independent review is therefore **not claimed** for this pass.

## Remaining product work before claiming equivalence

1. Resumable plan-review history. Initial Plan → Manual handoff and inline revision feedback are implemented; every revised plan still needs a fresh review. Pre-approval revision feedback is returned to the creating chat, not a durable executable checkpoint.
2. A deliberate decision whether to build a reusable script-backed orchestration/runtime layer (using Pi's supported codemode, not another unsafe evaluator), with durable phase/agent identity, resumable boundaries and repeatable saved workflows.
3. Background parent-chat responsiveness while structured work is running; do not let a sequential tool wait masquerade as a fully independent background workflow.
4. Real bounded provider-backed usability pilot after separate approval: plan revision, implementation, actual check failure/repair, fresh review, blocked dependency, stop/restart, and reviewed application. Measure user decisions, cost, useful output and misleading completion states, not only test counts.

## Stable feature validation and explicit Plan-mode handoff

User requested pushing only after a completely green suite. Continued on latest `origin/master` at `e219d64e` before integration of subsequent unrelated changes.

- Added the native Plan-mode handoff: the tool can submit a concrete plan for review while read-only. Only **Approve plan · switch to Manual** changes the owning conversation's permission. Merely opening review, cancellation, legacy choice strings, changed owner/model/permission and read-only-worker calls do not upgrade permissions or start implementation. Manual exact-tool approvals remain required.
- Three incoming auto-update Clippy errors were cleared without behavioral expansion: a staged-install type alias, collapsed equivalent asset predicate, and test-only preview wrapper. No update/download/install action ran.
- Investigated flaky Git fixtures instead of changing production deadlines. `workspace::tests::fixture` previously cloned the entire changing Pui repository (824 tracked files / 174 commits at inspection) for one-file status/index assertions. It now uses a once-created tiny disposable repository with real merge ancestry, an 80-line README for hunk assertions, and private clone indexes. Real Git/index/branch/filter/conflict assertions remain unchanged; no production Git code or timeout changed.
- SVG lifecycle fixture now records the owned direct child's PID in a minimal shell before `exec` into Python. The PID survives exec; the same 300 ms deadline and direct-child reaping assertions still apply even when Python initialization is slow. This fixes missing PID evidence, not a shortened or omitted lifecycle assertion.
- Final complete `scripts/test-rust.sh` with `PUI_TEST_THREADS=1`: **1,935 binary tests + 10 integration tests = 1,945 passed**, **0 failed**, 2 pre-existing ignored. All bulk/serial groups ran once; no failure retry or omitted group inside this passing run. `/tmp/pui-conversational-deterministic-full.log`.
- Full JS: **101 passed**, 0 failed, 1 pre-existing skipped. `/tmp/pui-conversational-publish-js.log`.
- Formatting, diff whitespace checks and warnings-denied all-target Clippy passed. `/tmp/pui-conversational-publish-clippy.log`.
- Earlier unsuccessful full runs remain recorded above/in logs; the green run follows actual fixture/lint changes, not retries masked as stability. This is one complete passing run, not a measured zero-flake-rate guarantee.

Remote master advanced again during final validation (scheduled-job deletion and removal of a restored legacy Workflows header button). Those changes will be integrated normally and validated before any push; no force-push or original-checkout reset is authorized or used.

## Latest-master integration accepted for push

Merged `origin/master` at `dcae7a06` normally (includes scheduled-job deletion and removal of the legacy Workflows header button), preserving those changes. The first combined run still hit a Git-hunk inspection deadline. A bounded launch diagnostic measured the macOS `/usr/bin/git` developer-tool shim at roughly 41 ms per trivial invocation versus about 19–21 ms for the developer Git executable; these multi-command three-second snapshots amplify that avoidable cost. The test runner now resolves `xcrun --find git` only when the caller selected `/usr/bin/git`, preserving other Git installations. This changes test-process PATH only, not production executable selection or any deadline/assertion.

Accepted combined validation after that runner fix:

- Complete `scripts/test-rust.sh`, supported `PUI_TEST_THREADS=1`: **1,941 binary + 10 integration = 1,951 passed**, **0 failed**, **2 pre-existing ignored**. Every bulk/serial group executed once without retries/omissions in this run. `/tmp/pui-conversational-direct-git-full.log`.
- Full JS: **101 passed**, 0 failed, 1 pre-existing skipped. `/tmp/pui-conversational-integrated-js.log`.
- Whole-checkout formatting, whitespace checks and strict all-target Clippy passed. `/tmp/pui-conversational-integrated-clippy.log`.
- `bash -n scripts/test-rust.sh` passed. No OS agents, security software or other applications were stopped or reconfigured.

The earlier failures remain part of the record; no claim of proven zero flake rate or paid provider/native installed-app validation is made. User explicitly authorized a normal push to master only after a green complete suite; that condition is now met for this snapshot. No force-push, installation or provider-backed workflow execution is included.

## Inline plan-change requests

The green handoff snapshot was normally pushed as `ebafae22`. Continued with a bounded review loop: concrete-plan approval now offers **Request changes**. An inline editor returns bounded feedback and the original plan to the existing waiting tool call with `approved:false` and `revision_requested:true`. It does not create a workflow, run a provider/check, change permissions, or silently resubmit a plan. The lead is instructed to revise only the plan and obtain a fresh native approval. A distinct feedback-dialog ID prevents stale original approval responses from being interpreted as feedback; aborting the original tool closes the replacement dialog. Cancellation, empty/NUL/oversized feedback, changed model/permission/owner and read-only contexts fail closed. Objective-only requests retain their prior choices. Review uses the existing shared inline prompt surface, including light/dark, narrow and zoom regression coverage.

Validation of this slice before latest-master integration:

- Focused native consent: **24 passed**; the corrected projection lifecycle fixture also passed.
- Complete `scripts/test-rust.sh`: **1,955 Rust/integration passed**, 0 failed, 2 pre-existing ignored; all 22 groups ran once with no omitted groups or failure retries in this run. `/tmp/pui-plan-revision-green-candidate.log`.
- Full JS: **102 passed**, 0 failed, 1 pre-existing skipped. `/tmp/pui-plan-revision-final-js-serial.log`.
- Strict all-target Clippy, formatting, diff checks and runner shell syntax passed. `/tmp/pui-plan-revision-final-clippy.log`.
- Earlier attempts are not hidden: one full invocation failed to link (`library 'System' not found`); selecting developer-tool PATH also bypasses Apple's compiler SDK shim, so the runner now preserves caller SDKROOT or selects the active SDK. A subsequent full run stopped at the inherited projection lease fixture; `Weak::upgrade()==None` can precede completion of `Lease::drop`, so that fixture now waits for the worker's existing Release/Acquire permit after input drop, preserving the five-second deadline, 64 MiB assertion and production code. Focused development also exposed/fixed a reply-filter assumption and test-only private-method calls. A development rebuild and an overlapping JS/build-lock attempt timed out; these were not passes or accepted suite runs.
- New managed independent review was again refused by the 128 retained-session limit. No hidden reviewer was launched and no independent-review claim is made. No paid-provider or installed-app validation occurred.

Remote master advanced to `613610bd` (Normal-view transcript action summaries) during validation. Committed the validated revision slice as `b2c2f565` and merged that master normally as `5484cdaa`, preserving incoming behavior. Final combined validation:

- Complete runner: **1,957 Rust/integration passed**, 0 failed, 2 pre-existing ignored. Every one of the 22 groups ran once, without retries or omissions. `/tmp/pui-plan-revision-integrated-full.log`.
- Full JS: **102 passed**, 0 failed, 1 pre-existing skipped. `/tmp/pui-plan-revision-integrated-js.log`.
- Strict all-target Clippy, formatting, whitespace and shell syntax passed. `/tmp/pui-plan-revision-integrated-clippy.log`.
- Remote master remained `613610bd` at the final pre-push check; the normal push is therefore a fast-forward. Original checkout/worktrees, running app, permissions and retained sessions were not reset or replaced.

The user’s green-suite condition is met for this combined snapshot. This remains provider-free validation without independent review, installed-app exercise or proven zero-flake-rate claims.
