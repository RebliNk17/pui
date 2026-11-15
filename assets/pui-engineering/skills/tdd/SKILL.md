---
name: tdd
description: Implement an agreed behavior with an observed failing test, a minimal fix, and safe refactoring. Use when the user requests test-driven development.
disable-model-invocation: true
metadata:
  author: Pui
  collection: pui-engineering
  version: "1.0.0"
---

# TDD

Use tests as feedback for one observable behavior at a time. Adapt to the project's existing test framework rather than introducing a new one by default.

## Establish the target

- Read project instructions, relevant implementation, nearby tests, and documented lint/test commands. Inspect current changes so you do not overwrite unrelated work.
- Identify the smallest useful behavior and the interface through which a caller observes it. Clarify material ambiguity with pui_ask_questions.
- Run a focused baseline when practical. Record existing failures separately from new ones.

## Repeat a short cycle

1. Add one test for the next behavior, using realistic inputs and a meaningful expected result. Prefer public behavior to private implementation details; fake external boundaries, not the code being tested.
2. Run that test before changing production code. Confirm it fails for the missing behavior, not a syntax error, unavailable dependency, or broken fixture. Report if a valid failing run cannot be obtained.
3. Make the smallest production change that satisfies the behavior. Run the test again and inspect the result.
4. With the test green, simplify duplicated or awkward code only within the touched area. Run the focused tests after refactoring.
5. Select the next acceptance example and repeat. Include relevant errors, cancellation, and permission-denied paths rather than just the happy path.

Do not write a large speculative test suite before learning from the first cycle. Do not weaken assertions, delete a regression test, or rewrite expected values merely to accept buggy behavior. If the requirement changes, explain why the test should change.

## Verify and report

Run the affected suite and existing lint/type checks when available, then broader tests as cost permits. State the commands, observed results, remaining gaps, and any environment blocker. Never label an unrun test as passing.

Stay within the user's implementation scope. Skill invocation is not permission to install dependencies, run destructive commands, commit, push, or deploy. If delegation is useful, use only Pui-managed workers with bounded assignments and review their changes; do not launch agents through a shell. Offer pui_workflow only when independent checks provide a concrete benefit, and wait for approval.
