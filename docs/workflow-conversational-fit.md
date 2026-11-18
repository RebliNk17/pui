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

1. Rich plan revisions and resumable plan-review history. The initial explicit Plan → Manual handoff is implemented here; editing an approved plan still requires a fresh review.
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
