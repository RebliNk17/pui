---
name: debug
description: Investigate a reproducible failure, test competing causes, and verify a focused fix. Use when diagnosing bugs, regressions, or unexpected behavior.
disable-model-invocation: true
metadata:
  author: Pui
  collection: pui-engineering
  version: "1.0.0"
---

# Debug

Find evidence for the cause before changing the system. Keep observations, hypotheses, and conclusions visibly distinct.

## Bound the failure

- Read project instructions and inspect the affected code, recent relevant changes, and existing tests without disturbing the user's work.
- Capture expected versus actual behavior, the smallest input that triggers it, and the relevant environment. Use pui_ask_questions for missing reproduction facts the repository cannot answer.
- Reproduce with a bounded local command or test. If reproduction is unsafe, requires credentials, or touches production, stop and ask for a safe alternative. Do not print secrets or collect unrelated data.

## Narrow the cause

- Follow the input through the relevant state transitions or call path. Record where behavior first differs from the expectation.
- Keep a short list of plausible causes and the observation that would distinguish each one.
- Run the cheapest discriminating check first, changing one variable at a time. Avoid random edits, sleeps, broad retries, or extra logging without a question to answer.
- For intermittent failures, record frequency and conditions. For performance, compare measurements under the same workload; do not infer speed from code shape alone.
- If evidence contradicts a hypothesis, discard it. If the problem cannot be reproduced, report that limit and propose the next observation rather than asserting a root cause.

## Fix within scope

When the user asked for a fix, capture the failure in a regression test where practical, verify that it fails for the observed reason, then make a narrow correction. If asked only to diagnose, report the proposed fix without editing. Remove temporary instrumentation you added, not pre-existing diagnostics. Re-run the reproduction, affected tests, and relevant lint checks.

Report the symptom, supported cause, changed files, checks actually run, and residual risk. Distinguish a workaround from a root-cause fix.

Do not run destructive cleanup, restart services, install dependencies, commit, push, or deploy without explicit authorization. Use Pui-managed workers only if bounded independent investigation would help; never launch agents through shell commands. Debugging does not authorize bypassing permissions.
