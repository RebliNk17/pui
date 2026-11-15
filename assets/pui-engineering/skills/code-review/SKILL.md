---
name: code-review
description: Review a specified diff for actionable correctness, security, and regression risks. Use when the user requests a code review or pre-merge assessment.
disable-model-invocation: true
metadata:
  author: Pui
  collection: pui-engineering
  version: "1.0.0"
---

# Code Review

Review the change without rewriting it. Prioritize demonstrable defects over style preferences.

## Set the boundary

- Read project instructions and identify the requested diff, files, or base revision. Inspect the working tree without resetting, stashing, or modifying it.
- If the review target is ambiguous, ask with pui_ask_questions. Do not silently compare an unrelated branch or assume all local edits belong to this task.
- Read surrounding code and affected callers, not just added lines. Check the intended behavior against relevant tests and documented contracts.

## Look for failures

Trace changed inputs through outputs and state transitions. Focus on incorrect results, broken compatibility, stale state, error paths, races, unbounded work, and missing regression coverage. In Pui, examine project trust, permission scope, worker visibility, cancellation, persistence, and whether browsing can unexpectedly execute or install code. For prompt UI changes, check shared prompt primitives and narrow/light/dark/disabled states.

Validate likely findings with a focused non-mutating check or existing test when practical. Do not run project scripts with external side effects just to strengthen a review. Explain what was inspected and what was actually executed.

## Report actionable findings

For each supported defect, provide:

- Severity and a short title.
- A precise file and line range in the changed code where possible.
- The triggering condition and user-visible consequence.
- Evidence or a concrete reproduction path, plus a focused remedy.

Rank findings by impact. Keep speculative concerns separate as questions, and omit cosmetic feedback unless requested. Do not inflate a report to meet a finding quota. If no actionable defect is found, say so and still name the verification limits; that is not a guarantee of correctness.

End with test results and residual risks. Do not edit files, apply fixes, commit, publish review comments, or approve/merge a change unless separately requested. If independent review would materially help, use a bounded read-only Pui-managed worker, never a shell-launched agent.
