---
name: clarify
description: Resolve ambiguous requirements before planning or changing code. Use when the user wants to explore an idea, compare options, or agree on scope.
disable-model-invocation: true
metadata:
  author: Pui
  collection: pui-engineering
  version: "1.0.0"
---

# Clarify

Help the user make the few decisions needed for a useful next step. This is a discussion, not permission to implement.

## Establish the decision

- Restate the intended outcome in one sentence. Separate what the user said from your assumptions.
- Read relevant project instructions and a small amount of existing code or documentation when the question depends on the app. Do not ask the user facts you can safely inspect.
- Identify the highest-impact uncertainty: who needs the change, which behavior should differ, or what constraint rules out an approach.

## Ask and refine

- Use pui_ask_questions for decisions. Batch independent questions, at most three per round, with a recommended option first and a short explanation of tradeoffs. Leave room for another answer.
- Ask dependent questions only after their prerequisites are answered. Skip questions that will not change the next step.
- Treat skipped questions as unresolved, not agreement. If the user is unsure, offer a small experiment rather than choosing silently.
- If the answer needs visual feedback or a measurement, describe the smallest prototype or check and get agreement before building it. Do not turn uncertainty into a long interview.

## Finish

Summarize the outcome, agreed behavior, explicit exclusions, and any remaining decision. Give observable acceptance examples and recommend one next step. Stop when the scope is clear enough; the user does not need to approve a large plan merely to end the discussion.

Do not edit files, create tickets, or start workers during clarification unless separately requested. Do not claim that discussing an option authorizes it.
