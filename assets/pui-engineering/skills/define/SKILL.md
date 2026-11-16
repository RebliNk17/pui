---
name: define
description: Turn agreed requirements into a small implementation specification with acceptance checks. Use when the user asks to define a feature or write a spec.
disable-model-invocation: true
metadata:
  author: Pui
  collection: pui-engineering
  version: "1.0.0"
---

# Define

Create an actionable specification grounded in the current conversation and repository. Do not invent stakeholder decisions to make the document look complete.

## Gather evidence

- Read project instructions, the affected interfaces, existing tests, and the established documentation layout.
- Separate agreed requirements, observations from code, and unresolved assumptions.
- Use pui_ask_questions only for gaps that materially affect scope or safety. Mark an unanswered gap as open instead of silently settling it.

## Write the specification

Keep these sections proportional to the change:

1. Outcome: who benefits and what observable behavior changes.
2. Scope: included behavior and explicit non-goals.
3. Current behavior: relevant entry points and constraints, with file references.
4. Proposed behavior: inputs, outputs, state transitions, errors, cancellation, and permission boundaries.
5. Acceptance checks: concrete examples covering the normal path and meaningful edge cases.
6. Implementation slices: small independently verifiable changes, dependencies, and the check for each slice.
7. Open decisions and risks: include compatibility, persistence, and rollout only where relevant.

Prefer existing modules and native Pui mechanisms. Avoid speculative abstraction or a task graph when a short checklist will do. A spec does not need an issue tracker.

## Deliver

Show the specification in chat unless the user requested a file. For an agreed file deliverable, follow the repository's documentation conventions and open the main document with pui_open. Distinguish a proposed test from one actually run.

Do not implement the feature, publish tickets, commit, or start a workflow as a side effect of writing a spec. A document is not execution approval. Offer a next step without starting it.
